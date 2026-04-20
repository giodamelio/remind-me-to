# remind-me-to — Requirements

## Overview

A fast, self-contained CLI tool that scans source files for specially formatted comments indicating actions to check on. It queries external services (GitHub, Gitea, Forgejo, etc.) to determine if conditions have been met, then reports which items need attention.

**Primary use case:** Managing the lifecycle of patches, forks, workarounds, and overrides across many projects by embedding machine-checkable reminders directly in the code where the workaround lives.

## Comment Format

Comments are designed to read naturally first, with structured operations at the end.

```
// REMIND-ME-TO: <human description> <operation1> [<operation2> ...]
```

### Marker

The marker `REMIND-ME-TO:` is language-agnostic and **case-insensitive** — any comment prefix works, and `remind-me-to:`, `Remind-Me-To:`, `REMIND-ME-TO:` are all valid:

```rust
// REMIND-ME-TO: Remove this override when the upstream bug is fixed  pr_merged=github:tokio-rs/tokio#5432
```
```python
# remind-me-to: Drop this fork once v2.0 releases  tag_exists=github:serde-rs/serde@>=2.0.0
```
```lua
-- Remind-Me-To: Switch back to upstream  pr_merged=github:neovim/neovim#28100
```
```html
<!-- REMIND-ME-TO: Remove polyfill when baseline support lands  tag_exists=github:nicolo-ribaudo/tc39-proposal-awaiting@>=1.0.0 -->
```

### Parsing Rules

Parsing is done with `chumsky` for robust, well-tested parser combinators. The parser:

1. Finds lines containing the marker (case-insensitive)
2. Tokenizes by whitespace after the marker
3. For each `word=value` token:
   - If `word` is a recognized operation name AND the value parses correctly → it's an operation
   - If `word` is a recognized operation name BUT the value is malformed → emit an error (helps catch typos)
   - Otherwise → it's part of the human description
4. Everything that isn't an operation is collected as the human-readable description

This means `x=5` in prose is fine — it won't match a known operation name. But `pr_merged=bad_value` will error because `pr_merged` is recognized but the value doesn't parse.

**Whitespace-delimited tokenization** — operations must be separated by whitespace. Comma-separated operations without spaces are not supported (they'd be a parse error on the value).

**Comment syntax stripping** — trailing comment closers (`-->`, `*/`, etc.) are not operations and won't match any known operation name, so they're harmlessly treated as description text. If a closer is jammed against a value with no space (e.g., `pr_merged=github:foo/bar#123-->`), the value will fail to parse and emit an error, prompting the user to add a space.

### Multi-line Comments

**Deferred — not in MVP.** All operations must be on the same line as the `REMIND-ME-TO:` marker for now. Multi-line continuation may be added later.

### Design Principles

1. **Human-readable first** — the description is plain English, no special syntax required
2. **Operations are trailing key=value pairs** — easily distinguishable from prose
3. **Multiple operations per comment** — ANY must be satisfied to trigger (OR logic by default)
4. **Backwards compatible** — new operation types can be added without breaking old parsers; unknown operations emit a warning, not an error
5. **Language agnostic** — the tool doesn't need to understand the comment syntax of every language, just find lines containing the marker
6. **Forge agnostic** — GitHub is the first provider, but the design must support any git forge (Gitea, Forgejo, GitLab, etc.)

## Operations

Operations are `key=value` pairs at the end of the comment.

### Value Syntax

The value format uses sigils to distinguish reference types:

- `#` = issue-like references (PRs, issues): `github:owner/repo#123`
- `@` = ref-like references (versions, branches, commits): `github:owner/repo@>=1.2.0`, `github:owner/repo@abc1234`, `github:owner/repo@branch-name`

### MVP Operations

| Operation | Syntax | Triggers when |
|-----------|--------|---------------|
| `pr_merged` | `pr_merged=github:owner/repo#123` | The specified PR is merged |
| `pr_closed` | `pr_closed=github:owner/repo#123` | The specified PR is closed (merged or not) |
| `tag_exists` | `tag_exists=github:owner/repo@>=1.2.0` | A tag matching the version constraint exists |
| `commit_released` | `commit_released=github:owner/repo@abc1234` | The commit SHA appears in a tagged release |
| `pr_released` | `pr_released=github:owner/repo#123` | The PR's merge commit is included in a release (best-effort) |
| `issue_closed` | `issue_closed=github:owner/repo#456` | The specified issue is closed |
| `branch_deleted` | `branch_deleted=github:owner/repo@branch-name` | Branch no longer exists |
| `date_passed` | `date_passed=2025-06-01` | The specified date has passed |

#### Notes on `tag_exists`

This is the unified operation for "has a version been released that satisfies my constraint." The constraint syntax supports semver ranges (`>=1.2.0`, `^2.0`, `~1.5`).

Use cases:
- "Notify me when v2.0 drops": `tag_exists=github:owner/repo@>=2.0.0`
- "Notify me when there's a release after 1.3.2": `tag_exists=github:owner/repo@>1.3.2`

#### Notes on `commit_released` and `pr_released`

**What "in a release" means:** Check if the commit (or the PR's merge commit) is an ancestor of any tag that represents a release. Strategy:

1. First check GitHub Releases (the API feature) — these are explicitly marked as releases
2. Fall back to git tags that match a version pattern (e.g., `v1.2.3`, `1.2.3`)
3. For `pr_released`: get the PR's merge commit SHA (available from the PR API when merged)
4. Check if the commit is an ancestor of any release tag

**Challenges with `pr_released`:**
- Squash merges: the PR's original commits are gone, but the PR API still records the merge SHA → this works
- Rebase merges: individual commits are rewritten with new SHAs → the PR API records the new SHA → this should still work
- The GitHub PR API provides `merge_commit_sha` regardless of merge strategy

**Decision:** Include `pr_released` in MVP as best-effort. The GitHub API gives us `merge_commit_sha` on merged PRs, and we can check if that SHA is an ancestor of a release tag. Document that this is best-effort and may not work for unusual merge workflows.

#### Notes on `date_passed`

- Uses RFC 3339 date format: `2025-06-01` or `2025-06-01T15:30:00Z`
- If no timezone is specified, uses the computer's local time
- Triggers when the current time is past the specified date/time
- Date-only values (e.g., `2025-06-01`) trigger at start of day (00:00 local time)

### Version Comparison

Uses the `versions` crate which provides three-tier automatic version parsing:

1. **SemVer** — strict semver 2.0 (`1.2.3`, `1.2.3-beta.1`)
2. **Version** — structured numeric with any number of segments (`1.2.3.4`, `2025.01.15`)
3. **Mess** — chaotic formats with epochs, mixed alphanumeric (`2:10.2+0.0093r3+1-1`)

`Versioning::new()` tries each tier in order, gracefully degrading. The `Requirement` type provides constraint matching with operators: `>=`, `>`, `<`, `<=`, `=`, `^`, `~`, `*`.

#### Version Comparison Strategy

Given a constraint string (e.g., `>=2.0.0`) and a list of git tags:

1. **Parse the constraint** with `Requirement::from_str(constraint)`
2. **For each tag:**
   a. Strip known prefixes: `v`, `V`
   b. Parse with `Versioning::new(stripped_tag)`
   c. If parsing fails → skip, emit debug log (`skipping tag "nightly": not a valid version`)
   d. If pre-release and constraint doesn't explicitly target pre-release → skip
   e. Check `requirement.matches(&versioning)`
3. **Return** whether any tag matches

#### Constraint Operator Semantics

| Operator | SemVer | Non-SemVer |
|----------|--------|------------|
| `=`      | Exact match | Exact match |
| `>`      | Greater than | Greater than (segment-by-segment) |
| `>=`     | Greater or equal | Greater or equal |
| `<`      | Less than | Less than |
| `<=`     | Less or equal | Less or equal |
| `^`      | Compatible (same major) | **Warning** — meaningless without semver semantics, suggest `>=` |
| `~`      | Approximately (same minor) | **Warning** — meaningless without semver semantics, suggest `>=` |
| `*`      | Any | Any |

When `^` or `~` is used with a version that doesn't parse as strict semver, emit a warning suggesting the user use `>=` instead. Still attempt matching (the `versions` crate handles it), but warn about potentially surprising behavior.

#### Pre-release Handling

- For semver versions: follow the spec (`1.2.3-alpha < 1.2.3`)
- Constraints like `>=1.2.0` do NOT match `1.3.0-beta.1` unless the constraint explicitly targets pre-release (e.g., `>=1.2.0-0`)
- This matches npm/Cargo behavior and prevents surprises from unstable versions
- For non-semver: check for common pre-release indicators (`-alpha`, `-beta`, `-rc`, `-dev`)

#### Behavior Examples

```
Constraint: >=2.0.0
Tags: ["v1.9.0", "v2.0.0", "v2.1.3", "v3.0.0-beta.1"]
Result: true (matches v2.0.0 and v2.1.3; skips beta)

Constraint: ^2.0
Tags: ["v1.9.0", "v2.0.0", "v2.5.1", "v3.0.0"]
Result: true (matches v2.0.0, v2.5.1; not v3.0.0)

Constraint: >=2025.01
Tags: ["2024.12.01", "2025.01.15", "2025.03.01"]
Result: true (matches 2025.01.15 and 2025.03.01 — CalVer works naturally)

Constraint: >=1.2.3.4
Tags: ["1.2.3.3", "1.2.3.4", "1.2.3.5"]
Result: true (4-segment comparison works correctly)
```

### Multiple Operations (OR semantics)

```rust
// REMIND-ME-TO: Remove custom TLS config — either the fix PR or a release with the fix  pr_merged=github:hyper-rs/hyper#3210 tag_exists=github:hyper-rs/hyper@>=1.5.0
```

Default semantics: **ANY** operation triggers the reminder (OR). If any single condition is met, the reminder fires. AND logic may be added later with explicit syntax.

## CLI Interface

### Basic Usage

```bash
# Scan current directory recursively (default behavior, no path needed)
remind-me-to check

# Scan a single file
remind-me-to check src/main.rs

# Scan multiple directories
remind-me-to check ~/projects/foo ~/projects/bar

# Scan all projects
remind-me-to check ~/projects
```

When no path is provided, defaults to scanning the current directory recursively.

### Options

```
remind-me-to check [OPTIONS] [PATH...]

Options:
  --format <FORMAT>       Output format: text (default), json, llm
  --ignore <PATTERN>      Additional ignore patterns
  --no-gitignore          Don't respect .gitignore/.ignore files
  --dry-run               Find and parse comments without checking external services
  --verbose               Show all found reminders, including ones not yet triggered
  --quiet                 Suppress all output, only set exit code
  --log-level <LEVEL>     Log level: error, warn, info, debug, trace
```

### Logging

Uses the `tracing` crate with `tracing-subscriber`'s built-in Compact formatter. Produces single-line colored output to stderr, with span context inlined.

#### Configuration

- `RUST_LOG` env var takes priority over CLI flags
- `--log-level` maps to filter directives on the `remind_me_to` target
- Default level: `warn`
- `-v` / `--verbose`: also sets tracing to `info`
- `--quiet`: sets tracing filter to `error` only
- `FmtSpan::CLOSE` shows span durations on exit (visible at debug level) — useful for identifying slow API calls

#### Initialization

```rust
use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_tracing(verbosity: u8, quiet: bool) {
    let default_directive = if quiet {
        "error"
    } else {
        match verbosity {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        }
    };

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("remind_me_to={default_directive}")));

    let fmt_layer = fmt::layer()
        .compact()
        .with_writer(std::io::stderr)
        .with_target(verbosity >= 3)
        .with_ansi(true)
        .with_span_events(fmt::format::FmtSpan::CLOSE);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();
}
```

### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success, no reminders triggered |
| `1` | At least one reminder triggered (action needed) |
| `2` | Error (network failure, parse error, etc.) |

Individual operation failures (rate limits, timeouts) do NOT fail the whole run — they are logged as errors and that operation is reported as "unknown/error" in the output. The run continues checking other operations.

### Configuration

Uses the `config` crate with TOML format. Config file lives at `$XDG_CONFIG_HOME/remind-me-to/config.toml` (defaults to `~/.config/remind-me-to/config.toml`).

```toml
# Default format
format = "text"

# Forge tokens — read from config or environment variables
[tokens]
github = "$GITHUB_TOKEN"  # Can reference env vars
# gitea = "$GITEA_TOKEN"
# gitlab = "$GITLAB_TOKEN"

[scan]
ignore_patterns = ["vendor/", "node_modules/"]
```

Token resolution order:
1. Environment variable (e.g., `GITHUB_TOKEN`)
2. Config file value
3. Error with helpful message

CLI flags override config file values. No per-project config — ignore/filter behavior follows `.gitignore`/`.ignore` files on a per-directory basis.

### Ignore/Filter Behavior

Follows the same conventions as ripgrep and other modern CLI tools:
- Respects `.gitignore` files at each repo level
- Respects `.ignore` files (same format as `.gitignore`, same as ripgrep)
- Respects global gitignore (`core.excludesFile`)
- Skips binary files automatically
- Skips hidden files/directories by default

### Context Awareness

When scanning inside a git repository, the tool should:
- Detect all remotes to support shorthand references
- Allow shorthand like `pr_merged=#123` which resolves against the repo's remotes
- Remote resolution strategy:
  - If there's only one remote → use it (common for direct clones without forking)
  - If there's an `upstream` remote → prefer it for operations that check upstream state (most operations)
  - Fall back to `origin` if no `upstream` exists
  - If shorthand can't be resolved → emit an error with a helpful message suggesting the full `github:owner/repo#123` syntax
- Output always includes the file path so context is clear even when scanning across multiple repos

**Forge detection from remotes:** For MVP, only `github.com` is recognized automatically from remote URLs. No manual config for mapping custom domains — if a remote doesn't match a known forge domain, shorthand won't work and the user must use the full `github:owner/repo#123` syntax. Future providers will be added by recognizing their known domains. The architecture is designed so adding new forge providers is straightforward.

### Output Formats

#### Text (default)

Shows triggered reminders. When nothing triggers, prints a short success summary (e.g., "12 reminders found, 0 triggered").

Output order: file path order as encountered during the walk (non-deterministic with parallel walking, but fast).

```
src/tls.rs:42: Remove custom TLS config when both the fix lands and a release includes it
  ✓ pr_merged=github:hyper-rs/hyper#3210 (merged 2025-01-15)
  · tag_exists=github:hyper-rs/hyper@>=1.5.0 (latest: v1.4.1, not yet)

src/auth.rs:108: Drop session workaround after upstream fix
  ✓ pr_merged=github:auth-lib/core#89 (merged 2025-03-01)
  · tag_exists=github:auth-lib/core@>=4.0.0 (latest: v3.9.1, not yet)
```

With `--verbose`: also shows reminders where no conditions are met yet (pending reminders).

With `--quiet`: no output at all, only the exit code.

#### JSON

Machine-readable structured output for integration with other tools.

#### LLM Prompt

Generates output formatted for feeding to a coding agent/LLM, including:
- The file path and line number
- The human description of what needs to be done
- Which conditions have been met and the relevant context
- Enough information for the agent to take action without additional lookups

## File Scanning

### Performance Requirements

- Must handle scanning `~/projects` (thousands of repos, hundreds of thousands of files) in seconds
- Multi-threaded directory walking
- Skip binary files
- Respect `.gitignore` / `.ignore` at each repo level

### Implementation: `ignore` crate

Uses the `ignore` crate (v0.4.x, from BurntSushi/ripgrep) for parallel directory traversal with built-in ignore-file support.

**Why `ignore`:**
- Battle-tested by ripgrep, fd, delta, tokei
- Parallel walking via crossbeam work-stealing (configurable thread count)
- All ignore files respected by default: `.gitignore`, `.ignore`, `.git/info/exclude`, global gitignore
- Hidden files/dirs skipped by default
- Supports multiple root directories via `WalkBuilder::add()` with resource reuse across roots
- Single files passed as paths handled correctly (treated as leaf nodes)
- `max_filesize()` filter to skip large files without reading them

**Binary detection:** The `ignore` crate does not include binary detection (ripgrep handles this at a higher layer). We implement a simple heuristic: read the first 8KB of each file and check for NUL bytes (same approach as git and ripgrep).

```rust
fn is_binary(path: &Path) -> bool {
    let mut buf = [0u8; 8192];
    let Ok(mut file) = File::open(path) else { return false };
    let Ok(n) = file.read(&mut buf) else { return false };
    buf[..n].contains(&0)
}
```

**API shape:** The parallel walker uses a closure-based API (`WalkParallel::run`) rather than `Iterator` — necessary for parallelism. Results are collected via an `mpsc` channel.

```rust
use ignore::WalkBuilder;
use ignore::WalkState;
use std::sync::mpsc;

fn walk_paths(paths: &[&Path]) -> Vec<ignore::DirEntry> {
    let mut builder = WalkBuilder::new(paths[0]);
    for path in &paths[1..] {
        builder.add(path);
    }

    builder
        .hidden(true)          // skip hidden files (default)
        .ignore(true)          // respect .ignore files (default)
        .git_ignore(true)      // respect .gitignore (default)
        .git_global(true)      // respect global gitignore (default)
        .git_exclude(true)     // respect .git/info/exclude (default)
        .max_filesize(Some(1_048_576)) // skip files > 1MB
        .threads(0);           // auto-detect thread count

    let (tx, rx) = mpsc::channel();

    builder.build_parallel().run(|| {
        let tx = tx.clone();
        Box::new(move |result| {
            match result {
                Ok(entry) => {
                    if entry.file_type().map_or(false, |ft| ft.is_file()) {
                        if !is_binary(entry.path()) {
                            let _ = tx.send(entry);
                        }
                    }
                    WalkState::Continue
                }
                Err(err) => {
                    tracing::warn!("walk error: {}", err);
                    WalkState::Continue
                }
            }
        })
    });

    drop(tx);
    rx.into_iter().collect()
}
```

### Pipeline Architecture

The scanning phase is decoupled from the checking phase:

1. **Scan** — walk files, find markers, parse operations → `ScanResult { reminders, errors }`
2. **Check** — batch API calls to resolve operations → list of triggered/pending reminders
3. **Report** — format and display results

### API Efficiency

- **Deduplicate API calls** — if the same operation (e.g., `pr_merged=github:foo/bar#123`) appears in multiple files, make one API call and apply the result to all locations
- Batch API calls where possible (e.g., multiple PRs in the same repo)
- No aggressive caching — users expect fresh data when they run the tool
- Respect `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY` environment variables

### Rate Limiting (GitHub)

Read GitHub's rate limit headers from every response:
- `X-RateLimit-Remaining` — requests left in current window
- `X-RateLimit-Reset` — Unix timestamp when the window resets
- `Retry-After` — seconds to wait (on 429 responses)

**Strategy:**
- On 429 response: read `Retry-After` header, wait that many seconds, retry once. If the retry also fails, report the operation as "error" and continue.
- If `X-RateLimit-Remaining` reaches 0 mid-run: stop making further API calls to that forge. Report remaining unchecked operations as "unknown (rate limited)" and log the reset time.
- **Unauthenticated:** If no token is available, stop after consuming half the unauthenticated limit (30 of 60 requests for GitHub) to leave headroom for other tools.
- No persistent rate limit state — each run starts fresh and reads headers from live responses.

## HTTP Client

### Decision: `ureq` 3.x (sync)

Uses `ureq` 3.x as the HTTP client with `std::thread::scope` for bounded parallelism during the checking phase.

**Why sync over async:**
- For 10-50 API calls with rate limiting, async provides no meaningful speed advantage over sync + thread pool. The bottleneck is network latency and rate limits, not thread overhead.
- No async infection of the library API — the `ForgeClient` trait is a simple sync trait, trivial to mock in tests.
- 3-5x faster compile times than reqwest (~30-50 transitive deps vs ~200-300).
- The library is usable from any context — sync CLI, async server, WASM (future).

**Features used:**
- `proxy-from-env` (default in 3.x) — `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` work automatically
- `json` feature — built-in serde JSON request/response bodies
- Connection pooling via `Agent` (keep-alive reuse across calls to the same host)
- rustls by default (no OpenSSL dependency)

**Parallelism strategy:** `std::thread::scope` with a small fixed number of worker threads (e.g., 8). Operations are grouped by host/repo, chunks assigned to threads. This naturally limits concurrent connections per host and makes rate limiting straightforward.

```rust
pub fn check_all(
    operations: &[Operation],
    client: &dyn ForgeClient,
    max_concurrent: usize,
) -> Vec<CheckResult> {
    std::thread::scope(|s| {
        let chunks: Vec<_> = operations.chunks(
            (operations.len() / max_concurrent).max(1)
        ).collect();

        let handles: Vec<_> = chunks.into_iter().map(|chunk| {
            s.spawn(|| {
                chunk.iter().map(|op| check_one(op, client)).collect::<Vec<_>>()
            })
        }).collect();

        handles.into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    })
}
```

### ForgeClient Trait

The HTTP client is injectable via a trait for testing:

```rust
pub trait ForgeClient: Send + Sync {
    fn get_pr_status(&self, owner: &str, repo: &str, number: u64) -> Result<PrStatus, CheckError>;
    fn get_tags(&self, owner: &str, repo: &str) -> Result<Vec<Tag>, CheckError>;
    fn get_issue_status(&self, owner: &str, repo: &str, number: u64) -> Result<IssueStatus, CheckError>;
    fn branch_exists(&self, owner: &str, repo: &str, branch: &str) -> Result<bool, CheckError>;
    fn get_commit_releases(&self, owner: &str, repo: &str, sha: &str) -> Result<Vec<Release>, CheckError>;
}
```

### GitHub Implementation

```rust
pub struct GitHubClient {
    agent: ureq::Agent,
    token: Option<String>,
}

impl GitHubClient {
    pub fn new(token: Option<String>) -> Self {
        let agent = ureq::Agent::new_with_defaults(); // picks up proxy from env
        Self { agent, token }
    }

    fn request(&self, url: &str) -> Result<ureq::Response, CheckError> {
        let mut req = self.agent.get(url);
        req = req.header("Accept", "application/vnd.github+json");
        req = req.header("User-Agent", "remind-me-to");
        if let Some(ref token) = self.token {
            req = req.header("Authorization", &format!("Bearer {token}"));
        }
        let response = req.call().map_err(|e| CheckError::Network {
            message: e.to_string(),
            source: Box::new(e),
        })?;
        Ok(response)
    }
}
```

### Cargo.toml

```toml
[dependencies]
ureq = { version = "3", features = ["json"] }
```

## Architecture

### Workspace Structure

Cargo workspace with two crates — a library that does the heavy lifting and a thin CLI binary:

```
remind-me-to/
├── Cargo.toml              # Workspace root (workspace = true)
├── crates/
│   ├── remind-me-to-lib/   # Library crate — all core logic
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── scanner/    # File walking and marker detection
│   │       │   ├── mod.rs
│   │       │   ├── walker.rs   # Parallel directory traversal (ignore crate)
│   │       │   └── parser.rs   # Marker parsing, operation extraction (chumsky)
│   │       ├── ops/        # Operation definitions and checking
│   │       │   ├── mod.rs
│   │       │   ├── types.rs    # Operation enum, ForgeClient trait
│   │       │   ├── github.rs   # GitHub provider (ureq)
│   │       │   └── version.rs  # Version comparison logic (versions crate)
│   │       ├── errors.rs       # Error types (thiserror)
│   │       └── output/     # Formatting and display
│   │           ├── mod.rs
│   │           ├── text.rs
│   │           ├── json.rs
│   │           └── llm.rs
│   └── remind-me-to-cli/   # Binary crate — thin CLI wrapper
│       ├── Cargo.toml
│       └── src/
│           └── main.rs     # Arg parsing (clap derive), config loading, error display (miette), calls into lib
```

### Library API Boundary

The library is designed for reusability and testability:

- **Returns structured data** — `ScanResult { reminders, errors }` with status, location, description, and per-operation results
- **I/O is internal but injectable** — the lib handles file walking and HTTP internally, but the HTTP client is injectable via the `ForgeClient` trait for testing
- **Formatting lives in the lib** — output formatters are part of the library so other consumers (GitHub Action, etc.) can reuse them
- **The CLI is a thin shell** — parses args, loads config, sets up tracing, calls lib functions, renders errors with miette, exits with appropriate code
- **Errors are structured** — `thiserror` enums that programmatic consumers can match on; the CLI renders them with `miette`

The goal is: all business logic is in the lib, highly testable, and reusable by other consumers.

## Error Handling

### Strategy: `thiserror` (lib) + `miette` (CLI)

The library defines structured error types with `thiserror`. The CLI crate renders them with `miette` for pretty diagnostic output. For parser errors specifically, `ariadne` may be used if converting chumsky's span types to miette proves too painful (ariadne is chumsky's sister project by the same author, with zero-friction span conversion).

### Library Error Types

```rust
use std::path::PathBuf;

/// Result of scanning files — contains both successes and errors.
/// A parse error in one file does not stop the scan.
pub struct ScanResult {
    pub reminders: Vec<Reminder>,
    pub errors: Vec<ScanError>,
}

/// Non-fatal errors during file scanning/parsing
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("parse error in {file}:{line}: {message}")]
    Parse {
        file: PathBuf,
        line: usize,
        col: usize,
        message: String,
        span: std::ops::Range<usize>,
        source_line: String,
        expected: Vec<String>,
        found: Option<String>,
    },

    #[error("failed to read {path}: {source}")]
    FileRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to walk directory {path}: {source}")]
    Walk {
        path: PathBuf,
        #[source]
        source: ignore::Error,
    },
}

/// Non-fatal errors during operation checking (per-operation, doesn't stop run)
#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error("API request failed for {operation}: {message}")]
    ApiError {
        operation: String,
        message: String,
        status: Option<u16>,
        retryable: bool,
    },

    #[error("rate limited by {forge}, resets at {reset_at}")]
    RateLimited {
        forge: String,
        reset_at: String,
    },

    #[error("authentication required for {forge}")]
    AuthRequired {
        forge: String,
    },

    #[error("network error: {message}")]
    Network {
        message: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

/// Fatal errors that prevent the tool from running at all
#[derive(Debug, thiserror::Error)]
pub enum FatalError {
    #[error("configuration error: {message}")]
    Config { message: String },

    #[error("no files to scan")]
    NoInput,
}
```

### CLI Error Rendering

The CLI uses `miette` to render errors with source spans, help text, and diagnostic codes:

```rust
use miette::{Diagnostic, Result as MietteResult};

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum CliError {
    #[error("configuration error: {0}")]
    #[diagnostic(help("Check your config at ~/.config/remind-me-to/config.toml"))]
    Config(String),

    #[error("{0}")]
    Fatal(#[from] remind_me_to_lib::FatalError),
}
```

For parse errors, the CLI renders source-annotated diagnostics showing the exact location and what was expected. If miette's source-span conversion from chumsky is straightforward, use miette for everything. If not, use `ariadne` for parse errors specifically (it's designed to consume chumsky spans directly).

### Design Principles

- **Never panic, never stop on non-fatal errors** — collect all errors, report at the end
- **Library is rendering-agnostic** — error types carry all information needed for rendering, but the lib doesn't render
- **Chumsky error recovery** — the parser uses chumsky's error recovery to produce multiple parse errors per file alongside partial results
- **Structured for programmatic consumers** — `thiserror` enums can be matched on by library users

## Testing Strategy

### Unit Tests

- **Parser logic** — chumsky parsers are very testable; test each operation value parser, marker detection, error recovery
- **Version comparison** — test `versions` crate behavior for semver, CalVer, 4-segment, pre-release, unparseable tags
- **Operation type handling** — test `ForgeClient` trait implementations with mock clients
- **Batching/deduplication** — verify that duplicate operations result in a single API call

### CLI Snapshot Tests: `assert_cmd` + `insta`

Uses `assert_cmd` + `insta` (via `insta-cmd`) as the primary CLI testing approach:

**Why this combination:**
- Full programmatic control for test setup (temp directories with git repos, configured remotes, env vars, mock servers)
- `cargo insta review` for interactive snapshot diff review
- Powerful non-deterministic output handling via `add_filter()` and line sorting
- Exit code, stdout, stderr all captured in a single snapshot via `insta-cmd`
- `predicates` crate for quick targeted assertions (exit codes, error substrings)

**Handling non-deterministic file order:** Sort output blocks before snapshotting. Use `add_filter()` to redact temp directory paths, timestamps, etc.

```rust
use assert_cmd::Command;
use insta_cmd::assert_cmd_snapshot;
use insta::Settings;
use tempfile::TempDir;

#[test]
fn test_dry_run_text_output() {
    let fixture = setup_test_fixture();
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
                .arg(fixture.path())
        );
    });
}

#[test]
fn test_parse_error_exits_with_code_2() {
    Command::cargo_bin("remind-me-to").unwrap()
        .arg("check")
        .arg("--dry-run")
        .arg(dir.path())
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains("error"));
}
```

**Dev tools needed:** `cargo install cargo-insta` for the snapshot review workflow.

### HTTP Mocking: Two-Layer Strategy

#### Layer 1: Trait-based mocking (unit tests)

The `ForgeClient` trait enables pure unit testing with no HTTP, no extra crates:

```rust
#[cfg(test)]
pub struct MockForgeClient {
    pub pr_responses: HashMap<(String, String, u64), Result<PrStatus, CheckError>>,
    pub call_count: std::cell::RefCell<HashMap<String, usize>>,
}

#[cfg(test)]
impl ForgeClient for MockForgeClient {
    fn get_pr_status(&self, owner: &str, repo: &str, number: u64) -> Result<PrStatus, CheckError> {
        *self.call_count.borrow_mut().entry("get_pr_status".into()).or_default() += 1;
        self.pr_responses
            .get(&(owner.into(), repo.into(), number))
            .cloned()
            .unwrap_or(Err(CheckError::ApiError {
                operation: format!("pr_merged=github:{owner}/{repo}#{number}"),
                message: "not found".into(),
                status: Some(404),
                retryable: false,
            }))
    }
    // ...
}
```

**Covers:** Business logic, response parsing, error handling, batching/deduplication, rate limit decisions. ~80% of test surface.

#### Layer 2: `httpmock` (integration tests)

Uses `httpmock` for a small set of integration tests (~10-20) that verify actual HTTP mechanics:

**Why `httpmock`:**
- Sync API for test setup (no `#[tokio::test]` needed — matches our ureq choice)
- Response sequencing built-in (critical for rate limit back-off testing)
- Built-in record/playback (eliminates need for separate VCR crate)
- Proxy mode for testing `HTTP_PROXY` support
- Random port per server instance — fully parallel-safe

```rust
use httpmock::prelude::*;
use serde_json::json;

#[test]
fn test_github_pr_request_construction() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/tokio-rs/tokio/pulls/5432")
            .header("Accept", "application/vnd.github+json")
            .header("Authorization", "Bearer test-token");
        then.status(200)
            .json_body(json!({
                "number": 5432,
                "state": "closed",
                "merged": true,
                "merged_at": "2025-01-15T00:00:00Z",
                "merge_commit_sha": "abc123def456"
            }));
    });

    let client = GitHubClient::new_with_base_url(&server.base_url(), "test-token");
    let pr = client.get_pr_status("tokio-rs", "tokio", 5432).unwrap();
    assert!(pr.merged);
    mock.assert();
}

#[test]
fn test_rate_limit_backoff() {
    let server = MockServer::start();

    // First: rate limited. Second: success.
    server.mock(|when, then| {
        when.method(GET).path("/repos/owner/repo/pulls/1");
        then.status(429)
            .header("Retry-After", "1")
            .header("X-RateLimit-Remaining", "0");
    });
    server.mock(|when, then| {
        when.method(GET).path("/repos/owner/repo/pulls/1");
        then.status(200)
            .json_body(json!({"number": 1, "state": "open", "merged": false}));
    });

    let client = GitHubClient::new_with_base_url(&server.base_url(), "token");
    let pr = client.get_pr_status_with_retry("owner", "repo", 1).unwrap();
    assert_eq!(pr.state, PrState::Open);
}
```

**Covers:** URL/header construction, JSON deserialization from HTTP responses, HTTP status code handling, rate limit header parsing, proxy env var support.

### Integration Tests

Full scan-check-report pipeline tests using temp directories with REMIND-ME-TO comments, mock HTTP servers for the checking phase, and snapshot testing for output verification.

## Dependencies

All dependencies are decided. No further research needed.

### Library Crate (`remind-me-to-lib`)

| Purpose | Crate | Version | Notes |
|---------|-------|---------|-------|
| CLI comment parsing | `chumsky` | 1.x | Parser combinators with error recovery |
| File walking | `ignore` | 0.4.x | Parallel traversal, .gitignore/.ignore support |
| HTTP client | `ureq` | 3.x | Sync, features: `json`, `proxy-from-env` (default) |
| Version comparison | `versions` | 6.x | Three-tier parsing: SemVer → Version → Mess |
| Error types | `thiserror` | 2.x | Derive macro for structured error enums |
| JSON | `serde` / `serde_json` | 1.x | Serialization/deserialization |
| Logging | `tracing` | 0.1.x | Structured logging with spans |

### CLI Crate (`remind-me-to-cli`)

| Purpose | Crate | Version | Notes |
|---------|-------|---------|-------|
| Arg parsing | `clap` | 4.x | Derive-based CLI definition |
| Config file | `config` | 0.14.x | TOML config with env var expansion |
| Error display | `miette` | 7.x | Pretty diagnostic output with source spans |
| Tracing setup | `tracing-subscriber` | 0.3.x | Features: `env-filter`; Compact formatter |
| Parse error display | `ariadne` | 0.4.x | Only if miette conversion from chumsky is painful |

### Dev Dependencies (both crates)

| Purpose | Crate | Version | Notes |
|---------|-------|---------|-------|
| CLI testing | `assert_cmd` | 2.x | Run binary, capture output |
| Snapshot testing | `insta` | 1.x | Features: `filters`; snapshot diffs |
| CLI snapshots | `insta-cmd` | 0.6.x | Bridge between assert_cmd and insta |
| Assertions | `predicates` | 3.x | Composable assertions for exit codes, substrings |
| Temp files | `tempfile` | 3.x | Temporary directories for test fixtures |
| HTTP mocking | `httpmock` | 0.8.x | Mock server for integration tests |
| Serialization test | `serde_json` | 1.x | For JSON output testing |

### Dev Tools

| Tool | Purpose |
|------|---------|
| `cargo-insta` | Interactive snapshot review (`cargo insta review`) |
| `cargo-watch` | Development iteration (`cargo watch -x test`) |

## Non-Goals

- Auto-fix (removing the comment/code) — too dangerous, never
- Watch mode — this is a point-in-time check tool, never
- IDE integration (LSP) — maybe later, not MVP
- Manual forge domain config — not needed for MVP, just recognize known domains

## Future Possibilities (not MVP)

- **GitHub Actions action** — a packaged action that runs `remind-me-to check` on a cron schedule in CI, using the `github-actions` output format to surface triggered reminders as workflow annotations, and optionally opening issues or PR comments when action is needed
- Additional forge providers (GitLab, Gitea, Forgejo, Codeberg, sourcehut)
- `remind-me-to init` to add a reminder interactively
- LSP/IDE integration for inline diagnostics
- Multi-line comment continuation
- AND logic for multiple operations (explicit syntax TBD)
- Custom domain → forge mapping in config (for self-hosted instances)
- Git-only fallback operations (for forges we don't have API support for yet)

## DevShell Changes Needed

The `devenv.nix` needs to be updated to include development tooling:
- Rust toolchain (already present)
- `cargo-watch` for development iteration
- `cargo-insta` for snapshot review workflow
