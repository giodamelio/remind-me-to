# Research: Tracing Pretty Formatter

## Context

`remind-me-to` uses the `tracing` crate for structured logging. The logging serves two purposes:

1. **Development/debugging:** developers need to see what's happening (network requests, parse decisions, etc.)
2. **User-facing verbosity:** when users pass higher log levels, they see more detail about what the tool is doing

We need a pretty-printing layer that:
- Looks good in a terminal (colors, readable formatting)
- Works well for a CLI tool (not a long-running server)
- Shows useful context without being overwhelming
- Integrates with `--log-level` flag and `RUST_LOG` env var

The tool's primary output (triggered reminders) goes to stdout. Logging/tracing goes to stderr so they don't intermix.

## Candidates

### `tracing-subscriber` with `fmt` layer

- Built-in to the tracing ecosystem
- Basic but functional
- `Pretty` formatter for human-readable output
- `Compact` formatter for denser output

### `tracing-tree`

- https://crates.io/crates/tracing-tree
- Hierarchical, indented output showing span nesting
- Good for understanding call flow

### Custom `fmt::Layer`

- Write our own formatting
- Full control over output
- More work

### Other options?

- `tracing-forest` — tree-like output
- `tracing-human-layer` — designed for CLI tools
- Check what modern CLI tools use

## Evaluation Criteria

1. **Readability for CLI use** — is it clear and scannable for a one-shot CLI tool?
2. **Color support** — does it detect terminal capabilities?
3. **Stderr output** — easy to direct to stderr?
4. **Level filtering** — integrates with `EnvFilter`?
5. **Span display** — how does it show tracing spans? (useful for "checking github:foo/bar..." context)
6. **Compile time impact** — how heavy?
7. **Maintenance status** — actively maintained?

## Questions to Answer

1. For a one-shot CLI tool (not a server), is hierarchical/tree output useful or just noisy?
2. What do other Rust CLI tools use for their tracing output? (examples from ripgrep, cargo, etc.)
3. Can `tracing-subscriber::fmt::Pretty` be configured to look good for our use case without a custom layer?
4. How do we handle the case where `--quiet` suppresses all output but we still want errors to go somewhere?
5. Should spans show timing information? (useful for debugging slow API calls)
6. Is there a way to make tracing output "progressive" — show a progress indicator for long-running API checks?

## Findings

<!-- To be filled in by research agent -->

## Recommendation

<!-- To be filled in by research agent -->
