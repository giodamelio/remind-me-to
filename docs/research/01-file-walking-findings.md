# Findings: File Walking Crate

## Summary

The `ignore` crate (from BurntSushi/ripgrep) is the clear winner for `remind-me-to`. It provides parallel directory walking via crossbeam work-stealing, built-in support for `.gitignore`, `.ignore`, `.git/info/exclude`, and global gitignore (`core.excludesFile`) -- all enabled by default. It natively supports multiple root directories, hidden file skipping, and has been battle-tested by ripgrep, fd, delta, and dozens of other high-profile tools.

## Candidate Analysis

### `ignore`

**Version:** 0.4.25  
**Repository:** BurntSushi/ripgrep (crates/ignore)  
**Downloads:** Millions (used by ripgrep, fd-find, delta, tokei, etc.)  
**License:** MIT / Unlicense  

**Strengths:**
- Parallel walking via crossbeam work-stealing deque (not rayon) -- custom thread pool with configurable thread count
- All ignore files respected by default: `.gitignore`, `.ignore`, `.git/info/exclude`, global gitignore
- `.ignore` files (ripgrep/ag convention) enabled out of the box
- Hidden files/dirs skipped by default
- Supports multiple root directories via `WalkBuilder::add()` with resource reuse across roots
- Single files passed as paths are handled correctly (treated as leaf nodes, symlink-followed automatically)
- `max_filesize()` filter to skip large files
- File type matching system (e.g., only search `.rs` files)
- Override globs for custom include/exclude patterns
- Symlinks not followed by default (configurable)
- Same-filesystem constraint available
- Battle-tested: powers ripgrep which searches millions of files daily across countless developer machines
- Actively maintained as part of the ripgrep project

**Weaknesses:**
- No built-in binary file detection (ripgrep handles this at a higher layer by inspecting file content)
- `WalkParallel` uses a closure-based API (`run(|| |entry| { ... })`) rather than `Iterator` -- less ergonomic but necessary for parallelism
- API designed for ripgrep's needs first, library users second (some rough edges)
- Cannot sort results (parallel walker yields in arbitrary order)

**Threading model:** Crossbeam work-stealing deques. Each thread gets a local deque; idle threads steal work from others. Thread count defaults to automatic heuristic (typically num_cpus) but is configurable via `threads()`.

**Dependencies:** crossbeam-deque, crossbeam-channel, globset, regex, memchr, walkdir (used internally for sequential walk), same-file, log

### `jwalk`

**Version:** 0.8.1 (last release: December 2022)  
**Repository:** Byron/jwalk  
**Stars:** ~272  
**License:** MIT  

**Strengths:**
- Parallel walking via rayon
- Streaming iterator API (implements standard `Iterator` trait)
- Results can be yielded in sorted order even with parallelism
- `process_read_dir` callback for custom filtering/sorting per directory
- ~4x faster than walkdir for sorted results with metadata

**Weaknesses:**
- No built-in gitignore support (must be implemented manually or combined with another crate)
- No `.ignore` file support
- No hidden file skipping by default
- No binary detection
- Last release over 3 years ago -- maintenance status unclear
- Smaller community and less battle-tested than `ignore`
- Parallelism is at directory level only (won't help for single directory with many entries)

**Threading model:** Rayon thread pool. Work is queued in depth-first order; rayon threads pull from shared queue.

### `walkdir`

**Version:** 2.5.0  
**Repository:** BurntSushi/walkdir  
**License:** MIT / Unlicense  

**Strengths:**
- Simple, clean `Iterator`-based API
- Well-tested, mature, stable
- Performance comparable to `find` and glibc's `nftw`
- Minimal dependencies
- Excellent error handling (errors are yielded as items, not panics)
- Symlink loop detection
- Configurable depth limits
- From same author as `ignore`

**Weaknesses:**
- Single-threaded only -- no parallelism
- No gitignore support
- No `.ignore` support
- No hidden file filtering built-in
- No binary detection

**Note:** `ignore` uses `walkdir` internally for its sequential walker.

### Others

**globwalk (0.9.1):**
- Built on top of `walkdir` and `globset`
- Glob-based file matching with multiple patterns and negation
- Single-threaded, no gitignore support
- In "perpetual maintenance mode" -- author recommends alternatives for new projects
- Not suitable for our use case

**wax:**
- Modern glob matching library with directory walking
- More expressive glob syntax than standard
- Single-threaded (uses walkdir underneath)
- No gitignore support
- Good for glob-heavy use cases but not our primary need

**gix-dir (from gitoxide):**
- Part of the gitoxide project (pure-Rust git implementation)
- Comprehensive gitignore handling
- Very git-aware but tightly coupled to gitoxide's architecture
- Heavier dependency, more complex API
- Worth watching but overkill for file walking

## Answers to Questions

1. **Does `ignore` support starting from multiple root directories, or do we need to call it once per path?**

   Yes, `ignore` natively supports multiple roots. Use `WalkBuilder::new(first_path)` then call `.add(second_path)`, `.add(third_path)`, etc. The documentation explicitly states: "Each additional file path added is traversed recursively. This should be preferred over building multiple Walk iterators since this enables reusing resources across iteration." All paths share the same thread pool and configuration.

2. **What's the actual performance difference between `ignore` and `jwalk` for large directory trees?**

   jwalk claims ~4x walkdir speed for sorted parallel results. `ignore`'s parallel walker is comparable in raw traversal speed to jwalk (both use work-stealing parallelism), but `ignore` adds overhead for gitignore pattern matching at each directory boundary. For our use case (where we *need* gitignore filtering), `ignore` is faster overall because it skips ignored subtrees entirely rather than walking them and filtering after the fact. In practice, skipping `node_modules/`, `target/`, `.git/`, and `vendor/` directories via gitignore saves far more time than any micro-benchmark difference in raw walking speed.

3. **If we use `ignore`, do we get `.ignore` file support for free or is it opt-in?**

   Free by default. The `ignore` crate respects `.ignore` files automatically. The `.ignore` file format is identical to `.gitignore` syntax and is supported by ripgrep, The Silver Searcher (ag), and other tools. It can be disabled via `WalkBuilder::ignore(false)` if needed.

4. **How does each handle the case where we're pointed at a single file (not a directory)?**

   - **`ignore`**: Handles single files correctly. When a file path is passed, it's treated as a leaf node and yielded directly. Symlink following is automatically enabled for file paths.
   - **`jwalk`**: No documented single-file handling. Likely errors or yields nothing.
   - **`walkdir`**: Yields the single file as the only entry (documented behavior).

5. **Can we easily limit file reading or does the walker just provide paths?**

   All three crates provide paths (as `DirEntry` structs with metadata), not file content. Reading is entirely up to the caller. `ignore` additionally provides `max_filesize()` to skip files above a size threshold without reading them. For binary detection, we would need to read the first few bytes ourselves (e.g., check for NUL bytes in the first 8KB, which is what ripgrep does at its application layer).

6. **What's the threading model -- rayon, thread pool, or custom?**

   - **`ignore`**: Custom work-stealing via crossbeam-deque. Not rayon. Each thread has a local deque; idle threads steal from others. Thread count configurable via `threads()` (defaults to heuristic based on CPU count).
   - **`jwalk`**: Rayon global thread pool.
   - **`walkdir`**: Single-threaded, no threading.

## Recommendation

**Use the `ignore` crate.** It is the only candidate that satisfies all requirements out of the box:

| Requirement | `ignore` | `jwalk` | `walkdir` |
|---|---|---|---|
| Parallel walking | Yes (crossbeam) | Yes (rayon) | No |
| .gitignore compliance | Yes (default) | No | No |
| .ignore support | Yes (default) | No | No |
| Global gitignore | Yes (default) | No | No |
| Skip hidden files/dirs | Yes (default) | No | No |
| Binary detection | No (DIY) | No | No |
| Multiple roots | Yes (native) | No | No |
| Single file support | Yes | Unclear | Yes |
| Battle-tested | Extremely | Moderate | Very |
| Actively maintained | Yes | Stale | Yes |

The only gap is binary file detection, which is straightforward to implement: read the first 8KB of each file and check for NUL bytes (the same heuristic git and ripgrep use). This is a ~5-line function.

For our use case of scanning ~/projects with thousands of git repos, `ignore`'s ability to skip entire ignored subtrees (node_modules, target, .git, vendor, build) at the directory level -- before even `stat()`ing their contents -- will provide massive performance wins that dwarf any raw walking speed differences.

## Example Usage

```rust
use ignore::WalkBuilder;
use ignore::WalkState;
use std::path::Path;
use std::sync::mpsc;

/// Walk multiple project directories in parallel, respecting all ignore files.
fn walk_projects(paths: &[&Path]) -> Vec<ignore::DirEntry> {
    let mut builder = WalkBuilder::new(paths[0]);

    // Add additional root directories
    for path in &paths[1..] {
        builder.add(path);
    }

    // Configuration (most defaults are already what we want)
    builder
        .hidden(true)          // skip hidden files (default: true)
        .ignore(true)          // respect .ignore files (default: true)
        .git_ignore(true)      // respect .gitignore (default: true)
        .git_global(true)      // respect global gitignore (default: true)
        .git_exclude(true)     // respect .git/info/exclude (default: true)
        .max_filesize(Some(1_048_576)) // skip files > 1MB
        .threads(0);           // auto-detect thread count

    // Parallel walking with channel to collect results
    let (tx, rx) = mpsc::channel();

    builder.build_parallel().run(|| {
        let tx = tx.clone();
        Box::new(move |result| {
            match result {
                Ok(entry) => {
                    // Skip directories (we only want files)
                    if entry.file_type().map_or(false, |ft| ft.is_file()) {
                        // Optional: skip binary files
                        if !is_binary(&entry) {
                            let _ = tx.send(entry);
                        }
                    }
                    WalkState::Continue
                }
                Err(err) => {
                    eprintln!("walk error: {}", err);
                    WalkState::Continue
                }
            }
        })
    });

    drop(tx);
    rx.into_iter().collect()
}

/// Simple binary detection: check first 8KB for NUL bytes.
fn is_binary(entry: &ignore::DirEntry) -> bool {
    use std::fs::File;
    use std::io::Read;

    let path = entry.path();
    let mut buf = [0u8; 8192];

    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let Ok(n) = file.read(&mut buf) else {
        return false;
    };

    buf[..n].contains(&0)
}
```

### Cargo.toml

```toml
[dependencies]
ignore = "0.4"
```
