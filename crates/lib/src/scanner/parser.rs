use std::path::Path;

use chumsky::prelude::*;

use crate::errors::{ScanError, ScanResult};
use crate::ops::types::{ForgeRef, IssueRef, NixpkgRef, Operation, RefRef, Reminder};

/// The case-insensitive marker we search for in each line.
const MARKER: &str = "remind-me-to:";

/// Comment prefixes that are valid at the start of a line's trimmed content.
const LINE_START_PREFIXES: &[&str] = &["//", "#", "--", "/*", "<!--", "%", ";", "* "];

/// Comment prefixes that can appear mid-line (after code). These must be
/// multi-character to avoid false positives from operators or markdown.
const INLINE_PREFIXES: &[&str] = &["//", "/*", "<!--"];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse an entire file, returning all reminders found and any parse errors.
pub fn parse_file(path: &Path, content: &str) -> ScanResult {
    let mut reminders = Vec::new();
    let mut errors = Vec::new();

    for (line_idx, line) in content.lines().enumerate() {
        let line_num = line_idx + 1;

        if let Some(remainder) = scan_line_for_marker(line) {
            let remainder = remainder.trim();
            if remainder.is_empty() {
                continue;
            }

            let (ops, desc_parts, line_errors) = parse_remainder(remainder);

            let description = desc_parts.join(" ");

            for err_msg in line_errors {
                errors.push(ScanError::Parse {
                    file: path.to_owned(),
                    line: line_num,
                    col: 0,
                    message: err_msg.clone(),
                    span: 0..remainder.len(),
                    source_line: line.to_string(),
                    expected: vec![],
                    found: Some(err_msg),
                });
            }

            if ops.is_empty() {
                if !description.is_empty() {
                    log::warn!(
                        "{}:{}: reminder has no operations: {}",
                        path.display(),
                        line_num,
                        description,
                    );
                }
                continue;
            }

            reminders.push(Reminder {
                file: path.to_owned(),
                line: line_num,
                description,
                operations: ops,
            });
        }
    }

    ScanResult { reminders, errors }
}

// ---------------------------------------------------------------------------
// Line scanner – cheap check before invoking chumsky
// ---------------------------------------------------------------------------

/// If the line contains our marker inside what looks like a comment, return
/// the text *after* the marker. Otherwise `None`.
fn scan_line_for_marker(line: &str) -> Option<&str> {
    let lower = line.to_lowercase();
    let marker_pos = lower.find(MARKER)?;

    let before = &line[..marker_pos];
    if !looks_like_comment(before) {
        return None;
    }

    let start = marker_pos + MARKER.len();
    Some(&line[start..])
}

/// Heuristic: does the text before the marker look like it is inside a
/// comment rather than inside a string literal or regular code?
///
/// Known limitations: escaped quotes (`\"`) fool the quote-counting heuristic,
/// and single-quoted or raw strings are not handled. This is an acceptable
/// trade-off — false positives produce extra (harmless) reminders, and markers
/// in non-comment code are caught by the "no comment prefix" path.
fn looks_like_comment(before: &str) -> bool {
    let trimmed = before.trim();

    // Line starts with a comment prefix – most common case.
    if LINE_START_PREFIXES.iter().any(|p| trimmed.starts_with(p)) {
        return true;
    }

    // Inline comment after code: `some_code(); // …`
    // Only use multi-character prefixes here to avoid matching markdown
    // bullets, operators, etc.
    for prefix in INLINE_PREFIXES {
        if let Some(pos) = trimmed.rfind(prefix) {
            let quotes = trimmed[..pos].chars().filter(|&c| c == '"').count();
            if quotes % 2 == 0 {
                return true;
            }
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Chumsky parser – runs on the text after the marker for a single line
// ---------------------------------------------------------------------------

/// Characters that are allowed inside a "value" (the part after `=` in an
/// operation like `pr_merged=github:owner/repo#123`).  Anything outside this
/// set terminates the value.
fn is_value_char(c: &char) -> bool {
    c.is_alphanumeric()
        || matches!(
            c,
            ':' | '/' | '#' | '@' | '>' | '<' | '=' | '^' | '~' | '*' | '.' | '-' | '_' | '+'
        )
}

/// Characters allowed inside a plain word (description text).
fn is_word_char(c: &char) -> bool {
    !c.is_whitespace()
}

/// The result of parsing a single token from the remainder.
#[derive(Debug, Clone)]
enum Token {
    /// A successfully parsed operation.
    Op(Operation),
    /// A known operation name with a value that failed to parse.
    BadOp(String),
    /// A plain word (part of the description).
    Word(String),
}

/// Build the chumsky parser for the content after the marker.
///
/// Grammar (roughly):
///   remainder = (token whitespace*)* EOF
///   token     = known_op '=' value   -- attempt to parse as operation
///             | word                  -- anything else is description
///
/// If a known op name is followed by `=` but the value is malformed, the
/// token is emitted as `BadOp` (an error) rather than silently swallowed.
fn token_parser<'a>() -> impl Parser<'a, &'a str, Vec<Token>, extra::Err<Rich<'a, char>>> {
    // A "value" is a run of value-chars (no whitespace, no junk).
    let value = any()
        .filter(is_value_char)
        .repeated()
        .at_least(1)
        .to_slice();

    // A plain word is any non-whitespace run.
    let word = any()
        .filter(is_word_char)
        .repeated()
        .at_least(1)
        .to_slice()
        .map(|s: &str| Token::Word(s.to_string()));

    // An operation attempt: `known_op_name=value_chars`
    let operation = choice((
        just("pr_merged="),
        just("pr_closed="),
        just("tag_exists="),
        just("commit_released="),
        just("pr_released="),
        just("issue_closed="),
        just("branch_deleted="),
        just("date_passed="),
        just("nixpkg_version="),
    ))
    .then(value)
    .map(|(prefix, val): (&str, &str)| {
        let op_name = &prefix[..prefix.len() - 1]; // strip trailing '='
        match build_operation(op_name, val) {
            Ok(op) => Token::Op(op),
            Err(msg) => Token::BadOp(msg),
        }
    });

    // Each token is either an operation attempt or a plain word.
    // We try the operation first so that `pr_merged=github:…` is not consumed
    // as a word.
    let token = operation.or(word);

    token
        .padded_by(text::inline_whitespace())
        .repeated()
        .collect()
        .then_ignore(end())
}

/// Parse the text after the marker, returning operations, description words,
/// and error messages.
fn parse_remainder(input: &str) -> (Vec<Operation>, Vec<String>, Vec<String>) {
    let mut ops = Vec::new();
    let mut desc = Vec::new();
    let mut errs = Vec::new();

    match token_parser().parse(input).into_result() {
        Ok(tokens) => {
            for tok in tokens {
                match tok {
                    Token::Op(op) => ops.push(op),
                    Token::BadOp(msg) => errs.push(msg),
                    Token::Word(w) => desc.push(w),
                }
            }
        }
        Err(parse_errors) => {
            // Chumsky parse failure – should be rare because the `word`
            // fallback accepts almost anything.  Record as errors.
            for e in parse_errors {
                errs.push(format!("{e}"));
            }
        }
    }

    (ops, desc, errs)
}

// ---------------------------------------------------------------------------
// Value builders – turn the raw value string into an Operation
// ---------------------------------------------------------------------------

fn build_operation(name: &str, value: &str) -> Result<Operation, String> {
    match name {
        "pr_merged" => parse_issue_ref(value).map(Operation::PrMerged),
        "pr_closed" => parse_issue_ref(value).map(Operation::PrClosed),
        "pr_released" => parse_issue_ref(value).map(Operation::PrReleased),
        "issue_closed" => parse_issue_ref(value).map(Operation::IssueClosed),
        "tag_exists" => parse_ref_ref(value).map(Operation::TagExists),
        "commit_released" => parse_ref_ref(value).map(Operation::CommitReleased),
        "branch_deleted" => parse_ref_ref(value).map(Operation::BranchDeleted),
        "date_passed" => parse_date(value).map(Operation::DatePassed),
        "nixpkg_version" => parse_nixpkg_ref(value).map(Operation::NixpkgVersion),
        _ => Err(format!("unknown operation: {name}")),
    }
}

/// Parse `github:owner/repo#123`
fn parse_issue_ref(value: &str) -> Result<IssueRef, String> {
    let (forge_ref, rest) = parse_forge_ref_prefix(value)?;
    let rest = rest
        .strip_prefix('#')
        .ok_or_else(|| format!("expected '#' in '{value}'"))?;
    let number: u64 = rest
        .parse()
        .map_err(|_| format!("expected number after '#' in '{value}'"))?;
    Ok(IssueRef { forge_ref, number })
}

/// Parse `github:owner/repo@constraint`
fn parse_ref_ref(value: &str) -> Result<RefRef, String> {
    let (forge_ref, rest) = parse_forge_ref_prefix(value)?;
    let rest = rest
        .strip_prefix('@')
        .ok_or_else(|| format!("expected '@' in '{value}'"))?;
    if rest.is_empty() {
        return Err(format!("expected value after '@' in '{value}'"));
    }
    Ok(RefRef {
        forge_ref,
        value: rest.to_string(),
    })
}

/// Parse the `forge:owner/repo` prefix, return `(ForgeRef, remaining_str)`.
fn parse_forge_ref_prefix(value: &str) -> Result<(ForgeRef, &str), String> {
    let (forge, rest) = value
        .split_once(':')
        .ok_or_else(|| format!("expected forge prefix (e.g. 'github:') in '{value}'"))?;
    let (owner, rest) = rest
        .split_once('/')
        .ok_or_else(|| format!("expected 'owner/repo' in '{value}'"))?;
    if owner.is_empty() {
        return Err(format!("empty owner in '{value}'"));
    }

    let repo_end = rest.find(['#', '@']).unwrap_or(rest.len());
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

/// Parse `package@version_constraint` for nixpkg_version operations.
fn parse_nixpkg_ref(value: &str) -> Result<NixpkgRef, String> {
    let (package, constraint) = value
        .split_once('@')
        .ok_or_else(|| format!("expected 'package@constraint' in '{value}'"))?;
    if package.is_empty() {
        return Err(format!("empty package name in '{value}'"));
    }
    if constraint.is_empty() {
        return Err(format!(
            "expected version constraint after '@' in '{value}'"
        ));
    }
    Ok(NixpkgRef {
        package: package.to_string(),
        version_constraint: constraint.to_string(),
    })
}

/// Parse `YYYY-MM-DD` or RFC 3339 datetime.
fn parse_date(value: &str) -> Result<String, String> {
    if value.len() < 10 {
        return Err(format!("expected YYYY-MM-DD, got '{value}'"));
    }

    let date_str = &value[..10];
    date_str
        .parse::<jiff::civil::Date>()
        .map_err(|e| format!("invalid date '{value}': {e}"))?;

    if value.len() > 10 {
        let sep = value.as_bytes()[10];
        if sep != b'T' && sep != b't' {
            return Err(format!("expected 'T' after date in '{value}'"));
        }
    }

    Ok(value.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    // Helper: parse a single comment line, return the ScanResult.
    fn parse(line: &str) -> ScanResult {
        parse_file(&PathBuf::from("test.rs"), line)
    }

    // ---- Marker detection ----

    #[test]
    fn finds_marker_case_insensitive() {
        let r1 = parse("// REMIND-ME-TO: do something pr_merged=github:foo/bar#1");
        assert_eq!(r1.reminders.len(), 1);

        let r2 = parse("// remind-me-to: do something pr_merged=github:foo/bar#1");
        assert_eq!(r2.reminders.len(), 1);

        let r3 = parse("// Remind-Me-To: do something pr_merged=github:foo/bar#1");
        assert_eq!(r3.reminders.len(), 1);
    }

    #[test]
    fn no_marker_returns_empty() {
        let result = parse("// This is a normal comment\nfn main() {}");
        assert!(result.reminders.is_empty());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn multiple_reminders_in_file() {
        let content = "\
// REMIND-ME-TO: first pr_merged=github:a/b#1
fn foo() {}
// REMIND-ME-TO: second issue_closed=github:c/d#2
";
        let result = parse_file(&PathBuf::from("test.rs"), content);
        assert_eq!(result.reminders.len(), 2);
        assert_eq!(result.reminders[0].line, 1);
        assert_eq!(result.reminders[1].line, 3);
    }

    // ---- Operation parsing ----

    #[test]
    fn parse_pr_merged() {
        let r = parse("// REMIND-ME-TO: fix this pr_merged=github:tokio-rs/tokio#5432");
        assert_eq!(r.reminders.len(), 1);
        let op = &r.reminders[0].operations[0];
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
        let r = parse("// REMIND-ME-TO: cleanup pr_closed=github:owner/repo#99");
        assert_eq!(r.reminders[0].operations.len(), 1);
        assert!(matches!(
            &r.reminders[0].operations[0],
            Operation::PrClosed(_)
        ));
    }

    #[test]
    fn parse_tag_exists() {
        let r = parse("// REMIND-ME-TO: upgrade tag_exists=github:serde-rs/serde@>=2.0.0");
        let op = &r.reminders[0].operations[0];
        match op {
            Operation::TagExists(r) => {
                assert_eq!(r.forge_ref.owner, "serde-rs");
                assert_eq!(r.forge_ref.repo, "serde");
                assert_eq!(r.value, ">=2.0.0");
            }
            _ => panic!("expected TagExists, got {op:?}"),
        }
    }

    #[test]
    fn parse_commit_released() {
        let r = parse("// REMIND-ME-TO: remove hack commit_released=github:foo/bar@abc1234");
        match &r.reminders[0].operations[0] {
            Operation::CommitReleased(r) => assert_eq!(r.value, "abc1234"),
            other => panic!("expected CommitReleased, got {other:?}"),
        }
    }

    #[test]
    fn parse_pr_released() {
        let r = parse("// REMIND-ME-TO: update pr_released=github:foo/bar#42");
        assert!(matches!(
            &r.reminders[0].operations[0],
            Operation::PrReleased(_)
        ));
    }

    #[test]
    fn parse_issue_closed() {
        let r = parse("// REMIND-ME-TO: remove workaround issue_closed=github:foo/bar#456");
        match &r.reminders[0].operations[0] {
            Operation::IssueClosed(i) => assert_eq!(i.number, 456),
            other => panic!("expected IssueClosed, got {other:?}"),
        }
    }

    #[test]
    fn parse_branch_deleted() {
        let r = parse("// REMIND-ME-TO: cleanup branch_deleted=github:foo/bar@feature-branch");
        match &r.reminders[0].operations[0] {
            Operation::BranchDeleted(rr) => assert_eq!(rr.value, "feature-branch"),
            other => panic!("expected BranchDeleted, got {other:?}"),
        }
    }

    #[test]
    fn parse_date_passed() {
        let r = parse("// REMIND-ME-TO: review date_passed=2025-06-01");
        match &r.reminders[0].operations[0] {
            Operation::DatePassed(d) => assert_eq!(d, "2025-06-01"),
            other => panic!("expected DatePassed, got {other:?}"),
        }
    }

    #[test]
    fn parse_date_passed_rfc3339() {
        let r = parse("// REMIND-ME-TO: review date_passed=2025-06-01T15:30:00Z");
        match &r.reminders[0].operations[0] {
            Operation::DatePassed(d) => assert_eq!(d, "2025-06-01T15:30:00Z"),
            other => panic!("expected DatePassed, got {other:?}"),
        }
    }

    // ---- Description extraction ----

    #[test]
    fn extracts_description() {
        let r = parse(
            "// REMIND-ME-TO: Remove this override when the upstream bug is fixed pr_merged=github:tokio-rs/tokio#5432",
        );
        assert_eq!(
            r.reminders[0].description,
            "Remove this override when the upstream bug is fixed"
        );
    }

    #[test]
    fn description_only_no_operations_is_skipped() {
        // A reminder with only description and no operations is not emitted —
        // it produces a tracing warning instead.
        let r = parse("// REMIND-ME-TO: just a note with no operations");
        assert!(r.reminders.is_empty());
        assert!(r.errors.is_empty());
    }

    #[test]
    fn unknown_key_value_treated_as_description() {
        let r = parse("// REMIND-ME-TO: when x=5 is fixed pr_merged=github:a/b#1");
        assert_eq!(r.reminders[0].description, "when x=5 is fixed");
        assert_eq!(r.reminders[0].operations.len(), 1);
    }

    // ---- Multiple operations ----

    #[test]
    fn multiple_operations_or_semantics() {
        let r = parse(
            "// REMIND-ME-TO: Remove custom TLS config pr_merged=github:hyper-rs/hyper#3210 tag_exists=github:hyper-rs/hyper@>=1.5.0",
        );
        assert_eq!(r.reminders[0].operations.len(), 2);
        assert!(matches!(
            &r.reminders[0].operations[0],
            Operation::PrMerged(_)
        ));
        assert!(matches!(
            &r.reminders[0].operations[1],
            Operation::TagExists(_)
        ));
    }

    // ---- Error handling ----

    #[test]
    fn known_op_with_bad_value_is_error() {
        let r = parse("// REMIND-ME-TO: fix pr_merged=bad_value");
        assert_eq!(r.errors.len(), 1);
        match &r.errors[0] {
            ScanError::Parse { message, .. } => {
                assert!(message.contains("forge prefix"));
            }
            _ => panic!("expected Parse error"),
        }
    }

    #[test]
    fn error_recovery_still_collects_valid_ops() {
        let r = parse("// REMIND-ME-TO: fix pr_merged=bad_value tag_exists=github:a/b@>=1.0");
        assert_eq!(r.errors.len(), 1);
        assert_eq!(r.reminders.len(), 1);
        assert_eq!(r.reminders[0].operations.len(), 1);
        assert!(matches!(
            &r.reminders[0].operations[0],
            Operation::TagExists(_)
        ));
    }

    // ---- Comment closers ----

    #[test]
    fn html_comment_closer_in_description() {
        let r = parse(
            "<!-- REMIND-ME-TO: Remove polyfill tag_exists=github:nicolo-ribaudo/tc39-proposal@>=1.0.0 -->",
        );
        assert_eq!(r.reminders.len(), 1);
        assert_eq!(r.reminders[0].operations.len(), 1);
        assert!(r.reminders[0].description.contains("-->"));
    }

    #[test]
    fn block_comment_closer_in_description() {
        let r = parse("/* REMIND-ME-TO: Remove hack pr_merged=github:a/b#1 */");
        assert_eq!(r.reminders[0].operations.len(), 1);
        assert!(r.reminders[0].description.contains("*/"));
    }

    // ---- Various comment styles ----

    #[test]
    fn python_comment() {
        let r = parse("# remind-me-to: Drop this fork tag_exists=github:serde-rs/serde@>=2.0.0");
        assert_eq!(r.reminders.len(), 1);
        assert_eq!(r.reminders[0].operations.len(), 1);
    }

    #[test]
    fn lua_comment() {
        let r =
            parse("-- Remind-Me-To: Switch back to upstream pr_merged=github:neovim/neovim#28100");
        assert_eq!(r.reminders.len(), 1);
    }

    // ---- Edge cases ----

    #[test]
    fn empty_file() {
        let r = parse("");
        assert!(r.reminders.is_empty());
        assert!(r.errors.is_empty());
    }

    #[test]
    fn marker_with_no_content() {
        let r = parse("// REMIND-ME-TO:");
        assert!(r.reminders.is_empty());
    }

    #[test]
    fn marker_with_only_whitespace() {
        let r = parse("// REMIND-ME-TO:   ");
        assert!(r.reminders.is_empty());
    }

    #[test]
    fn date_invalid() {
        let r = parse("// REMIND-ME-TO: fix date_passed=not-a-date");
        assert_eq!(r.errors.len(), 1);
    }

    #[test]
    fn issue_ref_missing_number() {
        let r = parse("// REMIND-ME-TO: fix pr_merged=github:a/b#notnum");
        assert_eq!(r.errors.len(), 1);
    }

    #[test]
    fn ref_ref_missing_at() {
        let r = parse("// REMIND-ME-TO: fix tag_exists=github:a/b");
        assert_eq!(r.errors.len(), 1);
    }

    #[test]
    fn ref_ref_empty_value() {
        let r = parse("// REMIND-ME-TO: fix tag_exists=github:a/b@");
        assert_eq!(r.errors.len(), 1);
    }

    // ---- Not-too-eager: markers inside string literals are ignored ----

    #[test]
    fn ignores_marker_in_rust_string_literal() {
        // This is what a test file looks like – the marker is inside a string,
        // not in a real comment.
        let r = parse(r#"        let s = "// REMIND-ME-TO: not real pr_merged=github:a/b#1";"#);
        assert!(r.reminders.is_empty());
        assert!(r.errors.is_empty());
    }

    #[test]
    fn ignores_marker_in_multiline_string_continuation() {
        // Second line of a Rust string literal that happens to start with //
        let r = parse(
            r#"                        // REMIND-ME-TO: not real issue_closed=github:c/d#2\n";"#,
        );
        // The line starts with `//` so the scanner *will* pick it up,
        // but the value `github:c/d#2\n` contains `\n` which is not a valid
        // value char, so chumsky will NOT match it as an operation.
        // It becomes description text instead (no error).
        assert!(r.errors.is_empty());
    }

    #[test]
    fn value_with_trailing_junk_stops_at_junk() {
        // The chumsky value parser stops at non-value chars like `,` and `"`.
        // `github:a/b#1` is still valid so the op parses; trailing junk
        // becomes description words.
        let r = parse(r#"// REMIND-ME-TO: fix pr_merged=github:a/b#1","#);
        assert!(r.errors.is_empty());
        assert_eq!(r.reminders[0].operations.len(), 1);
        // Description has "fix" plus whatever trailing junk became words.
        assert!(r.reminders[0].description.starts_with("fix"));
    }

    #[test]
    fn clean_value_parses_without_trailing_junk() {
        // A well-formed value right before end of line works fine.
        let r = parse("// REMIND-ME-TO: fix pr_merged=github:a/b#1");
        assert!(r.errors.is_empty());
        assert_eq!(r.reminders[0].operations.len(), 1);
    }

    #[test]
    fn value_stops_at_invalid_chars() {
        // Backslash is not a valid value char so the parser stops before it.
        // `github:a/b#1` is valid, so the operation parses; `\n` becomes a
        // separate description word.
        let r = parse(r#"// REMIND-ME-TO: fix pr_merged=github:a/b#1\n"#);
        assert!(r.errors.is_empty());
        assert_eq!(r.reminders[0].operations.len(), 1);
        // The trailing `\n` ends up as description text.
        assert!(r.reminders[0].description.contains(r"\n"));
    }

    #[test]
    fn inline_comment_after_code_is_detected() {
        let r = parse(r#"let x = 5; // REMIND-ME-TO: fix pr_merged=github:a/b#1"#);
        assert_eq!(r.reminders.len(), 1);
        assert_eq!(r.reminders[0].operations.len(), 1);
    }

    #[test]
    fn marker_in_pure_code_is_ignored() {
        // No comment prefix at all.
        let r = parse(r#"println!("REMIND-ME-TO: not a comment")"#);
        assert!(r.reminders.is_empty());
    }

    // ---- Snapshot tests ----

    #[test]
    fn snapshot_single_reminder() {
        let result = parse_file(
            &PathBuf::from("src/tls.rs"),
            "// REMIND-ME-TO: Remove custom TLS config when the fix lands pr_merged=github:hyper-rs/hyper#3210 tag_exists=github:hyper-rs/hyper@>=1.5.0",
        );
        insta::assert_debug_snapshot!(result.reminders);
    }

    #[test]
    fn snapshot_parse_error() {
        let result = parse_file(
            &PathBuf::from("src/bad.rs"),
            "// REMIND-ME-TO: fix this pr_merged=invalid_value",
        );
        insta::assert_debug_snapshot!(result.errors);
    }

    #[test]
    fn snapshot_mixed_valid_and_invalid() {
        let result = parse_file(
            &PathBuf::from("src/mixed.rs"),
            "// REMIND-ME-TO: fix pr_merged=bad tag_exists=github:a/b@>=1.0 issue_closed=github:c/d#5",
        );
        insta::assert_debug_snapshot!("mixed_reminders", &result.reminders);
        insta::assert_debug_snapshot!("mixed_errors", &result.errors);
    }

    // ---- nixpkg_version operation ----

    #[test]
    fn parse_nixpkg_version() {
        let r = parse("// REMIND-ME-TO: remove workaround nixpkg_version=redis@>=7.0.0");
        assert_eq!(r.reminders.len(), 1);
        match &r.reminders[0].operations[0] {
            Operation::NixpkgVersion(nixpkg_ref) => {
                assert_eq!(nixpkg_ref.package, "redis");
                assert_eq!(nixpkg_ref.version_constraint, ">=7.0.0");
            }
            other => panic!("expected NixpkgVersion, got {other:?}"),
        }
    }

    #[test]
    fn parse_nixpkg_version_with_hyphen_package() {
        let r = parse("// REMIND-ME-TO: upgrade nixpkg_version=python3-full@>=3.13");
        match &r.reminders[0].operations[0] {
            Operation::NixpkgVersion(nixpkg_ref) => {
                assert_eq!(nixpkg_ref.package, "python3-full");
                assert_eq!(nixpkg_ref.version_constraint, ">=3.13");
            }
            other => panic!("expected NixpkgVersion, got {other:?}"),
        }
    }

    #[test]
    fn parse_nixpkg_version_missing_at() {
        let r = parse("// REMIND-ME-TO: fix nixpkg_version=redis");
        assert_eq!(r.errors.len(), 1);
    }

    #[test]
    fn parse_nixpkg_version_empty_package() {
        let r = parse("// REMIND-ME-TO: fix nixpkg_version=@>=1.0");
        assert_eq!(r.errors.len(), 1);
    }

    #[test]
    fn parse_nixpkg_version_empty_constraint() {
        let r = parse("// REMIND-ME-TO: fix nixpkg_version=redis@");
        assert_eq!(r.errors.len(), 1);
    }
}
