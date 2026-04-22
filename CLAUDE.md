# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Maintaining This File

Update this CLAUDE.md as you implement features. When architecture decisions change, new crates are added, or conventions evolve, reflect those changes here so future sessions stay accurate.

## Version Control: Jujutsu (jj)

**NEVER use git commands.** This project uses [Jujutsu (jj)](https://martinvonz.github.io/jj/) for all version control.

**Make commits frequently** — small, logical chunks as you work. Don't batch up large changes. Each commit should represent one coherent step (e.g., "add parser for forge references", "wire up file walker to scanner").

Common operations:
```bash
jj log                    # view history
jj diff                   # see current changes
jj describe -m "message"  # set commit message on working copy
jj new                    # start a new change
jj squash                 # fold working copy into parent
jj status                 # see file status
```

## Build & Test Commands

```bash
# Development environment (requires devenv/nix)
devenv shell

# Build
cargo build --workspace
cargo clippy --workspace   # lint

# Tests — ALWAYS use nextest, never cargo test
cargo nextest run --workspace
cargo nextest run -p <crate> -E 'test(<filter>)'    # run subset
cargo nextest run -p <crate> -E 'test(=exact_name)' # single test

# Snapshot testing
cargo insta test --test-runner nextest --workspace
cargo insta review

# Run the CLI
cargo run -p remind-me-to-cli -- check --dry-run .
cargo run -p remind-me-to-cli -- check --help
```

## Project Overview

`remind-me-to` is a CLI tool that scans source files for `REMIND-ME-TO:` comments containing machine-checkable conditions (e.g., "remove this workaround when upstream PR merges"). It queries external APIs (GitHub) to check if conditions are met, then reports which reminders need attention.

## Architecture (Implemented Workspace Layout)

```
crates/
  lib/                 # Core library (pure business logic, no config)
    src/
      lib.rs           # Re-exports modules
      errors.rs        # ScanError, CheckError, FatalError (thiserror)
      scanner/
        mod.rs         # scan() top-level function
        parser.rs      # REMIND-ME-TO marker detection + operation parsing
        walker.rs      # Parallel file walking (ignore crate)
        git_context.rs # Git remote detection, shorthand resolution
      ops/
        mod.rs
        types.rs       # Operation enum, ForgeClient trait, Reminder, CheckResult types
        github.rs      # GitHubClient (ureq) + MockForgeClient for testing
        version.rs     # Version constraint checking (versions crate)
        checker.rs     # check_all() orchestration, deduplication, parallel checking
      output/
        mod.rs
        text.rs        # Human-readable text formatter
        json.rs        # JSON output formatter
        llm.rs         # LLM-friendly prompt formatter
  cli/                 # Binary crate (thin CLI wrapper)
    src/
      main.rs          # clap args, tracing init, config, calls lib functions
```

## Key Design Decisions

- **Parser:** Line scanner + chumsky parser combinators for post-marker content
- **File walking:** `ignore` crate (respects .gitignore, parallel walking)
- **HTTP:** `ureq` 3.x (blocking, sync) with `http` crate types
- **Version comparison:** `versions` crate with manual fallback for CalVer/4-segment
- **Error handling:** `thiserror` for structured error types
- **CLI:** `clap` derive with subcommands
- **Logging:** `tracing` + `tracing-subscriber` with Compact formatter
- **Testing:** `cargo-insta` snapshots, `tempfile` for test fixtures, `MockForgeClient` for unit tests
- **Lib crate is config-free:** All config loading lives in the CLI crate; the lib accepts pre-resolved values

## 8 MVP Operations

| Operation | Triggers when |
|-----------|---------------|
| `pr_merged` | PR is merged |
| `pr_closed` | PR is closed (merged or not) |
| `tag_exists` | Version constraint satisfied by a tag |
| `commit_released` | Commit SHA in a release |
| `pr_released` | PR's merge commit in a release |
| `issue_closed` | Issue is closed |
| `branch_deleted` | Branch no longer exists |
| `date_passed` | Specified date has passed |
| `nixpkg_version` | Package version exists in nixpkgs |

## Testing Conventions

- Use `cargo-insta` snapshot tests for parser output and error messages
- Use `MockForgeClient` (in `ops::github::mock`) for operation checking tests
- Use `tempfile` for test fixtures with directory trees
- Test error recovery paths — one bad file shouldn't halt the scan
- Always use `cargo nextest run`, never `cargo test`

## Implementation Plan

See `docs/IMPLEMENTATION_PLAN.md` for the phased build plan. See `docs/REQUIREMENTS.md` for full specification. Research findings are in `docs/research/`.
