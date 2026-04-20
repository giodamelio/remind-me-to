use versions::{Requirement, Versioning};

/// Check if any tag in the list satisfies the given version constraint.
///
/// Returns `Some(tag_name)` of the first matching tag, or `None` if no match.
pub fn check_version_constraint(constraint: &str, tags: &[String]) -> Option<String> {
    // Try standard Requirement parsing first
    if let Some(req) = Requirement::new(constraint) {
        // Check if the requirement has a non-ideal version (which can cause matching issues)
        let has_non_ideal_version = req
            .version
            .as_ref()
            .is_some_and(|v| !matches!(v, Versioning::Ideal(_)));

        if !has_non_ideal_version {
            warn_if_non_semver_caret_tilde(constraint, &req);
            return check_with_requirement(&req, constraint, tags);
        }
        // Fall through to manual comparison for non-ideal versions
    }

    // Fall back to manual operator + version parsing for non-standard constraints
    if let Some((op, ver_str)) = parse_constraint_manual(constraint) {
        return check_with_manual_comparison(op, ver_str, tags);
    }

    tracing::error!("invalid version constraint '{}'", constraint);
    None
}

/// Check tags using the standard Requirement type.
fn check_with_requirement(req: &Requirement, constraint: &str, tags: &[String]) -> Option<String> {
    for tag in tags {
        let stripped = strip_version_prefix(tag);

        let versioning = match Versioning::new(stripped) {
            Some(v) => v,
            None => {
                tracing::debug!("skipping tag '{}': not a valid version", tag);
                continue;
            }
        };

        if is_prerelease(&versioning) && !constraint_targets_prerelease(constraint) {
            tracing::debug!("skipping pre-release tag '{}'", tag);
            continue;
        }

        if req.matches(&versioning) {
            tracing::debug!("tag '{}' satisfies constraint '{}'", tag, constraint);
            return Some(tag.clone());
        }
    }

    None
}

/// Parse a constraint string manually when Requirement::new fails.
/// Returns (operator, version_string).
fn parse_constraint_manual(constraint: &str) -> Option<(&str, &str)> {
    let trimmed = constraint.trim();
    if let Some(rest) = trimmed.strip_prefix(">=") {
        Some((">=", rest))
    } else if let Some(rest) = trimmed.strip_prefix("<=") {
        Some(("<=", rest))
    } else if let Some(rest) = trimmed.strip_prefix('>') {
        Some((">", rest))
    } else if let Some(rest) = trimmed.strip_prefix('<') {
        Some(("<", rest))
    } else if let Some(rest) = trimmed.strip_prefix('=') {
        Some(("=", rest))
    } else if let Some(rest) = trimmed.strip_prefix('^') {
        Some(("^", rest))
    } else if let Some(rest) = trimmed.strip_prefix('~') {
        Some(("~", rest))
    } else {
        None
    }
}

/// Check tags using manual version comparison for non-standard version formats.
fn check_with_manual_comparison(op: &str, ver_str: &str, tags: &[String]) -> Option<String> {
    let constraint_ver = Versioning::new(ver_str)?;

    for tag in tags {
        let stripped = strip_version_prefix(tag);

        let tag_ver = match Versioning::new(stripped) {
            Some(v) => v,
            None => continue,
        };

        if is_prerelease(&tag_ver) {
            continue;
        }

        let matches = match op {
            ">=" => tag_ver >= constraint_ver,
            ">" => tag_ver > constraint_ver,
            "<=" => tag_ver <= constraint_ver,
            "<" => tag_ver < constraint_ver,
            "=" => tag_ver == constraint_ver,
            // For ^ and ~ with non-semver, fall back to >= behavior with a warning
            "^" | "~" => {
                tracing::warn!(
                    "constraint '{}{}' uses ^/~ but version doesn't look like semver. \
                     Falling back to >= behavior. Consider using >= instead.",
                    op, ver_str
                );
                tag_ver >= constraint_ver
            }
            _ => false,
        };

        if matches {
            return Some(tag.clone());
        }
    }

    None
}

/// Strip v/V prefix from a tag name.
fn strip_version_prefix(tag: &str) -> &str {
    tag.strip_prefix('v')
        .or_else(|| tag.strip_prefix('V'))
        .unwrap_or(tag)
}

/// Check if a version looks like a pre-release.
fn is_prerelease(v: &Versioning) -> bool {
    match v {
        Versioning::Ideal(s) => s.pre_rel.is_some(),
        Versioning::General(v) => {
            let s = v.to_string();
            s.contains("-alpha")
                || s.contains("-beta")
                || s.contains("-rc")
                || s.contains("-dev")
                || s.contains("-pre")
        }
        Versioning::Complex(_) => false,
    }
}

/// Check if the constraint string explicitly targets pre-release versions.
fn constraint_targets_prerelease(constraint: &str) -> bool {
    let stripped = constraint.trim_start_matches(|c: char| !c.is_ascii_digit());
    stripped.contains('-')
}

/// Warn if ^ or ~ is used with a non-semver version in a standard Requirement.
fn warn_if_non_semver_caret_tilde(constraint: &str, req: &Requirement) {
    let trimmed = constraint.trim();
    let uses_caret_or_tilde = trimmed.starts_with('^') || trimmed.starts_with('~');

    if uses_caret_or_tilde
        && let Some(ref ver) = req.version
            && !matches!(ver, Versioning::Ideal(_)) {
                tracing::warn!(
                    "constraint '{}' uses ^/~ but version doesn't look like semver. \
                     Consider using >= instead.",
                    constraint
                );
            }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    // ---- Basic constraint operators ----

    #[test]
    fn greater_or_equal() {
        let t = tags(&["v1.9.0", "v2.0.0", "v2.1.3"]);
        assert!(check_version_constraint(">=2.0.0", &t).is_some());
    }

    #[test]
    fn greater_than() {
        let t = tags(&["v1.0.0", "v1.5.0"]);
        assert!(check_version_constraint(">1.0.0", &t).is_some());
        assert!(check_version_constraint(">1.5.0", &t).is_none());
    }

    #[test]
    fn less_than() {
        let t = tags(&["v1.0.0", "v2.0.0"]);
        assert!(check_version_constraint("<1.5.0", &t).is_some());
    }

    #[test]
    fn less_or_equal() {
        let t = tags(&["v1.0.0", "v2.0.0"]);
        assert!(check_version_constraint("<=1.0.0", &t).is_some());
    }

    #[test]
    fn exact_match() {
        let t = tags(&["v1.0.0", "v2.0.0"]);
        assert!(check_version_constraint("=2.0.0", &t).is_some());
        assert!(check_version_constraint("=1.5.0", &t).is_none());
    }

    #[test]
    fn caret_constraint() {
        let t = tags(&["v1.9.0", "v2.0.0", "v2.5.1", "v3.0.0"]);
        let result = check_version_constraint("^2.0", &t);
        assert!(result.is_some());
    }

    #[test]
    fn tilde_constraint() {
        let t = tags(&["v1.0.0", "v1.1.0", "v1.2.0", "v2.0.0"]);
        let result = check_version_constraint("~1.1", &t);
        assert!(result.is_some());
    }

    #[test]
    fn wildcard_constraint() {
        let t = tags(&["v1.0.0"]);
        assert!(check_version_constraint("*", &t).is_some());
    }

    // ---- Pre-release handling ----

    #[test]
    fn skips_prerelease_by_default() {
        let t = tags(&["v3.0.0-beta.1"]);
        assert!(check_version_constraint(">=2.0.0", &t).is_none());
    }

    #[test]
    fn includes_stable_versions_alongside_prereleases() {
        let t = tags(&["v2.0.0", "v3.0.0-beta.1"]);
        let result = check_version_constraint(">=2.0.0", &t);
        assert_eq!(result, Some("v2.0.0".to_string()));
    }

    // ---- Version prefix stripping ----

    #[test]
    fn strips_v_prefix() {
        let t = tags(&["v1.0.0"]);
        assert!(check_version_constraint(">=1.0.0", &t).is_some());
    }

    #[test]
    fn strips_capital_v_prefix() {
        let t = tags(&["V1.0.0"]);
        assert!(check_version_constraint(">=1.0.0", &t).is_some());
    }

    #[test]
    fn no_prefix_also_works() {
        let t = tags(&["1.0.0"]);
        assert!(check_version_constraint(">=1.0.0", &t).is_some());
    }

    // ---- Unparseable tags ----

    #[test]
    fn skips_unparseable_tags() {
        let t = tags(&["v2.0.0"]);
        let result = check_version_constraint(">=1.0.0", &t);
        assert_eq!(result, Some("v2.0.0".to_string()));
    }

    // ---- CalVer ----

    #[test]
    fn calver_comparison() {
        let t = tags(&["2024.12.01", "2025.01.15", "2025.03.01"]);
        assert!(check_version_constraint(">=2025.01", &t).is_some());
    }

    // ---- 4-segment versions ----

    #[test]
    fn four_segment_versions() {
        let t = tags(&["1.2.3.3", "1.2.3.4", "1.2.3.5"]);
        assert!(check_version_constraint(">=1.2.3.4", &t).is_some());
    }

    // ---- Invalid constraint ----

    #[test]
    fn invalid_constraint_returns_none() {
        let t = tags(&["v1.0.0"]);
        assert!(check_version_constraint("not_a_constraint!!!", &t).is_none());
    }

    // ---- No matching tags ----

    #[test]
    fn no_matching_tags() {
        let t = tags(&["v0.1.0", "v0.2.0"]);
        assert!(check_version_constraint(">=1.0.0", &t).is_none());
    }

    // ---- Empty tags ----

    #[test]
    fn empty_tags() {
        let t: Vec<String> = vec![];
        assert!(check_version_constraint(">=1.0.0", &t).is_none());
    }

    // ---- Requirements doc behavior examples ----

    #[test]
    fn behavior_example_1() {
        let t = tags(&["v1.9.0", "v2.0.0", "v2.1.3", "v3.0.0-beta.1"]);
        assert!(check_version_constraint(">=2.0.0", &t).is_some());
    }

    #[test]
    fn behavior_example_2() {
        let t = tags(&["v1.9.0", "v2.0.0", "v2.5.1", "v3.0.0"]);
        assert!(check_version_constraint("^2.0", &t).is_some());
    }

    #[test]
    fn behavior_example_3() {
        let t = tags(&["2024.12.01", "2025.01.15", "2025.03.01"]);
        assert!(check_version_constraint(">=2025.01", &t).is_some());
    }

    #[test]
    fn behavior_example_4() {
        let t = tags(&["1.2.3.3", "1.2.3.4", "1.2.3.5"]);
        assert!(check_version_constraint(">=1.2.3.4", &t).is_some());
    }

    // ---- Unit tests for helper functions ----

    #[test]
    fn strip_prefix_v() {
        assert_eq!(strip_version_prefix("v1.0.0"), "1.0.0");
        assert_eq!(strip_version_prefix("V1.0.0"), "1.0.0");
        assert_eq!(strip_version_prefix("1.0.0"), "1.0.0");
    }

    #[test]
    fn prerelease_detection() {
        let v = Versioning::new("1.0.0-beta.1").unwrap();
        assert!(is_prerelease(&v));

        let v = Versioning::new("1.0.0").unwrap();
        assert!(!is_prerelease(&v));
    }
}
