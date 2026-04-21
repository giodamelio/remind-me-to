use std::path::Path;

use crate::errors::{ScanError, ScanResult};
use crate::ops::types::{ForgeRef, IssueRef, Operation, RefRef, Reminder};

/// The case-insensitive marker we search for in each line.
const MARKER: &str = "remind-me-to:";

/// All known operation names.
const KNOWN_OPS: &[&str] = &[
    "pr_merged",
    "pr_closed",
    "tag_exists",
    "commit_released",
    "pr_released",
    "issue_closed",
    "branch_deleted",
    "date_passed",
];

/// Known comment prefix patterns. We only recognize a REMIND-ME-TO marker
/// if the text before it on the line looks like a comment.
const COMMENT_PREFIXES: &[&str] = &[
    "//", "#", "--", "/*", "<!--", "%", ";", "*",
];

/// Parse an entire file, returning all reminders found and any parse errors.
pub fn parse_file(path: &Path, content: &str) -> ScanResult {
    let mut reminders = Vec::new();
    let mut errors = Vec::new();

    for (line_idx, line) in content.lines().enumerate() {
        let line_num = line_idx + 1;
        let lower = line.to_lowercase();

        if let Some(marker_pos) = lower.find(MARKER) {
            // Only recognize the marker if it appears inside a comment.
            // Check that the text before the marker contains a comment prefix.
            let before_marker = &line[..marker_pos];
            if !looks_like_comment(before_marker) {
                continue;
            }

            let after_marker = marker_pos + MARKER.len();
            let remainder = &line[after_marker..];

            let (reminder, line_errors) = parse_reminder_line(path, line_num, remainder, line);
            if let Some(r) = reminder {
                reminders.push(r);
            }
            errors.extend(line_errors);
        }
    }

    ScanResult { reminders, errors }
}

/// Check if the text before the marker looks like it's inside a comment.
///
/// We look for a recognized comment prefix in the preceding text, but only
/// if the prefix appears *outside* of string literals. A simple heuristic:
/// the first non-whitespace content on the line should start with a comment
/// prefix, not a quote character or code.
fn looks_like_comment(before_marker: &str) -> bool {
    let trimmed = before_marker.trim();

    // If the line starts with a string delimiter before any comment prefix,
    // this is likely a string literal containing the marker, not a real comment.
    // Check if a comment prefix appears at the very start of meaningful content.
    for prefix in COMMENT_PREFIXES {
        if trimmed.starts_with(prefix) {
            return true;
        }
    }

    // Also handle block comment continuations and inline comments after code.
    // For inline, check if a comment prefix appears AND no unmatched quotes precede it.
    for prefix in COMMENT_PREFIXES {
        if let Some(pos) = trimmed.rfind(prefix) {
            let before_prefix = &trimmed[..pos];
            // Count quotes — if the quote count is even, the prefix is outside strings
            let double_quotes = before_prefix.chars().filter(|&c| c == '"').count();
            if double_quotes % 2 == 0 {
                return true;
            }
        }
    }

    false
}

/// Parse the content after the reminder marker on a single line.
/// Returns a Reminder (if any operations or description found) and any parse errors.
fn parse_reminder_line(
    path: &Path,
    line_num: usize,
    remainder: &str,
    full_line: &str,
) -> (Option<Reminder>, Vec<ScanError>) {
    let mut operations = Vec::new();
    let mut description_parts = Vec::new();
    let mut errors = Vec::new();

    let tokens = tokenize(remainder);

    for token in &tokens {
        if let Some(eq_pos) = token.find('=') {
            let key = &token[..eq_pos];
            let value = &token[eq_pos + 1..];

            if KNOWN_OPS.contains(&key) {
                match parse_operation(key, value) {
                    Ok(op) => operations.push(op),
                    Err(msg) => {
                        errors.push(ScanError::Parse {
                            file: path.to_owned(),
                            line: line_num,
                            col: 0,
                            message: msg,
                            span: 0..token.len(),
                            source_line: full_line.to_string(),
                            expected: vec![format!("valid value for {key}")],
                            found: Some(value.to_string()),
                        });
                    }
                }
            } else {
                // Unknown key=value — treat as description
                description_parts.push(*token);
            }
        } else {
            // Not a key=value token — treat as description
            description_parts.push(*token);
        }
    }

    let description = description_parts
        .into_iter()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();

    if operations.is_empty() && description.is_empty() && errors.is_empty() {
        return (None, errors);
    }

    let reminder = Reminder {
        file: path.to_owned(),
        line: line_num,
        description,
        operations,
    };

    (Some(reminder), errors)
}

/// Split remainder into whitespace-delimited tokens.
fn tokenize(input: &str) -> Vec<&str> {
    input.split_whitespace().collect()
}

/// Parse an operation value given the operation name.
fn parse_operation(name: &str, value: &str) -> Result<Operation, String> {
    match name {
        "pr_merged" => parse_issue_ref(value).map(Operation::PrMerged),
        "pr_closed" => parse_issue_ref(value).map(Operation::PrClosed),
        "pr_released" => parse_issue_ref(value).map(Operation::PrReleased),
        "issue_closed" => parse_issue_ref(value).map(Operation::IssueClosed),
        "tag_exists" => parse_ref_ref(value).map(Operation::TagExists),
        "commit_released" => parse_ref_ref(value).map(Operation::CommitReleased),
        "branch_deleted" => parse_ref_ref(value).map(Operation::BranchDeleted),
        "date_passed" => parse_date(value).map(Operation::DatePassed),
        _ => Err(format!("unknown operation: {name}")),
    }
}

/// Parse a forge issue reference like `github:owner/repo#123`
fn parse_issue_ref(value: &str) -> Result<IssueRef, String> {
    let (forge_ref, after_repo) = parse_forge_ref_prefix(value)?;

    let rest = after_repo
        .strip_prefix('#')
        .ok_or_else(|| format!("expected '#' after repo in '{value}', got '{after_repo}'"))?;

    let number: u64 = rest
        .parse()
        .map_err(|_| format!("expected issue/PR number after '#', got '{rest}'"))?;

    Ok(IssueRef {
        forge_ref,
        number,
    })
}

/// Parse a forge ref reference like `github:owner/repo@constraint`
fn parse_ref_ref(value: &str) -> Result<RefRef, String> {
    let (forge_ref, after_repo) = parse_forge_ref_prefix(value)?;

    let rest = after_repo
        .strip_prefix('@')
        .ok_or_else(|| format!("expected '@' after repo in '{value}', got '{after_repo}'"))?;

    if rest.is_empty() {
        return Err(format!("expected value after '@' in '{value}'"));
    }

    Ok(RefRef {
        forge_ref,
        value: rest.to_string(),
    })
}

/// Parse the `github:owner/repo` prefix, returning the ForgeRef and remaining string.
fn parse_forge_ref_prefix(value: &str) -> Result<(ForgeRef, &str), String> {
    let (forge, rest) = value
        .split_once(':')
        .ok_or_else(|| format!("expected forge prefix (e.g., 'github:') in '{value}'"))?;

    let (owner, rest) = rest
        .split_once('/')
        .ok_or_else(|| format!("expected 'owner/repo' after '{forge}:' in '{value}'"))?;

    if owner.is_empty() {
        return Err(format!("empty owner in '{value}'"));
    }

    // The repo name ends at '#' or '@' (the sigil)
    let repo_end = rest
        .find(['#', '@'])
        .unwrap_or(rest.len());

    let repo = &rest[..repo_end];
    let remaining = &rest[repo_end..];

    if repo.is_empty() {
        return Err(format!("empty repo in '{value}'"));
    }

    Ok((
        ForgeRef {
            forge: forge.to_string(),
            owner: owner.to_string(),
            repo: repo.to_string(),
        },
        remaining,
    ))
}

/// Parse a date string. Accepts `YYYY-MM-DD` or RFC 3339 with time.
fn parse_date(value: &str) -> Result<String, String> {
    // Basic validation: must start with YYYY-MM-DD pattern
    if value.len() < 10 {
        return Err(format!(
            "expected date in YYYY-MM-DD format, got '{value}'"
        ));
    }

    let date_part = &value[..10];

    // Validate basic date format
    let parts: Vec<&str> = date_part.split('-').collect();
    if parts.len() != 3 {
        return Err(format!(
            "expected date in YYYY-MM-DD format, got '{value}'"
        ));
    }

    let year: u32 = parts[0]
        .parse()
        .map_err(|_| format!("invalid year in '{value}'"))?;
    let month: u32 = parts[1]
        .parse()
        .map_err(|_| format!("invalid month in '{value}'"))?;
    let day: u32 = parts[2]
        .parse()
        .map_err(|_| format!("invalid day in '{value}'"))?;

    if !(1970..=2100).contains(&year) {
        return Err(format!("year out of range in '{value}'"));
    }
    if !(1..=12).contains(&month) {
        return Err(format!("month out of range in '{value}'"));
    }
    if !(1..=31).contains(&day) {
        return Err(format!("day out of range in '{value}'"));
    }

    // If there's more after the date, validate it looks like a time component
    if value.len() > 10 {
        let separator = value.as_bytes()[10];
        if separator != b'T' && separator != b't' {
            return Err(format!(
                "expected 'T' after date in RFC 3339 datetime, got '{value}'"
            ));
        }
    }

    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    // ---- Marker detection ----

    #[test]
    fn finds_marker_case_insensitive() {
        let path = PathBuf::from("test.rs");

        let r1 = parse_file(&path, "// REMIND-ME-TO: do something pr_merged=github:foo/bar#1");
        assert_eq!(r1.reminders.len(), 1);

        let r2 = parse_file(&path, "// remind-me-to: do something pr_merged=github:foo/bar#1");
        assert_eq!(r2.reminders.len(), 1);

        let r3 = parse_file(&path, "// Remind-Me-To: do something pr_merged=github:foo/bar#1");
        assert_eq!(r3.reminders.len(), 1);
    }

    #[test]
    fn no_marker_returns_empty() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(&path, "// This is a normal comment\nfn main() {}");
        assert!(result.reminders.is_empty());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn multiple_reminders_in_file() {
        let path = PathBuf::from("test.rs");
        let content = "// REMIND-ME-TO: first pr_merged=github:a/b#1\n\
                        fn foo() {}\n\
                        // REMIND-ME-TO: second issue_closed=github:c/d#2\n";
        let result = parse_file(&path, content);
        assert_eq!(result.reminders.len(), 2);
        assert_eq!(result.reminders[0].line, 1);
        assert_eq!(result.reminders[1].line, 3);
    }

    // ---- Operation parsing: pr_merged ----

    #[test]
    fn parse_pr_merged() {
        let path = PathBuf::from("test.rs");
        let result =
            parse_file(&path, "// REMIND-ME-TO: fix this pr_merged=github:tokio-rs/tokio#5432");
        assert_eq!(result.reminders.len(), 1);
        let op = &result.reminders[0].operations[0];
        match op {
            Operation::PrMerged(r) => {
                assert_eq!(r.forge_ref.forge, "github");
                assert_eq!(r.forge_ref.owner, "tokio-rs");
                assert_eq!(r.forge_ref.repo, "tokio");
                assert_eq!(r.number, 5432);
            }
            _ => panic!("expected PrMerged, got {op:?}"),
        }
    }

    #[test]
    fn parse_pr_closed() {
        let path = PathBuf::from("test.rs");
        let result =
            parse_file(&path, "// REMIND-ME-TO: cleanup pr_closed=github:owner/repo#99");
        assert_eq!(result.reminders[0].operations.len(), 1);
        assert!(matches!(
            &result.reminders[0].operations[0],
            Operation::PrClosed(_)
        ));
    }

    // ---- Operation parsing: tag_exists ----

    #[test]
    fn parse_tag_exists() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(
            &path,
            "// REMIND-ME-TO: upgrade tag_exists=github:serde-rs/serde@>=2.0.0",
        );
        assert_eq!(result.reminders.len(), 1);
        let op = &result.reminders[0].operations[0];
        match op {
            Operation::TagExists(r) => {
                assert_eq!(r.forge_ref.owner, "serde-rs");
                assert_eq!(r.forge_ref.repo, "serde");
                assert_eq!(r.value, ">=2.0.0");
            }
            _ => panic!("expected TagExists, got {op:?}"),
        }
    }

    // ---- Operation parsing: commit_released ----

    #[test]
    fn parse_commit_released() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(
            &path,
            "// REMIND-ME-TO: remove hack commit_released=github:foo/bar@abc1234",
        );
        let op = &result.reminders[0].operations[0];
        match op {
            Operation::CommitReleased(r) => {
                assert_eq!(r.value, "abc1234");
            }
            _ => panic!("expected CommitReleased, got {op:?}"),
        }
    }

    // ---- Operation parsing: pr_released ----

    #[test]
    fn parse_pr_released() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(
            &path,
            "// REMIND-ME-TO: update pr_released=github:foo/bar#42",
        );
        assert!(matches!(
            &result.reminders[0].operations[0],
            Operation::PrReleased(_)
        ));
    }

    // ---- Operation parsing: issue_closed ----

    #[test]
    fn parse_issue_closed() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(
            &path,
            "// REMIND-ME-TO: remove workaround issue_closed=github:foo/bar#456",
        );
        let op = &result.reminders[0].operations[0];
        match op {
            Operation::IssueClosed(r) => {
                assert_eq!(r.number, 456);
            }
            _ => panic!("expected IssueClosed, got {op:?}"),
        }
    }

    // ---- Operation parsing: branch_deleted ----

    #[test]
    fn parse_branch_deleted() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(
            &path,
            "// REMIND-ME-TO: cleanup branch_deleted=github:foo/bar@feature-branch",
        );
        let op = &result.reminders[0].operations[0];
        match op {
            Operation::BranchDeleted(r) => {
                assert_eq!(r.value, "feature-branch");
            }
            _ => panic!("expected BranchDeleted, got {op:?}"),
        }
    }

    // ---- Operation parsing: date_passed ----

    #[test]
    fn parse_date_passed() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(
            &path,
            "// REMIND-ME-TO: review date_passed=2025-06-01",
        );
        let op = &result.reminders[0].operations[0];
        match op {
            Operation::DatePassed(d) => {
                assert_eq!(d, "2025-06-01");
            }
            _ => panic!("expected DatePassed, got {op:?}"),
        }
    }

    #[test]
    fn parse_date_passed_rfc3339() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(
            &path,
            "// REMIND-ME-TO: review date_passed=2025-06-01T15:30:00Z",
        );
        let op = &result.reminders[0].operations[0];
        match op {
            Operation::DatePassed(d) => {
                assert_eq!(d, "2025-06-01T15:30:00Z");
            }
            _ => panic!("expected DatePassed, got {op:?}"),
        }
    }

    // ---- Description extraction ----

    #[test]
    fn extracts_description() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(
            &path,
            "// REMIND-ME-TO: Remove this override when the upstream bug is fixed pr_merged=github:tokio-rs/tokio#5432",
        );
        assert_eq!(
            result.reminders[0].description,
            "Remove this override when the upstream bug is fixed"
        );
    }

    #[test]
    fn description_only_no_operations() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(&path, "// REMIND-ME-TO: just a note with no operations");
        assert_eq!(result.reminders.len(), 1);
        assert!(result.reminders[0].operations.is_empty());
        assert_eq!(
            result.reminders[0].description,
            "just a note with no operations"
        );
    }

    #[test]
    fn unknown_key_value_treated_as_description() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(
            &path,
            "// REMIND-ME-TO: when x=5 is fixed pr_merged=github:a/b#1",
        );
        assert_eq!(result.reminders[0].description, "when x=5 is fixed");
        assert_eq!(result.reminders[0].operations.len(), 1);
    }

    // ---- Multiple operations ----

    #[test]
    fn multiple_operations_or_semantics() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(
            &path,
            "// REMIND-ME-TO: Remove custom TLS config pr_merged=github:hyper-rs/hyper#3210 tag_exists=github:hyper-rs/hyper@>=1.5.0",
        );
        assert_eq!(result.reminders[0].operations.len(), 2);
        assert!(matches!(
            &result.reminders[0].operations[0],
            Operation::PrMerged(_)
        ));
        assert!(matches!(
            &result.reminders[0].operations[1],
            Operation::TagExists(_)
        ));
    }

    // ---- Error handling ----

    #[test]
    fn known_op_with_bad_value_is_error() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(
            &path,
            "// REMIND-ME-TO: fix pr_merged=bad_value",
        );
        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ScanError::Parse {
                message, found, ..
            } => {
                assert!(message.contains("forge prefix"));
                assert_eq!(found.as_deref(), Some("bad_value"));
            }
            _ => panic!("expected Parse error"),
        }
    }

    #[test]
    fn error_recovery_still_collects_valid_ops() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(
            &path,
            "// REMIND-ME-TO: fix pr_merged=bad_value tag_exists=github:a/b@>=1.0",
        );
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.reminders.len(), 1);
        assert_eq!(result.reminders[0].operations.len(), 1);
        assert!(matches!(
            &result.reminders[0].operations[0],
            Operation::TagExists(_)
        ));
    }

    // ---- Comment closers ----

    #[test]
    fn html_comment_closer_in_description() {
        let path = PathBuf::from("test.html");
        let result = parse_file(
            &path,
            "<!-- REMIND-ME-TO: Remove polyfill tag_exists=github:nicolo-ribaudo/tc39-proposal@>=1.0.0 -->",
        );
        assert_eq!(result.reminders.len(), 1);
        assert_eq!(result.reminders[0].operations.len(), 1);
        // "-->" becomes part of description
        assert!(result.reminders[0].description.contains("-->"));
    }

    #[test]
    fn block_comment_closer_in_description() {
        let path = PathBuf::from("test.c");
        let result = parse_file(
            &path,
            "/* REMIND-ME-TO: Remove hack pr_merged=github:a/b#1 */",
        );
        assert_eq!(result.reminders[0].operations.len(), 1);
        assert!(result.reminders[0].description.contains("*/"));
    }

    // ---- Various comment styles ----

    #[test]
    fn python_comment() {
        let path = PathBuf::from("test.py");
        let result = parse_file(
            &path,
            "# remind-me-to: Drop this fork tag_exists=github:serde-rs/serde@>=2.0.0",
        );
        assert_eq!(result.reminders.len(), 1);
        assert_eq!(result.reminders[0].operations.len(), 1);
    }

    #[test]
    fn lua_comment() {
        let path = PathBuf::from("test.lua");
        let result = parse_file(
            &path,
            "-- Remind-Me-To: Switch back to upstream pr_merged=github:neovim/neovim#28100",
        );
        assert_eq!(result.reminders.len(), 1);
    }

    // ---- Edge cases ----

    #[test]
    fn empty_file() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(&path, "");
        assert!(result.reminders.is_empty());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn marker_with_no_content() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(&path, "// REMIND-ME-TO:");
        assert!(result.reminders.is_empty());
    }

    #[test]
    fn marker_with_only_whitespace() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(&path, "// REMIND-ME-TO:   ");
        assert!(result.reminders.is_empty());
    }

    #[test]
    fn date_invalid() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(&path, "// REMIND-ME-TO: fix date_passed=not-a-date");
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn issue_ref_missing_number() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(&path, "// REMIND-ME-TO: fix pr_merged=github:a/b#notnum");
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn ref_ref_missing_at() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(&path, "// REMIND-ME-TO: fix tag_exists=github:a/b");
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn ref_ref_empty_value() {
        let path = PathBuf::from("test.rs");
        let result = parse_file(&path, "// REMIND-ME-TO: fix tag_exists=github:a/b@");
        assert_eq!(result.errors.len(), 1);
    }

    // ---- Snapshot tests ----

    #[test]
    fn snapshot_single_reminder() {
        let path = PathBuf::from("src/tls.rs");
        let result = parse_file(
            &path,
            "// REMIND-ME-TO: Remove custom TLS config when the fix lands pr_merged=github:hyper-rs/hyper#3210 tag_exists=github:hyper-rs/hyper@>=1.5.0",
        );
        insta::assert_debug_snapshot!(result.reminders);
    }

    #[test]
    fn snapshot_parse_error() {
        let path = PathBuf::from("src/bad.rs");
        let result = parse_file(
            &path,
            "// REMIND-ME-TO: fix this pr_merged=invalid_value",
        );
        insta::assert_debug_snapshot!(result.errors);
    }

    #[test]
    fn snapshot_mixed_valid_and_invalid() {
        let path = PathBuf::from("src/mixed.rs");
        let result = parse_file(
            &path,
            "// REMIND-ME-TO: fix pr_merged=bad tag_exists=github:a/b@>=1.0 issue_closed=github:c/d#5",
        );
        insta::assert_debug_snapshot!("mixed_reminders", &result.reminders);
        insta::assert_debug_snapshot!("mixed_errors", &result.errors);
    }
}
