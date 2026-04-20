# Research: CLI Snapshot Testing

## Context

`remind-me-to` needs thorough testing of its CLI output. The tool has multiple output formats (text, json, llm) and various flags (--verbose, --quiet, --dry-run) that change output behavior. We want to:

- Test that the CLI binary produces expected output for given inputs
- Catch regressions in output formatting
- Make it easy to update snapshots when output intentionally changes
- Test exit codes (0, 1, 2)
- Test error output (parse errors, network errors)

The test inputs are files containing `REMIND-ME-TO:` comments. For pure CLI tests (--dry-run mode), we don't need network access. For full integration tests, we'll mock the HTTP layer separately.

## Candidates

### `trycmd`

- https://crates.io/crates/trycmd
- File-based test cases (`.md` or `.toml` files that describe command + expected output)
- From the cargo team
- Very ergonomic for "lots of small CLI tests"

### `assert_cmd` + `insta`

- `assert_cmd`: https://crates.io/crates/assert_cmd — run binary and assert on output
- `insta`: https://crates.io/crates/insta — snapshot testing with `cargo insta review`
- More programmatic, Rust-native test code
- Good for complex setup scenarios

### `snapbox`

- https://crates.io/crates/snapbox
- Also from the cargo team
- Middle ground between trycmd and assert_cmd
- Can be used file-based or inline

### `assert_cmd` + `predicates`

- Standard approach without snapshots
- Manual assertions on stdout/stderr/exit code
- Most flexible but most verbose

## Evaluation Criteria

1. **Ease of adding new test cases** — how much boilerplate per test?
2. **Snapshot update workflow** — how easy to review and update when output changes?
3. **Exit code testing** — can we assert on specific exit codes?
4. **Stderr vs stdout** — can we test both independently?
5. **Test fixture management** — how do we provide input files (test repos with REMIND-ME-TO comments)?
6. **Filtering/redacting** — can we mask non-deterministic output (timestamps, file order)?
7. **Integration with cargo test** — standard `cargo test` or custom runner?
8. **Maintenance status** — actively maintained?

## Questions to Answer

1. Can `trycmd` handle tests where we need to set up a temp directory with specific files first?
2. Does `insta` work well for multi-line CLI output with ANSI colors?
3. How does `snapbox` compare to `trycmd` in practice? When would you pick one over the other?
4. Can we combine approaches? (e.g., `trycmd` for simple cases, `assert_cmd` + `insta` for complex ones)
5. How do these handle non-deterministic output (parallel walker means file order varies)?
6. What do popular Rust CLI tools use? (ripgrep, bat, fd, delta — check their test approaches)

## Findings

<!-- To be filled in by research agent -->

## Recommendation

<!-- To be filled in by research agent -->
