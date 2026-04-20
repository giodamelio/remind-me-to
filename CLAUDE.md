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
```

## Project Overview

`remind-me-to` is a CLI tool that scans source files for `REMIND-ME-TO:` comments containing machine-checkable conditions (e.g., "remove this workaround when upstream PR merges"). It queries external APIs (GitHub, Gitea, Forgejo) to check if conditions are met, then reports which reminders need attention.

## Architecture (Planned Workspace Layout)

```
crates/
  remind-me-to-lib/    # Core library
    src/
      scanner/         # File walking (ignore crate) + comment parsing (chumsky)
      ops/             # Operation checkers (pr_merged, tag_exists, etc.)
      output/          # Formatters (text, JSON, LLM prompt)
      errors.rs        # Error types (thiserror + miette)
  remind-me-to-cli/    # Binary crate (clap + config)
```

## Key Design Decisions

- **Parser:** `chumsky` for comment parsing with error recovery
- **File walking:** `ignore` crate (respects .gitignore, parallel walking)
- **HTTP:** `ureq` (blocking, simple) with `httpmock` for tests
- **Version comparison:** `versions` crate (handles semver, calver, 4-segment)
- **Error handling:** `thiserror` for types, `miette` for CLI rendering
- **CLI:** `clap` derive, `config` crate for TOML config files
- **Logging:** `tracing` with Compact formatter
- **CLI testing:** `assert_cmd` + `insta-cmd` for snapshot testing

## Implementation Plan

See `docs/IMPLEMENTATION_PLAN.md` for the phased build plan. See `docs/REQUIREMENTS.md` for full specification. Research findings are in `docs/research/`.

## Testing Conventions

- Use `cargo-insta` snapshot tests for parser output and error messages
- Use `httpmock` for HTTP integration tests (not real network calls)
- Use `assert_cmd` + `insta-cmd` for CLI binary tests
- Test error recovery paths — one bad file shouldn't halt the scan
