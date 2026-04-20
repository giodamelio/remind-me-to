# Findings: CLI Snapshot Testing

## Summary

Use **`assert_cmd` + `insta` (via `insta-cmd`)** as the primary approach, with `assert_cmd` + `predicates` as a fallback for tests that need fine-grained programmatic control. This combination gives us the best balance of snapshot ergonomics, non-deterministic output handling, exit code testing, and temp directory setup -- all critical needs for `remind-me-to`.

## Candidate Analysis

### `trycmd`

**What it is:** File-based test harness from the assert-rs / cargo team. You write `.toml` or `.md` files describing a command and its expected output, and `trycmd` runs them all automatically.

**Strengths:**
- Extremely low boilerplate for simple "run command, check output" tests
- Tests double as documentation (especially `.md` format)
- Built-in sandbox mode (`fs.sandbox = true` in `.toml`) copies an `*.in/` directory into a temp directory
- Built-in substitution variables: `[EXE]`, `[CWD]`, `[ROOT]` for platform differences
- Supports `...` wildcard lines in expected output to skip non-deterministic sections
- Tests exit codes via `status.code` in `.toml` configuration
- Tests stdout and stderr separately

**Weaknesses:**
- **Limited temp directory setup**: The `fs.sandbox` + `*.in/` directory mechanism copies static fixture files, but you cannot run arbitrary setup code (e.g., `git init`, create dynamic files, set env vars per-test). This is a significant limitation for `remind-me-to` where we need git repos with remotes configured for context-aware shorthand resolution.
- **No programmatic hooks**: Cannot run setup/teardown logic, inject mock servers, or conditionally skip tests
- **Coarse non-deterministic handling**: The `...` wildcard skips entire line ranges but cannot sort output or do regex-based redaction within lines. For our parallel file walker producing non-deterministic file order, we would need to either sort output in the tool itself (bad) or accept that many tests would use `...` liberally (defeating the purpose of snapshot testing)
- **No ANSI color handling**: Output is compared as-is; stripping ANSI codes requires the binary to detect non-TTY and disable colors (which we should do anyway, but trycmd has no built-in filter for it)

**Verdict:** Good for simple CLI tools but too rigid for our needs. The inability to do programmatic setup (git repos, env vars, mock servers) and the weak non-deterministic output handling are dealbreakers.

### `assert_cmd` + `insta` (via `insta-cmd`)

**What it is:** `assert_cmd` runs your binary and captures output. `insta` provides snapshot testing with `cargo insta review` for interactive updates. `insta-cmd` bridges them with the `assert_cmd_snapshot!` macro.

**Strengths:**
- **Full programmatic control**: Tests are normal Rust functions. Set up temp directories with `tempfile`, initialize git repos, set env vars, start mock servers -- whatever you need
- **Excellent snapshot workflow**: `cargo insta review` shows diffs interactively, accept/reject per-snapshot. `cargo insta test --review` runs tests then reviews. `INSTA_UPDATE=always` for bulk updates
- **Powerful non-deterministic handling**:
  - `Settings::add_filter(regex, replacement)` for regex-based redaction (timestamps, paths, UUIDs)
  - `sorted_redaction()` for sorting sequences/maps with non-deterministic order
  - `set_sort_maps(true)` for forcing deterministic map serialization
  - Custom filters can normalize file paths, strip ANSI codes, sort output lines
- **ANSI color support**: Use `Settings::add_filter` with a regex like `\x1b\[[0-9;]*m` to strip ANSI codes, or use `strip-ansi-escapes` crate in the filter pipeline
- **Exit code testing**: `assert_cmd_snapshot!` captures exit code, stdout, and stderr in a single snapshot. Can also use `assert_cmd` directly for programmatic exit code checks
- **Inline or file snapshots**: Inline snapshots live in the test file (great for small outputs), file snapshots in `snapshots/` directory (great for large outputs)
- **Redaction for paths**: Built-in `Settings::add_filter` handles temp dir paths, home directories, etc.

**Weaknesses:**
- More boilerplate per test than trycmd (but less than manual assertions)
- Requires `cargo-insta` CLI tool for the review workflow (easy to install, but one more dev dependency)
- Snapshot files in `snapshots/` directories can clutter the repo (mitigated by inline snapshots for small tests)

**Example of handling non-deterministic file order:**
```rust
use insta::Settings;

#[test]
fn test_check_dry_run() {
    let dir = setup_test_fixture(); // creates temp dir with test files
    let mut settings = Settings::clone_current();
    // Sort output lines to handle non-deterministic file walk order
    settings.add_filter(r"\x1b\[[0-9;]*m", ""); // strip ANSI
    settings.bind(|| {
        let mut output = run_cli(&["check", "--dry-run", dir.path().to_str().unwrap()]);
        // Sort the output lines for deterministic comparison
        let mut lines: Vec<&str> = output.lines().collect();
        lines.sort();
        insta::assert_snapshot!(lines.join("\n"));
    });
}
```

**Verdict:** Best fit for `remind-me-to`. Handles all our requirements: complex setup, non-deterministic output, exit codes, ANSI colors, and has an excellent update workflow.

### `snapbox`

**What it is:** A snapshot-testing toolbox from the assert-rs team (same people as trycmd). Middle ground between trycmd's file-based approach and fully programmatic testing. In fact, trycmd is built on top of snapbox.

**Strengths:**
- Programmatic API similar to `assert_cmd` but with built-in snapshot capabilities
- Substitution variables (`[EXE]`, `[CWD]`, `[ROOT]`, `[..]` for wildcard matching)
- `filter::Redactions` for custom substitution/normalization
- Can be used file-based (like trycmd) or inline (like assert_cmd)
- `Command::new(bin).assert().stdout_matches(expected)` pattern
- Part of the cargo team's testing ecosystem, well-maintained

**Weaknesses:**
- Smaller community than insta -- fewer blog posts, examples, and StackOverflow answers
- Snapshot update workflow is less polished than `cargo insta review` (uses `SNAPSHOTS=overwrite` env var, no interactive review)
- Redaction system (`filter::Redactions`) is less mature than insta's filters + redactions
- No equivalent to insta's `sorted_redaction()` for non-deterministic ordering
- Documentation is sparser than insta's

**Verdict:** Solid middle ground but the update workflow and non-deterministic handling are weaker than insta. If we were already using trycmd and needed to escape to programmatic tests occasionally, snapbox would be the natural choice. But starting fresh, insta is better.

### `assert_cmd` + `predicates`

**What it is:** `assert_cmd` for running the binary, `predicates` crate for building assertion expressions. No snapshots -- purely programmatic assertions.

**Strengths:**
- Maximum flexibility and control
- No snapshot files to manage
- Can assert on arbitrary properties: exit code, output contains substring, output matches regex, etc.
- Good for tests where the exact output format doesn't matter (e.g., "exit code is 2 and stderr contains 'error'")

**Weaknesses:**
- Very verbose for testing exact output formatting
- No snapshot update workflow -- when output changes, you manually update string literals
- Easy to write tests that are too loose (checking `contains("error")` misses formatting regressions)
- Doesn't scale well for testing multiple output formats

**Verdict:** Use as a complement for specific assertions (exit codes, error conditions) but not as the primary approach for output testing.

## What Popular Tools Use

### ripgrep
- Uses a custom `WorkDir` test harness pattern (now available as the `cli_test_dir` crate)
- Integration tests in `tests/tests.rs` with `autotests = false`
- Tests create temp directories, write fixture files, run the binary, and assert on output
- No snapshot testing -- uses manual string assertions
- This was designed before modern snapshot tools existed; new projects would likely choose differently

### bat
- Uses `assert_cmd` with manual assertions
- Tests in `tests/integration_tests.rs`
- Creates temp files for test fixtures
- Strips ANSI codes in test helpers for comparison

### fd
- Uses `assert_cmd` + custom test helpers
- Tests create temp directory trees with specific file structures
- Normalizes output (sorts lines) to handle non-deterministic walk order
- Manual string comparison after normalization

### delta (git-delta)
- Uses `insta` for snapshot testing of its output formatting
- Good precedent for a tool with complex, colorized terminal output

### cargo itself
- Uses `snapbox` and `trycmd` extensively for CLI testing
- This is where both tools originated -- they were built to test cargo
- Cargo's use case (testing a CLI with many subcommands and flags) is similar to ours

### Key takeaway
The trend among modern Rust CLI tools is moving toward snapshot testing. Older tools (ripgrep, fd) use custom harnesses with manual assertions because they predate the ecosystem. Newer projects and cargo itself use snapbox/trycmd/insta.

## Answers to Questions

### 1. Can trycmd handle tests needing temp directory setup with specific files?

**Partially.** trycmd supports `fs.sandbox = true` in the `.toml` config, which copies files from a `*.in/` fixture directory into a temp directory before running the command. This works for static fixtures (files with known content). However, it cannot:
- Run arbitrary setup code (e.g., `git init`, create `.git/config` with remotes)
- Set per-test environment variables dynamically
- Start/configure mock HTTP servers
- Create files with content derived from the test environment (e.g., absolute paths)

For `remind-me-to`, where we need git repos with configured remotes for shorthand resolution testing, trycmd's fixture mechanism is insufficient.

### 2. Does insta work well for multi-line CLI output with ANSI colors?

**Yes, with filters.** Insta's `Settings::add_filter` can strip ANSI escape codes before snapshotting:
```rust
settings.add_filter(r"\x1b\[[0-9;]*m", "");
```
Alternatively, since most CLI tools (including ours via clap) detect non-TTY output and disable colors automatically, the test binary likely won't emit ANSI codes when run via `assert_cmd` (which captures stdout/stderr via pipes, not a TTY). This means snapshots naturally contain plain text. If you want to test colored output specifically, you'd force color on (`--color=always`) and either snapshot with colors or strip them via filter.

### 3. How does snapbox compare to trycmd in practice?

- **trycmd** = file-based, batch-oriented, minimal Rust code. Best for "here are 50 commands and their expected outputs."
- **snapbox** = programmatic, flexible, Rust-native. Best for custom test harnesses or when you need more control than trycmd provides.
- trycmd is actually built on snapbox internally. snapbox exposes the lower-level primitives.
- In practice: trycmd for simple tools with many straightforward test cases; snapbox when you outgrow trycmd but want to stay in the assert-rs ecosystem.

### 4. Can we combine approaches?

**Yes, absolutely.** This is common and recommended:
- Use `assert_cmd` + `insta` (via `insta-cmd`) for the primary test suite: output format testing, complex setup scenarios, non-deterministic output
- Use `assert_cmd` + `predicates` for focused tests: exit codes, error messages contain expected text, quick smoke tests
- Optionally use `trycmd` for a small set of "golden" examples that also serve as documentation

All three approaches use standard `cargo test` and can coexist in the same `tests/` directory.

### 5. How do these handle non-deterministic output?

| Tool | Approach | Effectiveness for file-order non-determinism |
|------|----------|----------------------------------------------|
| **trycmd** | `...` wildcard lines skip sections | Weak -- skips too much, loses regression detection |
| **snapbox** | `[..]` wildcards, `filter::Redactions` | Moderate -- can redact but not sort |
| **insta** | `add_filter()` regex, `sorted_redaction()`, pre-process in test code | Strong -- sort lines in test, redact dynamic values, full control |
| **assert_cmd + predicates** | Full programmatic control | Strong -- but no snapshots |

For `remind-me-to`, the recommended approach is:
1. Sort output lines in the test before snapshotting (handles file walk order)
2. Use `add_filter()` to redact timestamps, temp paths, etc.
3. For JSON output format: use `assert_json_snapshot!` with `sorted_redaction()` on the results array

### 6. What do popular Rust CLI tools use?

See the "What Popular Tools Use" section above. Summary:
- **ripgrep, fd**: Custom `WorkDir` harness + manual assertions (pre-dates modern tools)
- **bat**: `assert_cmd` + manual assertions
- **delta**: `insta` snapshots
- **cargo**: `snapbox` + `trycmd` (where both tools originated)
- **Modern trend**: snapshot testing (insta or snapbox) is becoming standard

## Recommendation

**Primary: `assert_cmd` + `insta` via `insta-cmd`**

Add to `Cargo.toml` dev-dependencies:
```toml
[dev-dependencies]
assert_cmd = "2"
insta = { version = "1", features = ["filters"] }
insta-cmd = "0.6"
predicates = "3"
tempfile = "3"
assert_fs = "1"
```

Also install the CLI tool for the review workflow:
```bash
cargo install cargo-insta
```

**Rationale:**
1. **Complex setup needs**: We need temp directories with git repos, configured remotes, and specific file trees. `assert_cmd` gives full programmatic control for this.
2. **Non-deterministic file order**: Insta's filters + sorting output lines in tests handles this cleanly.
3. **Multiple output formats**: Snapshot testing is ideal for catching regressions across text/json/llm formats.
4. **Excellent update workflow**: `cargo insta review` makes intentional output changes painless.
5. **Exit code testing**: `assert_cmd` handles this natively; `insta-cmd` captures it in snapshots.
6. **Community and ecosystem**: insta is the most popular Rust snapshot testing crate (6M+ downloads), well-documented, actively maintained by Armin Ronacher (of Flask/Sentry fame).

**Secondary: `assert_cmd` + `predicates` for targeted assertions**

Use for quick smoke tests and error-condition tests where exact output doesn't matter:
```rust
cmd.assert().failure().code(2).stderr(predicate::str::contains("error parsing"));
```

## Example Test

```rust
// tests/cli_tests.rs

use assert_cmd::Command;
use insta_cmd::assert_cmd_snapshot;
use insta::Settings;
use tempfile::TempDir;
use std::fs;

/// Create a test fixture directory with files containing REMIND-ME-TO comments
fn setup_fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        r#"// REMIND-ME-TO: Remove this workaround when the fix is released  pr_merged=github:tokio-rs/tokio#5432
fn workaround() {}
"#,
    ).unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        r#"// REMIND-ME-TO: Drop this polyfill  date_passed=2025-06-01
fn polyfill() {}
"#,
    ).unwrap();
    dir
}

#[test]
fn test_dry_run_text_output() {
    let fixture = setup_fixture();
    let mut settings = Settings::clone_current();
    // Redact the temp directory path
    settings.add_filter(
        &regex::escape(fixture.path().to_str().unwrap()),
        "[FIXTURE]"
    );
    settings.bind(|| {
        assert_cmd_snapshot!(
            Command::cargo_bin("remind-me-to").unwrap()
                .arg("check")
                .arg("--dry-run")
                .arg(fixture.path()),
            @r"
            success: true
            exit_code: 0
            ----- stdout -----
            [FIXTURE]/main.rs:1: Remove this workaround when the fix is released
              ? pr_merged=github:tokio-rs/tokio#5432 (dry run, not checked)

            [FIXTURE]/lib.rs:1: Drop this polyfill
              ? date_passed=2025-06-01 (dry run, not checked)

            2 reminders found, 0 triggered (dry run)
            ----- stderr -----
            "
        );
    });
}

#[test]
fn test_dry_run_json_output() {
    let fixture = setup_fixture();
    let mut settings = Settings::clone_current();
    settings.add_filter(
        &regex::escape(fixture.path().to_str().unwrap()),
        "[FIXTURE]"
    );
    settings.bind(|| {
        assert_cmd_snapshot!(
            Command::cargo_bin("remind-me-to").unwrap()
                .arg("check")
                .arg("--dry-run")
                .arg("--format=json")
                .arg(fixture.path())
        );
        // JSON snapshot saved to snapshots/cli_tests__dry_run_json_output.snap
    });
}

#[test]
fn test_parse_error_exits_with_code_2() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("bad.rs"),
        "// REMIND-ME-TO: bad op  pr_merged=not_a_valid_value\n",
    ).unwrap();

    Command::cargo_bin("remind-me-to").unwrap()
        .arg("check")
        .arg("--dry-run")
        .arg(dir.path())
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains("error"));
}

#[test]
fn test_quiet_mode_no_output() {
    let fixture = setup_fixture();
    Command::cargo_bin("remind-me-to").unwrap()
        .arg("check")
        .arg("--dry-run")
        .arg("--quiet")
        .arg(fixture.path())
        .assert()
        .success()
        .stdout(predicates::str::is_empty());
}

/// For tests with non-deterministic file order, sort lines before snapshotting
#[test]
fn test_sorted_output_for_determinism() {
    let fixture = setup_fixture();
    let output = Command::cargo_bin("remind-me-to").unwrap()
        .arg("check")
        .arg("--dry-run")
        .arg(fixture.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    // Split into per-file blocks, sort blocks, then snapshot
    let mut blocks: Vec<&str> = stdout.split("\n\n").collect();
    blocks.sort();
    let sorted = blocks.join("\n\n");

    let mut settings = Settings::clone_current();
    settings.add_filter(
        &regex::escape(fixture.path().to_str().unwrap()),
        "[FIXTURE]"
    );
    settings.bind(|| {
        insta::assert_snapshot!(sorted);
    });
}
```

### Workflow

```bash
# Run tests (new/changed snapshots are saved as .snap.new files)
cargo test

# Review snapshot changes interactively
cargo insta review

# Or accept all changes at once (use with caution)
cargo insta test --review --accept
```
