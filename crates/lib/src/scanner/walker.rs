use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::mpsc;

use ignore::WalkBuilder;
use ignore::WalkState;

/// Walk multiple paths in parallel, respecting .gitignore/.ignore, skipping binaries.
/// Returns a list of file paths to scan.
pub fn walk_paths(
    paths: &[&Path],
    respect_gitignore: bool,
    extra_ignore_patterns: &[String],
) -> (Vec<ignore::DirEntry>, Vec<crate::errors::ScanError>) {
    if paths.is_empty() {
        log::debug!("no paths to walk");
        return (Vec::new(), Vec::new());
    }

    log::debug!("starting parallel file walk roots={}", paths.len());

    let mut builder = WalkBuilder::new(paths[0]);
    for path in &paths[1..] {
        builder.add(path);
    }

    builder
        .hidden(true)
        .ignore(true)
        .git_ignore(respect_gitignore)
        .git_global(respect_gitignore)
        .git_exclude(respect_gitignore)
        .max_filesize(Some(1_048_576)) // 1MB
        .threads(0); // auto-detect thread count

    // Add extra ignore patterns via an override
    if !extra_ignore_patterns.is_empty() {
        let mut overrides = ignore::overrides::OverrideBuilder::new(paths[0]);
        for pattern in extra_ignore_patterns {
            // Negate the pattern to make it an ignore (override globs are include by default)
            if let Err(e) = overrides.add(&format!("!{pattern}")) {
                log::warn!("invalid ignore pattern '{}': {}", pattern, e);
            }
        }
        if let Ok(built) = overrides.build() {
            builder.overrides(built);
        }
    }

    let (tx, rx) = mpsc::channel();
    let (err_tx, err_rx) = mpsc::channel();

    builder.build_parallel().run(|| {
        let tx = tx.clone();
        let err_tx = err_tx.clone();
        Box::new(move |result| match result {
            Ok(entry) => {
                if entry.file_type().is_some_and(|ft| ft.is_file()) {
                    if is_binary(entry.path()) {
                        log::trace!("skipping binary file file={}", entry.path().display());
                    } else {
                        log::trace!("found file file={}", entry.path().display());
                        let _ = tx.send(entry);
                    }
                }
                WalkState::Continue
            }
            Err(err) => {
                let _ = err_tx.send(crate::errors::ScanError::Walk {
                    path: Path::new("<walk error>").to_path_buf(),
                    message: err.to_string(),
                });
                WalkState::Continue
            }
        })
    });

    drop(tx);
    drop(err_tx);

    let entries: Vec<_> = rx.into_iter().collect();
    let errors: Vec<_> = err_rx.into_iter().collect();

    log::debug!(
        "walk finished files={} errors={}",
        entries.len(),
        errors.len()
    );

    (entries, errors)
}

/// Simple binary detection: check first 8KB for NUL bytes.
fn is_binary(path: &Path) -> bool {
    let mut buf = [0u8; 8192];
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let Ok(n) = file.read(&mut buf) else {
        return false;
    };
    buf[..n].contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn binary_detection_detects_nul_bytes() {
        let dir = TempDir::new().unwrap();
        let binary_path = dir.path().join("binary.bin");
        fs::write(&binary_path, b"hello\x00world").unwrap();
        assert!(is_binary(&binary_path));
    }

    #[test]
    fn binary_detection_passes_text() {
        let dir = TempDir::new().unwrap();
        let text_path = dir.path().join("text.txt");
        fs::write(&text_path, "hello world").unwrap();
        assert!(!is_binary(&text_path));
    }

    #[test]
    fn walks_directory_finds_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("b.rs"), "fn foo() {}").unwrap();

        let (entries, errors) = walk_paths(&[dir.path()], false, &[]);
        assert!(errors.is_empty());
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn skips_binary_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("text.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("binary.bin"), b"hello\x00world").unwrap();

        let (entries, errors) = walk_paths(&[dir.path()], false, &[]);
        assert!(errors.is_empty());
        assert_eq!(entries.len(), 1);
        assert!(entries[0].path().to_str().unwrap().contains("text.rs"));
    }

    #[test]
    fn respects_gitignore() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored.rs\n").unwrap();
        fs::write(dir.path().join("kept.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("ignored.rs"), "fn ignored() {}").unwrap();

        // Initialize a git repo so gitignore is respected
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let (entries, errors) = walk_paths(&[dir.path()], true, &[]);
        assert!(errors.is_empty());
        let names: Vec<_> = entries
            .iter()
            .map(|e| e.path().file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert!(names.contains(&"kept.rs".to_string()));
        assert!(!names.contains(&"ignored.rs".to_string()));
    }

    #[test]
    fn multiple_roots() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        fs::write(dir1.path().join("a.rs"), "a").unwrap();
        fs::write(dir2.path().join("b.rs"), "b").unwrap();

        let (entries, errors) = walk_paths(&[dir1.path(), dir2.path()], false, &[]);
        assert!(errors.is_empty());
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn single_file_path() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("single.rs");
        fs::write(&file_path, "fn main() {}").unwrap();

        let (entries, errors) = walk_paths(&[file_path.as_path()], false, &[]);
        assert!(errors.is_empty());
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn skips_large_files() {
        let dir = TempDir::new().unwrap();
        let large_path = dir.path().join("large.rs");
        // Write a file larger than 1MB
        let content = "x".repeat(1_048_577);
        fs::write(&large_path, content).unwrap();
        fs::write(dir.path().join("small.rs"), "fn main() {}").unwrap();

        let (entries, errors) = walk_paths(&[dir.path()], false, &[]);
        assert!(errors.is_empty());
        assert_eq!(entries.len(), 1);
        assert!(entries[0].path().to_str().unwrap().contains("small.rs"));
    }

    #[test]
    fn empty_paths_returns_empty() {
        let (entries, errors) = walk_paths(&[], false, &[]);
        assert!(entries.is_empty());
        assert!(errors.is_empty());
    }
}
