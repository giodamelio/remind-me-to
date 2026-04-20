# Research: Error Handling Strategy

## Context

`remind-me-to` is split into a library crate and a CLI crate. This creates two different error-handling needs:

**Library crate needs:**
- Structured errors that programmatic consumers can match on
- Rich error context for user-facing messages (especially parser errors — "you wrote `pr_merged=bad` on line 42 of src/main.rs, did you mean `pr_merged=github:...`?")
- Errors that can carry source location (file, line number)
- Multiple errors from a single run (one file having a parse error shouldn't stop the whole scan)

**CLI crate needs:**
- Pretty, human-readable error output
- Colored output with source snippets for parse errors
- Non-fatal errors displayed inline (e.g., "couldn't check this operation, network timeout")
- Fatal errors displayed clearly and exit with code 2

**Key constraint:** The parser uses `chumsky`, which has its own error reporting system. We need to integrate with it rather than fight it.

## Candidates

### `thiserror` (for library) + `anyhow` (for CLI)

- `thiserror`: derive macro for custom error types
- `anyhow`: ergonomic error handling for applications
- Most common pattern in the Rust ecosystem

### `thiserror` (for library) + `miette` (for CLI)

- `miette`: fancy diagnostic output with source spans, labels, help text
- Designed for tools that report errors in source code (compilers, linters)
- Integrates well with parser error reporting
- https://crates.io/crates/miette

### `thiserror` (for library) + `eyre` (for CLI)

- `eyre`: like `anyhow` but with customizable error reporting
- `color-eyre`: pretty panic and error reports
- Less focused on source-span diagnostics than miette

### `error-stack`

- https://crates.io/crates/error-stack
- From the `hash` team
- Context-rich error chains
- Different philosophy than anyhow/eyre

### Lean on `chumsky`'s error system

- `chumsky` has built-in error recovery and reporting
- `ariadne` is the companion crate for pretty error display
- Maybe we should use `ariadne` for all source-location errors?

## Evaluation Criteria

1. **Library ergonomics** — how easy for lib consumers to handle errors programmatically?
2. **Source span support** — can we show "error at line 42, column 15" with a source snippet?
3. **Multiple error accumulation** — can we collect all errors from a scan and report them together?
4. **Integration with chumsky** — does it play well with chumsky's error types?
5. **Integration with tracing** — does it work alongside our tracing-based logging?
6. **Dependency weight** — how much does it add?
7. **User experience** — how pretty/helpful are the error messages for end users?

## Questions to Answer

1. Does `chumsky` have a preferred error reporting crate? (I think it's `ariadne` — confirm)
2. Can `miette` consume chumsky errors, or do we need to convert?
3. For non-parse errors (network failures, rate limits), do we need source-span reporting or just simple messages?
4. What does the error type hierarchy look like? Something like `ScanError`, `ParseError`, `CheckError`, `ConfigError`?
5. How do other CLI tools that use chumsky handle their error reporting? Any examples?
6. Is `miette` overkill if we're already using `ariadne` for chumsky parse errors?

## Findings

<!-- To be filled in by research agent -->

## Recommendation

<!-- To be filled in by research agent -->
