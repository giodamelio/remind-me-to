# Findings: Error Handling Strategy

## Summary

Use **`thiserror` (lib) + `ariadne` (parse errors) + `miette` (CLI)** as the recommended approach. This gives us structured library errors for programmatic consumers, chumsky-native error rendering for parse diagnostics, and miette's diagnostic framework for all other CLI error presentation. The key insight is that parse errors and operational errors have fundamentally different needs: parse errors benefit from ariadne's tight integration with chumsky spans, while operational errors (network, rate limits, config) benefit from miette's protocol-based diagnostic system.

## Candidate Analysis

### `thiserror` + `anyhow`

**Strengths:**
- Battle-tested, minimal overhead
- `thiserror` generates `Display` and `Error` impls from enums - perfect for structured lib errors
- `anyhow` gives easy context chaining with `.context()`

**Weaknesses:**
- `anyhow` errors are opaque - consumers can't match on them (fine for CLI, bad if we ever expose errors programmatically from the CLI crate)
- No built-in pretty diagnostic rendering - would need to build source-snippet display manually
- No colored output, no source spans, no fancy rendering without significant custom work
- Doesn't solve the "display parse errors with source context" problem at all

**Verdict:** Insufficient for our needs. The CLI needs pretty parse error output and `anyhow` provides none of that.

### `thiserror` + `miette`

**Strengths:**
- `miette` is a full diagnostic protocol (error codes, source spans, labels, help text, URLs)
- Derive macro makes it easy: `#[diagnostic(code(remind_me_to::parse_error))]`
- Multiple rendering backends (graphical, narrated for screen readers, JSON)
- Can carry `SourceSpan` and `SourceCode` for inline source display
- Works as both a library protocol and a CLI reporter
- Integrates smoothly with `thiserror` - you can derive both `Error` and `Diagnostic`

**Weaknesses:**
- Requires converting chumsky's `Rich` errors into miette `Diagnostic` types - not automatic
- Source spans use miette's own `SourceSpan` type (byte offset + length), not chumsky's spans directly
- Two diagnostic systems if we also use ariadne for parse errors
- Heavier dependency than anyhow

**Verdict:** Strong choice if we want a unified diagnostic system. The chumsky conversion is manual but straightforward.

### `thiserror` + `eyre`/`color-eyre`

**Strengths:**
- `eyre` is like `anyhow` but with customizable error hooks
- `color-eyre` adds beautiful panic reports and error chain display with colors
- SpanTrace integration with `tracing` (we already use tracing)
- Good for "unexpected" errors with full backtraces

**Weaknesses:**
- Designed more for unexpected/panic-like errors, not structured diagnostics
- No source-span rendering (no inline code snippets)
- Not really designed for parser error reporting
- Less useful for "expected" errors like "PR not found" or "rate limited"

**Verdict:** Good complement for fatal/unexpected errors but doesn't solve parse error display. Could be combined with ariadne but adds complexity.

### `error-stack`

**Strengths:**
- Works for both libraries and binaries
- Rich error stacks with arbitrary attachments (suggestions, context)
- Compatible with standard `Error` trait
- Can convert from `anyhow`/`eyre` errors
- Good for tracing error propagation through layers

**Weaknesses:**
- More development overhead than thiserror for defining root errors
- No built-in source-span diagnostic rendering
- Less ecosystem adoption than thiserror+anyhow/miette
- Still need ariadne or miette for pretty parse error output
- API is less familiar to most Rust developers

**Verdict:** Interesting but adds complexity without solving our core diagnostic rendering needs. Better suited for large services than CLI tools.

### `chumsky` + `ariadne`

**Strengths:**
- Sister projects by the same author (zesterer) - designed to work together
- Ariadne directly consumes chumsky's span types
- Beautiful multi-line source-annotated error output with colors
- Supports multiple labels per report, notes, and help messages
- Zero conversion needed for parse errors - just feed chumsky errors to ariadne
- Battle-tested in the chumsky examples and many parser projects

**Weaknesses:**
- Ariadne is only a reporter (the rendering part) - no error definition protocol
- Builder-based API requires manual construction of reports
- Only renders in one format (graphical terminal) - no JSON/narrated mode
- Doesn't help with non-parse errors (network failures, config issues)
- Not an error handling strategy on its own - only covers display

**Verdict:** Essential for parse error rendering. Must be combined with another strategy for non-parse errors.

## Chumsky Integration

### How Chumsky Errors Work

Chumsky 1.0 (current) provides two built-in error types:

1. **`Simple<T>`** - Tracks span and found token. Lightweight, good for medium performance.
2. **`Rich<'a, T>`** - Tracks spans, expected inputs, found input, labels, and context. Best for user-facing error messages.

Both are generic over the token type and use `SimpleSpan` (a `(usize, usize)` byte-offset range) by default.

Errors are collected during parsing via `extra::Err<Rich<'a, char>>` (or `Simple`). When using error recovery (`.recover_with(...)`), the parser continues past errors, collecting multiple errors while still producing a partial result.

### What Ariadne Provides

Ariadne renders diagnostic reports to the terminal:
- Colored source snippets with underlines
- Multiple labeled spans per report
- Notes, help text, error codes
- Multi-file support with a source cache

### Bridging Chumsky to Ariadne

The conversion is direct since they share span conventions:

```rust
use ariadne::{Report, ReportKind, Label, Source};
use chumsky::error::Rich;

fn report_parse_errors(errors: Vec<Rich<char>>, source: &str, filename: &str) {
    for error in errors {
        let span = error.span();
        Report::build(ReportKind::Error, filename, span.start)
            .with_message(format!("{}", error))
            .with_label(
                Label::new((filename, span.into_range()))
                    .with_message(format!("{}", error.reason()))
                    .with_color(ariadne::Color::Red),
            )
            .finish()
            .eprint((filename, Source::from(source)))
            .unwrap();
    }
}
```

### Key Design Point

Chumsky's error recovery means a single parse run can produce **multiple errors** alongside a partial result. This maps directly to our requirement that "parse error in one file shouldn't stop scan." We get this for free within a single file, and across files we simply continue scanning.

## Proposed Error Hierarchy

### Library Crate (`remind-me-to-lib`)

```rust
// Top-level result type for the library
pub struct ScanResult {
    pub reminders: Vec<Reminder>,
    pub errors: Vec<ScanError>,  // Non-fatal errors collected during scan
}

// Structured error enum - consumers can match on variants
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("parse error in {file}:{line}: {message}")]
    Parse {
        file: PathBuf,
        line: usize,
        col: usize,
        message: String,
        // Raw span info for rendering
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

// Errors from the checking phase
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

// Fatal errors that should stop execution
#[derive(Debug, thiserror::Error)]
pub enum FatalError {
    #[error("configuration error: {message}")]
    Config { message: String },

    #[error("no files to scan")]
    NoInput,
}
```

### CLI Crate (`remind-me-to-cli`)

```rust
// CLI uses ariadne for parse errors, miette for everything else
// Exit code 2 for fatal errors

use miette::{Diagnostic, Result as MietteResult};

#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("remind-me-to encountered a fatal error")]
#[diagnostic(code(remind_me_to::fatal))]
pub enum CliError {
    #[error("configuration error: {0}")]
    #[diagnostic(help("Check your config at ~/.config/remind-me-to/config.toml"))]
    Config(String),

    #[error("{0}")]
    Fatal(#[from] remind_me_to_lib::FatalError),
}
```

## Answers to Questions

### 1. Does chumsky have a preferred error reporting crate?

**Yes: ariadne.** They are sister projects by the same author (zesterer). Ariadne is not a dependency of chumsky, but they are designed to complement each other. All chumsky examples use ariadne for error rendering. The span types are compatible by design.

### 2. Can miette consume chumsky errors, or does it need conversion?

**Conversion is required.** Chumsky's `Rich` error type does not implement miette's `Diagnostic` trait. You would need to convert chumsky spans (`SimpleSpan` = `(usize, usize)`) into miette's `SourceSpan` (offset + length), and map the error reasons into miette labels. This is doable but adds a conversion layer that ariadne doesn't need.

### 3. For non-parse errors (network failures, rate limits), do we need source-span reporting?

**No.** Network failures, rate limits, and auth errors don't have a meaningful source location in user code. They relate to operations, not to syntax. These should be displayed as simple structured messages with context (which forge, which operation, when rate limit resets). A clear message with the reminder's file:line reference is sufficient - no need for source snippets.

### 4. What does the error type hierarchy look like?

See the "Proposed Error Hierarchy" section above. Key design:
- `ScanError` - non-fatal errors during file scanning/parsing (collected, not thrown)
- `CheckError` - non-fatal errors during operation checking (per-operation, doesn't stop run)
- `FatalError` - errors that prevent the tool from running at all (config, no input)
- `ScanResult` carries both successful results AND errors simultaneously

### 5. How do other CLI tools that use chumsky handle error reporting?

Most projects using chumsky use ariadne directly for error display. The typical pattern is:
1. Parse with `Rich` error type and error recovery enabled
2. Collect all errors from the parse result
3. For each error, build an `ariadne::Report` with labels from the error's span
4. Print reports to stderr

Projects include toy language compilers, configuration parsers, and query language tools. The pattern is consistent across them.

### 6. Is miette overkill if we're already using ariadne for chumsky parse errors?

**Partially, but there's a role for it.** If parse errors are the only errors that need fancy rendering (source snippets, colored labels), then ariadne alone handles that. However, miette adds value for:
- Structured diagnostic codes (useful for documentation/searchability)
- Help text on operational errors ("try setting GITHUB_TOKEN")
- Consistent error rendering for non-parse errors
- Future: JSON diagnostic output mode

**Pragmatic answer:** Start with ariadne for parse errors and simple `Display` formatting for operational errors. Add miette later only if the operational error UX proves insufficient. The library's `thiserror` enums are the stable API boundary regardless.

## Recommendation

### Primary Strategy: `thiserror` (lib) + `ariadne` (parse errors) + simple Display (operational errors)

**For the library crate:**
- Use `thiserror` to define structured error enums (`ScanError`, `CheckError`, `FatalError`)
- Return `ScanResult { reminders, errors }` - never panic, never stop on non-fatal errors
- Parse errors carry all the span/source info needed for rendering (but the lib doesn't render)
- The library is rendering-agnostic - consumers decide how to display

**For the CLI crate:**
- Use `ariadne` to render parse errors with colored source snippets
- Use simple colored output (via `owo-colors` or `termcolor`) for operational errors
- Fatal errors print a clear message to stderr and exit with code 2
- Non-fatal errors display inline in the output stream

**Why not miette everywhere:**
- Adds a conversion layer between chumsky and miette that ariadne avoids
- Two diagnostic rendering systems is confusing in one binary
- miette's source-span protocol is designed for errors you define, not errors from an external parser
- Ariadne produces output that's already excellent for parse diagnostics

**Why not error-stack:**
- More complex API for no clear benefit in a CLI tool
- Doesn't solve the rendering problem
- Less familiar to contributors

**Defer miette to later** if operational errors prove hard to make user-friendly with plain formatting. The thiserror enums in the library don't preclude adding miette later.

## Example Code

### Library: Parse Error Collection

```rust
use chumsky::prelude::*;
use std::path::PathBuf;

/// Result of scanning files - contains both successes and errors
pub struct ScanResult {
    pub reminders: Vec<Reminder>,
    pub errors: Vec<ScanError>,
}

/// Parse a single file, collecting errors without stopping
pub fn parse_file(path: &Path, content: &str) -> ScanResult {
    let (reminders, errors) = reminder_parser()
        .parse(content)
        .into_output_errors();

    let scan_errors: Vec<ScanError> = errors
        .into_iter()
        .map(|e| ScanError::Parse {
            file: path.to_owned(),
            line: offset_to_line(content, e.span().start),
            col: offset_to_col(content, e.span().start),
            message: format!("{}", e.reason()),
            span: e.span().into_range(),
            source_line: extract_line(content, e.span().start),
            expected: e.expected()
                .map(|exp| format!("{:?}", exp))
                .collect(),
            found: e.found().map(|f| format!("{:?}", f)),
        })
        .collect();

    ScanResult {
        reminders: reminders.unwrap_or_default(),
        errors: scan_errors,
    }
}
```

### CLI: Rendering Parse Errors with Ariadne

```rust
use ariadne::{Report, ReportKind, Label, Source, Color, ColorGenerator};
use remind_me_to_lib::ScanError;

fn render_parse_error(error: &ScanError) {
    if let ScanError::Parse { file, span, message, source_line, expected, .. } = error {
        let filename = file.display().to_string();
        let mut colors = ColorGenerator::new();
        let primary = colors.next();

        let mut report = Report::build(ReportKind::Error, &filename, span.start)
            .with_message(format!("failed to parse reminder"));

        report = report.with_label(
            Label::new((&filename, span.clone()))
                .with_message(message)
                .with_color(primary),
        );

        if !expected.is_empty() {
            report = report.with_note(
                format!("expected one of: {}", expected.join(", "))
            );
        }

        // Read source from the file for ariadne
        let source = std::fs::read_to_string(file).unwrap_or_default();
        report.finish()
            .eprint((&filename, Source::from(&source)))
            .unwrap();
    }
}
```

### CLI: Rendering Operational Errors Inline

```rust
use owo_colors::OwoColorize;
use remind_me_to_lib::CheckError;

fn render_check_error(error: &CheckError, location: &str) {
    match error {
        CheckError::RateLimited { forge, reset_at } => {
            eprintln!(
                "  {} rate limited by {} (resets {})",
                "!".yellow().bold(),
                forge,
                reset_at,
            );
        }
        CheckError::AuthRequired { forge } => {
            eprintln!(
                "  {} authentication required for {}",
                "!".yellow().bold(),
                forge,
            );
            eprintln!(
                "    {} set {} environment variable",
                "hint:".cyan(),
                format!("{}_TOKEN", forge.to_uppercase()),
            );
        }
        CheckError::ApiError { operation, message, .. } => {
            eprintln!(
                "  {} {} - {}",
                "x".red().bold(),
                operation,
                message,
            );
        }
        _ => {
            eprintln!("  {} {}", "x".red().bold(), error);
        }
    }
}
```

### CLI: Fatal Error and Exit

```rust
fn main() {
    let result = run();
    match result {
        Ok(exit_code) => std::process::exit(exit_code),
        Err(fatal) => {
            eprintln!("{} {}", "error:".red().bold(), fatal);
            std::process::exit(2);
        }
    }
}

fn run() -> Result<i32, remind_me_to_lib::FatalError> {
    let config = load_config()?;
    let scan_result = remind_me_to_lib::scan(&config)?;

    // Render non-fatal parse errors
    for error in &scan_result.errors {
        render_parse_error(error);
    }

    // Check operations (non-fatal errors handled per-operation)
    let check_result = remind_me_to_lib::check(&scan_result.reminders, &config)?;

    // Render results
    let triggered = render_results(&check_result);

    Ok(if triggered > 0 { 1 } else { 0 })
}
```

### Dependencies

```toml
# remind-me-to-lib/Cargo.toml
[dependencies]
thiserror = "2"
chumsky = "1"

# remind-me-to-cli/Cargo.toml
[dependencies]
remind-me-to-lib = { path = "../remind-me-to-lib" }
ariadne = "0.4"
owo-colors = "4"
clap = { version = "4", features = ["derive"] }
```
