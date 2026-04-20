# Research: File Walking Crate

## Context

`remind-me-to` needs to recursively scan potentially hundreds of thousands of files across `~/projects` (thousands of git repos) in seconds. It must:

- Walk directories in parallel for speed
- Respect `.gitignore` files at each repo boundary
- Respect `.ignore` files (same format, same as ripgrep convention)
- Respect global gitignore (`core.excludesFile`)
- Skip binary files automatically
- Skip hidden files/directories by default
- Handle being run on a single file, a single directory, or multiple directories

The file walker is the foundation of the scan phase — it feeds files to the parser which looks for `REMIND-ME-TO:` markers.

## Candidates

### `ignore` (from BurntSushi/ripgrep)

- https://crates.io/crates/ignore
- Used by ripgrep, fd, and other popular tools
- Built specifically for this use case

### `jwalk`

- https://crates.io/crates/jwalk
- Parallel directory walker
- May need manual .gitignore handling

### `walkdir`

- https://crates.io/crates/walkdir
- Simple, single-threaded
- No .gitignore support built-in
- Also by BurntSushi

### Others?

- Check if there are newer alternatives that have emerged
- `globwalk`, `wax`, etc.

## Evaluation Criteria

1. **Parallel walking support** — does it use multiple threads for traversal?
2. **.gitignore compliance** — built-in or needs manual implementation?
3. **.ignore file support** — does it handle `.ignore` files (ripgrep convention)?
4. **Binary file detection** — built-in or needs manual implementation?
5. **API ergonomics** — how easy is it to integrate? Can we pass multiple root paths?
6. **Dependency count / compile time** — how heavy is it?
7. **Maintenance status** — actively maintained? Last release?
8. **Battle-tested** — used in production by major tools?
9. **Symlink handling** — how does it handle symlinks? (important for ~/projects which may have symlinks)
10. **Error handling** — what happens with permission errors, broken symlinks, etc.?

## Questions to Answer

1. Does `ignore` support starting from multiple root directories, or do we need to call it once per path?
2. What's the actual performance difference between `ignore` and `jwalk` for large directory trees?
3. If we use `ignore`, do we get `.ignore` file support for free or is it opt-in?
4. How does each handle the case where we're pointed at a single file (not a directory)?
5. Can we easily limit file reading (e.g., stop after finding markers) or does the walker just provide paths?
6. What's the threading model — does it use rayon, a thread pool, or something custom?

## Findings

<!-- To be filled in by research agent -->

## Recommendation

<!-- To be filled in by research agent -->
