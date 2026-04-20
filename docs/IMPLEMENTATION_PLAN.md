# Implementation Plan

This document breaks the `remind-me-to` project into incremental phases. Each phase produces a testable, working slice of the system. Phases are ordered by dependency — later phases build on earlier ones.

---

## Phase 0: Project Scaffold & Workspace Setup

**Goal:** Transform the single-crate scaffold into the workspace layout described in the requirements.

### Tasks

1. Convert `Cargo.toml` to a workspace root with `[workspace]` members
2. Create `crates/remind-me-to-lib/` with `Cargo.toml` and `src/lib.rs`
3. Create `crates/remind-me-to-cli/` with `Cargo.toml` and `src/main.rs`
4. Set up module stubs: `scanner/`, `ops/`, `errors.rs`, `output/`
5. Add core dependencies to each crate's `Cargo.toml` (no feature code yet)
6. Update `devenv.nix` to include `cargo-watch` and `cargo-insta`
7. Verify `cargo build` and `cargo nextest run` pass on the empty workspace

### Testing

- `cargo build --workspace` succeeds
- `cargo nextest run --workspace` succeeds (no tests yet, but no errors)
- `cargo run -p remind-me-to-cli` prints a placeholder message

---

## Phase 1: Comment Parser (`chumsky`)

**Goal:** Parse `REMIND-ME-TO:` markers and extract operations from source lines.

### Tasks

1. Implement marker detection (case-insensitive search for `REMIND-ME-TO:`)
2. Implement whitespace tokenizer after the marker
3. Implement operation value parsers:
   - Forge reference: `github:owner/repo#123` (issue-like)
   - Forge reference: `github:owner/repo@constraint` (ref-like)
   - Date: `2025-06-01` or RFC 3339 with time
4. Implement the classification logic:
   - Known operation name + valid value → operation
   - Known operation name + invalid value → parse error
   - Unknown token → part of description
5. Collect human-readable description from non-operation tokens
6. Implement chumsky error recovery for partial results
7. Define the `Reminder` struct (file, line, description, operations)

### Testing

- **Unit tests for each operation parser** — valid inputs, malformed inputs, edge cases
- **Full-line parsing tests** — multiple operations, mixed prose and operations
- **Error recovery tests** — malformed value still produces the valid operations on the same line
- **Case-insensitivity tests** — `remind-me-to:`, `REMIND-ME-TO:`, `Remind-Me-To:`
- **Comment closer handling** — `-->`, `*/` at end don't cause errors (absorbed as description)
- **Snapshot tests** (`insta`) for parser error messages

### Key edge cases

- `x=5` in prose (not a known operation → ignored)
- `pr_merged=bad_value` (known operation, bad value → error)
- `tag_exists=github:owner/repo@>=2.0.0-->` (no space before closer → error)
- Operations with all supported constraint operators (`>=`, `>`, `<`, `<=`, `=`, `^`, `~`, `*`)

---

## Phase 2: File Walking (`ignore` crate)

**Goal:** Recursively scan directories for files, respecting `.gitignore`/`.ignore`, skipping binaries.

### Tasks

1. Implement `walk_paths()` using `ignore::WalkBuilder` with parallel walking
2. Implement binary detection (NUL byte in first 8KB)
3. Wire up `max_filesize` (1MB limit)
4. Support multiple root paths via `WalkBuilder::add()`
5. Handle single-file paths (leaf nodes)
6. Collect results via `mpsc` channel
7. Integrate with parser: walk → read file → find markers → parse → `ScanResult`

### Testing

- **Unit test for binary detection** — binary file skipped, text file included
- **Integration test with temp directories** — create a dir tree with `.gitignore`, hidden files, binary files, verify correct file set is scanned
- **Test multiple roots** — two directories, both scanned
- **Test single file path** — single file is parsed directly
- **Test `.ignore` file support** — patterns in `.ignore` are respected
- **Test large file skip** — file over 1MB is skipped

---

## Phase 3: Error Types & Tracing

**Goal:** Set up structured error handling and logging infrastructure.

### Tasks

1. Implement `ScanError`, `CheckError`, `FatalError` enums with `thiserror`
2. Implement `ScanResult { reminders, errors }` collection pattern
3. Set up `tracing` + `tracing-subscriber` with the Compact formatter
4. Implement `init_tracing(verbosity, quiet)` function
5. Add `tracing` instrumentation to scanner and walker
6. Wire up `miette` in the CLI crate for error rendering

### Testing

- **Unit tests for error type construction** — each variant creates correctly
- **Snapshot tests for miette-rendered errors** — verify pretty output format
- **Test tracing initialization** — verify filter behavior with different verbosity levels
- **Test that parse errors don't halt the scan** — one bad file doesn't stop others

---

## Phase 4: CLI Skeleton (`clap` + `config`)

**Goal:** Working CLI that can scan files in `--dry-run` mode (no network calls).

### Tasks

1. Define CLI args with `clap` derive: `check` subcommand, all options
2. Implement config file loading with `config` crate (TOML)
3. Implement token resolution (env var → config file → error)
4. Wire up: CLI args → config merge → call library scan → format output
5. Implement exit codes (0, 1, 2)
6. Implement `--dry-run` mode (scan + parse only, no checking)

### Testing

- **`assert_cmd` + `insta` snapshot tests:**
  - `--dry-run` with valid reminders → shows parsed reminders
  - `--dry-run` with parse errors → exit code 2, shows errors
  - `--help` output snapshot
  - No files to scan → appropriate error
- **Config file tests** — token from config, token from env, missing token
- **Exit code tests** — 0 for no triggers, 2 for errors

### Milestone: `remind-me-to check --dry-run .` works end-to-end

---

## Phase 5: GitHub Client (`ureq`)

**Goal:** Implement the `ForgeClient` trait and GitHub-specific HTTP client.

### Tasks

1. Define `ForgeClient` trait with all methods
2. Implement `GitHubClient` struct with `ureq::Agent`
3. Implement each API method:
   - `get_pr_status` → `GET /repos/{owner}/{repo}/pulls/{number}`
   - `get_tags` → `GET /repos/{owner}/{repo}/tags` (paginated)
   - `get_issue_status` → `GET /repos/{owner}/{repo}/issues/{number}`
   - `branch_exists` → `GET /repos/{owner}/{repo}/branches/{branch}` (404 = doesn't exist)
   - `get_commit_releases` → check if commit is ancestor of release tags
4. Implement rate limit header parsing and back-off logic
5. Implement `new_with_base_url()` constructor for testing
6. Implement connection pooling via `Agent` reuse

### Testing

- **Trait mock tests** — `MockForgeClient` for all business logic
- **`httpmock` integration tests:**
  - Request URL/header construction
  - JSON response deserialization
  - 404 handling (PR not found, branch doesn't exist)
  - 429 rate limit → retry with `Retry-After`
  - `X-RateLimit-Remaining: 0` → stop further calls
  - Auth header present when token is set, absent when not
  - Pagination for tags endpoint

---

## Phase 6: Version Comparison (`versions` crate)

**Goal:** Compare git tags against version constraints.

### Tasks

1. Implement tag stripping (remove `v`/`V` prefix)
2. Implement version parsing with `Versioning::new()`
3. Implement constraint parsing with `Requirement::from_str()`
4. Implement pre-release filtering logic
5. Implement `^`/`~` warning for non-semver versions
6. Wire into `tag_exists` operation checking

### Testing

- **Unit tests for all constraint operators** — `>=`, `>`, `<`, `<=`, `=`, `^`, `~`, `*`
- **SemVer tests** — standard semver comparison
- **CalVer tests** — `2025.01.15` style versions
- **4-segment tests** — `1.2.3.4` comparison
- **Pre-release filtering** — `>=1.2.0` does NOT match `1.3.0-beta.1`
- **Pre-release inclusion** — `>=1.2.0-0` DOES match pre-release
- **Unparseable tag skip** — `nightly`, `latest` silently skipped
- **`^`/`~` with non-semver** — warning emitted
- **All behavior examples from requirements** — exact scenarios from the spec

---

## Phase 7: Operation Checking & Orchestration

**Goal:** Wire together the checking phase — deduplicate, batch, run in parallel, collect results.

### Tasks

1. Implement operation deduplication (same operation across files → one API call)
2. Implement `check_all()` with `std::thread::scope` for bounded parallelism
3. Implement each operation checker:
   - `pr_merged` — check PR state == merged
   - `pr_closed` — check PR state == closed
   - `tag_exists` — get tags, run version comparison
   - `commit_released` — check if commit is ancestor of any release tag
   - `pr_released` — get PR merge_commit_sha, then check if released
   - `issue_closed` — check issue state == closed
   - `branch_deleted` — check branch does NOT exist
   - `date_passed` — compare current time to specified date
4. Implement OR semantics (any operation triggers → reminder fires)
5. Handle individual operation failures gracefully (error state, continue)

### Testing

- **Deduplication test** — same operation in 3 files → mock called once
- **OR semantics test** — one of two operations satisfied → reminder triggers
- **All-pending test** — no operations satisfied → reminder doesn't trigger
- **Error isolation test** — one operation errors, others still checked
- **`date_passed` tests** — past date triggers, future date doesn't
- **Thread safety tests** — verify `ForgeClient: Send + Sync` works correctly
- **Bounded parallelism test** — verify max concurrent threads respected

---

## Phase 8: Output Formatters

**Goal:** Implement all three output formats.

### Tasks

1. Implement text formatter (default):
   - Triggered reminders with operation status indicators (✓ / ·)
   - Summary line when nothing triggers
   - `--verbose` mode shows all reminders
2. Implement JSON formatter:
   - Structured output with all fields
   - Stable schema for machine consumption
3. Implement LLM prompt formatter:
   - File path, line number, description
   - Which conditions met, relevant context
   - Actionable without additional lookups

### Testing

- **Snapshot tests for each format** — golden output files
- **Text format edge cases** — no triggers, all triggers, mixed
- **JSON schema validation** — output is valid JSON, contains expected fields
- **`--verbose` vs default** — verbose shows pending, default hides them
- **`--quiet`** — no output at all

---

## Phase 9: Context Awareness (Git Remote Detection)

**Goal:** Support shorthand references like `pr_merged=#123` by detecting git remotes.

### Tasks

1. Detect if scanning inside a git repository
2. Parse remote URLs to identify forge and owner/repo
3. Implement remote resolution strategy:
   - Single remote → use it
   - `upstream` remote → prefer it
   - Fall back to `origin`
4. Resolve shorthand `#123` → full `github:owner/repo#123`
5. Emit helpful errors when shorthand can't be resolved

### Testing

- **Temp git repo tests** — create repo with remotes, verify resolution
- **Single remote** — any name works
- **upstream preferred** — `upstream` chosen over `origin`
- **No remote match** — error with helpful message
- **Non-github remote** — shorthand fails gracefully
- **SSH and HTTPS URL parsing** — both formats work

---

## Phase 10: Polish & Integration

**Goal:** End-to-end integration testing, documentation, and release readiness.

### Tasks

1. Full pipeline integration tests (scan → check → report) with `httpmock`
2. CLI snapshot tests for complete workflows
3. Error message review and polish
4. Performance testing with large directory trees
5. Verify proxy support (`HTTP_PROXY` / `HTTPS_PROXY`)
6. Final `devenv.nix` updates

### Testing

- **End-to-end happy path** — real-looking fixture dir, mock API, verify output
- **End-to-end error paths** — network errors, auth errors, parse errors
- **Performance benchmark** — scan a large fixture dir, verify reasonable time
- **Proxy integration test** — `httpmock` in proxy mode

---

## Dependency Graph

```
Phase 0 (scaffold)
    │
    ├── Phase 1 (parser)
    │       │
    │       └── Phase 2 (file walking)
    │               │
    │               └── Phase 4 (CLI skeleton + dry-run)
    │                       │
    │                       ├── Phase 7 (orchestration)
    │                       │       │
    │                       │       └── Phase 8 (output)
    │                       │               │
    │                       │               └── Phase 10 (polish)
    │                       │
    │                       └── Phase 9 (git context)
    │
    ├── Phase 3 (errors + tracing) ← needed by all phases after 0
    │
    ├── Phase 5 (GitHub client)
    │       │
    │       └── Phase 7 (orchestration)
    │
    └── Phase 6 (version comparison)
            │
            └── Phase 7 (orchestration)
```

Phases 1, 3, 5, and 6 can be worked on in parallel after Phase 0 is complete. Phase 7 requires 1, 2, 5, and 6. Phase 4 requires 1, 2, and 3. Phase 10 requires everything.

---

## Testing Strategy Summary

| Layer | Tool | What it covers |
|-------|------|----------------|
| Unit tests | `#[test]` + `insta` | Parser, version comparison, operation logic, error types |
| Trait mocks | `MockForgeClient` | Business logic without HTTP |
| HTTP integration | `httpmock` | Request construction, response parsing, rate limits |
| CLI snapshots | `assert_cmd` + `insta-cmd` | Full binary behavior, exit codes, output formats |
| End-to-end | All of the above | Complete pipeline with fixtures and mock servers |

### Test runner: `cargo nextest`

> **IMPORTANT:** Always use `cargo nextest run` to run tests. Never use `cargo test` directly. This applies everywhere — local development, CI, pre-commit hooks, and any test commands in documentation.

All tests are run via [`cargo-nextest`](https://nexte.st/) instead of `cargo test`. Nextest provides parallel test execution, better output, and per-test timeout control. It integrates with `insta` snapshots via `cargo insta test --test-runner nextest`.

```bash
# Run all tests
cargo nextest run --workspace

# Run with snapshot review
cargo insta test --test-runner nextest --workspace
cargo insta review

# Run specific phase's tests
cargo nextest run -p remind-me-to-lib -E 'test(parser)'
cargo nextest run -p remind-me-to-lib -E 'test(version)'
cargo nextest run -p remind-me-to-cli

# Run a single test
cargo nextest run -p remind-me-to-lib -E 'test(=test_name_here)'
```

---

## Estimated Order of Implementation

1. **Phase 0** — scaffold (prerequisite for everything)
2. **Phase 3** — errors + tracing (small, unblocks clean development)
3. **Phase 1** — parser (core logic, most complex, start early)
4. **Phase 2** — file walking (straightforward with `ignore` crate)
5. **Phase 6** — version comparison (independent, well-defined)
6. **Phase 5** — GitHub client (independent, well-defined)
7. **Phase 4** — CLI skeleton + dry-run (first user-visible milestone)
8. **Phase 7** — orchestration (brings it all together)
9. **Phase 8** — output formatters (needs orchestration results)
10. **Phase 9** — git context (nice-to-have, independent of output)
11. **Phase 10** — polish and integration testing
