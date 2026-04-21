pub mod git_context;
pub mod parser;
pub mod walker;

use std::path::Path;

use crate::errors::{ScanError, ScanResult};
use crate::ops::types::Reminder;

/// Scan the given paths for REMIND-ME-TO comments.
/// Walks directories recursively, respects .gitignore, skips binaries.
pub fn scan(
    paths: &[&Path],
    respect_gitignore: bool,
    extra_ignore_patterns: &[String],
) -> ScanResult {
    tracing::info!(
        paths = ?paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        respect_gitignore,
        "starting scan"
    );

    let (entries, mut errors) = walker::walk_paths(paths, respect_gitignore, extra_ignore_patterns);
    tracing::info!(files = entries.len(), walk_errors = errors.len(), "file walk complete");

    let mut all_reminders: Vec<Reminder> = Vec::new();

    for entry in entries {
        let path = entry.path();
        tracing::trace!(file = %path.display(), "scanning file");
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let result = parser::parse_file(path, &content);
                if !result.reminders.is_empty() {
                    tracing::debug!(
                        file = %path.display(),
                        reminders = result.reminders.len(),
                        "found reminders"
                    );
                }
                all_reminders.extend(result.reminders);
                errors.extend(result.errors);
            }
            Err(e) => {
                tracing::warn!(file = %path.display(), error = %e, "failed to read file");
                errors.push(ScanError::FileRead {
                    path: path.to_owned(),
                    source: e,
                });
            }
        }
    }

    tracing::info!(
        total_reminders = all_reminders.len(),
        total_errors = errors.len(),
        "scan complete"
    );

    ScanResult {
        reminders: all_reminders,
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn scan_finds_reminders_in_directory() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("a.rs"),
            "// REMIND-ME-TO: fix this pr_merged=github:foo/bar#1\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("b.py"),
            "# remind-me-to: update tag_exists=github:a/b@>=2.0\n",
        )
        .unwrap();
        fs::write(dir.path().join("c.txt"), "no reminders here\n").unwrap();

        let result = scan(&[dir.path()], false, &[]);
        assert_eq!(result.reminders.len(), 2);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn scan_collects_parse_errors() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("bad.rs"),
            "// REMIND-ME-TO: fix pr_merged=invalid\n",
        )
        .unwrap();

        let result = scan(&[dir.path()], false, &[]);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn scan_single_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("single.rs");
        fs::write(&file, "// REMIND-ME-TO: do it pr_merged=github:a/b#1\n").unwrap();

        let result = scan(&[file.as_path()], false, &[]);
        assert_eq!(result.reminders.len(), 1);
    }

    #[test]
    fn scan_skips_binary() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("binary.bin"),
            b"// REMIND-ME-TO: hidden \x00 pr_merged=github:a/b#1\n",
        )
        .unwrap();

        let result = scan(&[dir.path()], false, &[]);
        assert!(result.reminders.is_empty());
    }
}
