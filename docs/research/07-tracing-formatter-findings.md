# Findings: Tracing Pretty Formatter

## Summary

For a one-shot CLI tool like `remind-me-to`, **`tracing-subscriber`'s built-in `fmt` layer with the `Compact` formatter** is the best default choice. It is well-maintained (part of the core tracing ecosystem), lightweight, produces single-line colored output, and integrates trivially with `EnvFilter` and stderr. For future enhancement, `tracing-indicatif` can add progress bars tied to spans without invasive code changes.

## Candidate Analysis

### `tracing-subscriber` fmt layer

The built-in fmt layer offers three formatters:

| Formatter | Output Style | Best For |
|-----------|-------------|----------|
| `Full` (default) | Single-line, span context before event | General use |
| `Compact` | Single-line, span fields appended to event line | CLI tools, short terminals |
| `Pretty` | Multi-line, verbose, one field per line | Local development/debugging only |

**Compact** is ideal for CLI tools:
- Single line per event keeps output scannable
- Span context is inlined (e.g., `check{repo="foo/bar"}: fetching latest release`)
- Colors via ANSI, auto-detected
- Configurable: `.with_target(false)`, `.with_file(false)`, `.with_thread_ids(false)` to reduce noise
- `.with_writer(std::io::stderr)` directs output to stderr
- `FmtSpan::CLOSE` can show span durations on exit

**Pretty** is too verbose for normal CLI use (multiple lines per event) but can be activated via an env var for deep debugging sessions.

**Key configuration options:**
- `.with_ansi(true/false)` — color support
- `.with_timer(tracing_subscriber::fmt::time::uptime())` — relative timestamps
- `.with_span_events(FmtSpan::CLOSE)` — show timing on span close
- `.with_writer(std::io::stderr)` — stderr output

### `tracing-tree`

- Produces hierarchical indented output showing span nesting with ASCII art
- 179K downloads/month, actively maintained
- Configuration: `HierarchicalLayer::new(indent_amount)` with options for `.with_ansi(true)`, `.with_timer()`, `.with_targets(true)`, `.with_deferred_spans(true)`, `.with_indent_lines(true)`
- Shows timing information per span
- **Verdict for CLI tools:** Visually appealing for development/debugging but too verbose for user-facing output. The indentation implies long-running nested operations which a one-shot CLI doesn't typically have. Could be useful as a debug-only mode (e.g., `RUST_LOG=trace` activates tree view).

### `tracing-forest`

- Designed as a concurrent-safe alternative to tracing-tree
- Collects entire span trees and prints them coherently even in parallel contexts
- Output uses box-drawing characters (e.g., `INFO conn [ 150us | 100.00% ]`)
- Feature flags: `ansi`, `chrono`, `uuid`, `serde`
- **Verdict:** Overkill for a sequential CLI tool. Its main advantage (coherent output in concurrent contexts) isn't needed here. The buffered output model (waits for span to close before printing) means no real-time feedback during long operations.

### `tracing-human-layer`

- Designed specifically for CLI tools
- Human-friendly colored output with line wrapping
- Writes to stderr by default
- Performance: 1.92-6.17us to format, 12.55us including write
- Simple setup: `registry().with(HumanLayer::new()).init()`
- Customizable styles per log level via `LayerStyles` and `ProvideStyle` trait
- **Verdict:** Promising but relatively niche (low download count compared to fmt). The extra dependency may not justify the benefit over a well-configured `fmt::layer().compact()`. Worth watching but not the safe choice today.

### Custom layer

A custom `fmt::Layer` makes sense when:
- You want to strip all "log-like" decoration (timestamps, levels) for user-facing messages at INFO level while keeping full detail at DEBUG/TRACE
- You need to integrate with indicatif progress bars
- You want different formatting per log level (e.g., WARN/ERROR get prefixed, INFO is plain text)

For `remind-me-to`, a custom layer is **not needed initially**. The compact formatter with minimal decoration handles the use case well.

## What Other Tools Use

| Tool | Approach |
|------|----------|
| **rustup** | `tracing-subscriber` fmt layer; legacy stderr format when `RUSTUP_LOG` unset; standard tracing format when set |
| **rustc** | `tracing-subscriber` with `EnvFilter`, `RUSTC_LOG` env var |
| **cargo** | Custom logging built on `log` crate (predates tracing), stderr output |
| **git-branchless** | `tracing-subscriber` with custom `Effects` type to coordinate with progress meters |
| **ripgrep** | Minimal logging, not tracing-based |

The pattern across the ecosystem is: **`tracing-subscriber` fmt layer + `EnvFilter` + stderr** is the standard approach for Rust CLI tools that use tracing.

## Answers to Questions

### 1. For a one-shot CLI tool, is hierarchical/tree output useful or noisy?

**Mostly noisy.** Tree output shines when you have deep call stacks executing over time and want to understand nesting. A one-shot CLI that does: parse args -> check API -> compare versions -> print results has at most 2-3 levels of nesting. The indentation adds visual weight without much clarity benefit. However, it can be valuable as a *debug mode* activated by `RUST_LOG=trace` for development.

### 2. What do other Rust CLI tools use for tracing output?

Standard `tracing-subscriber` fmt layer with `EnvFilter`. See table above. The pattern is:
- Default: minimal or no log output
- `TOOL_LOG=info` or `--verbose`: compact single-line logs to stderr
- `TOOL_LOG=debug,trace`: full detail for debugging

### 3. Can `tracing-subscriber::fmt::Pretty` be configured to look good without a custom layer?

**Not really for normal use.** Pretty always produces multi-line output which is too verbose for typical CLI operation. It's fine for debugging sessions but shouldn't be the default. Use `Compact` as default, offer Pretty via env var (e.g., `REMIND_ME_LOG_FORMAT=pretty`).

### 4. How to handle --quiet suppressing all output but still wanting errors somewhere?

Use `MakeWriterExt` to route by level:

```rust
use tracing_subscriber::fmt::writer::MakeWriterExt;

let stderr = std::io::stderr;
// In quiet mode, only WARN and ERROR go to stderr
let writer = stderr.with_max_level(tracing::Level::WARN);
```

Or with `EnvFilter`:
- `--quiet`: set filter to `off` (suppress all tracing output; errors still go to stderr via eprintln for critical failures)
- Normal: filter to `warn`
- `--verbose` / `-v`: filter to `info`
- `-vv`: filter to `debug`
- `-vvv` or `RUST_LOG=trace`: filter to `trace`

### 5. Should spans show timing information?

**Yes, but only on span close and only at debug level or higher verbosity.** Use `FmtSpan::CLOSE` which prints the span with its duration when it exits. This is valuable for identifying slow API calls without cluttering normal output. Example output:

```
DEBUG remind_me_to::github: close check_release{repo="foo/bar"} time.busy=245ms time.idle=2us
```

### 6. Is there a way to make tracing output "progressive" (progress indicator for long API checks)?

**Yes: `tracing-indicatif`.** This crate automatically creates indicatif progress bars for active spans. Setup is minimal:

```rust
let indicatif_layer = IndicatifLayer::new();
tracing_subscriber::registry()
    .with(fmt::layer().with_writer(indicatif_layer.get_stderr_writer()))
    .with(indicatif_layer)
    .init();
```

Progress bars appear when spans are entered and disappear when they close. This is the cleanest way to show progress for long API calls without manual progress bar management. Consider this as a phase 2 enhancement.

## Recommendation

**Phase 1: `tracing-subscriber` fmt layer with Compact formatter**

This is the pragmatic choice:
- Zero extra dependencies beyond what tracing-subscriber already provides
- Battle-tested across the Rust ecosystem
- Easy to configure for CLI use
- Integrates with `EnvFilter` for `RUST_LOG` and `--verbose` flags

**Phase 2 (optional): Add `tracing-indicatif` for progress**

If API calls are noticeably slow (>500ms), add progress bars tied to spans.

## Example Configuration

```rust
use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize tracing based on CLI verbosity and environment.
///
/// Priority: RUST_LOG env var > --log-level flag > default (warn)
pub fn init_tracing(verbosity: u8) {
    // Determine the default filter level from CLI flags
    let default_directive = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    // RUST_LOG takes priority over CLI flags
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            EnvFilter::new(format!("remind_me_to={default_directive}"))
        });

    // Configure the fmt layer
    let fmt_layer = fmt::layer()
        .compact()
        .with_writer(std::io::stderr)
        .with_target(verbosity >= 3) // show module paths only at trace
        .with_ansi(true) // TODO: detect terminal with supports-color crate
        .with_span_events(fmt::format::FmtSpan::CLOSE); // show span durations

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();
}
```

### Quiet mode variant

```rust
pub fn init_tracing(verbosity: u8, quiet: bool) {
    if quiet {
        // In quiet mode, only show errors
        let env_filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("error"));

        let fmt_layer = fmt::layer()
            .compact()
            .with_writer(std::io::stderr)
            .with_target(false);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .init();
    } else {
        init_tracing(verbosity);
    }
}
```

### Phase 2: With progress bars

```rust
use tracing_indicatif::IndicatifLayer;

pub fn init_tracing_with_progress(verbosity: u8) {
    let default_directive = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("remind_me_to={default_directive}")));

    let indicatif_layer = IndicatifLayer::new();

    let fmt_layer = fmt::layer()
        .compact()
        .with_writer(indicatif_layer.get_stderr_writer())
        .with_target(verbosity >= 3)
        .with_span_events(fmt::format::FmtSpan::CLOSE);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(indicatif_layer)
        .init();
}
```

### Cargo.toml dependencies

```toml
[dependencies]
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Phase 2 (optional)
# tracing-indicatif = "0.3"
```

## Sources

- [tracing-subscriber fmt format docs](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/index.html)
- [tracing-subscriber fmt module](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/index.html)
- [tracing-tree on crates.io](https://crates.io/crates/tracing-tree)
- [HierarchicalLayer docs](https://docs.rs/tracing-tree/latest/tracing_tree/struct.HierarchicalLayer.html)
- [tracing-forest on crates.io](https://crates.io/crates/tracing-forest)
- [tracing-human-layer docs](https://docs.rs/tracing-human-layer/latest/tracing_human_layer/)
- [tracing-indicatif docs](https://docs.rs/tracing-indicatif/latest/tracing_indicatif/)
- [Rustup dev guide: tracing](https://rust-lang.github.io/rustup/dev-guide/tracing.html)
- [Using tracing with Rust CLI apps (Waleed Khan)](https://blog.waleedkhan.name/tracing-rust-cli-apps/)
- [clap-verbosity-flag](https://github.com/clap-rs/clap-verbosity-flag)
- [EnvFilter docs](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html)
