use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use colored::Colorize;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use remind_lib::errors::FatalError;
use remind_lib::ops::github::GitHubClient;
use remind_lib::ops::types::ForgeClient;

/// A CLI tool that scans source files for REMIND-ME-TO comments and checks
/// if their conditions have been met.
#[derive(Parser, Debug)]
#[command(name = "remind-me-to", version, about)]
struct Cli {
    /// When to use colors: auto (default, detect tty), always, never.
    /// Respects NO_COLOR env var.
    #[arg(long, global = true, default_value = "auto")]
    color: ColorMode,

    #[command(subcommand)]
    command: Option<Commands>,

    /// Arguments for the default check command (when no subcommand is given)
    #[command(flatten)]
    check_args: CheckArgs,
}

#[derive(Debug, Clone, ValueEnum)]
enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Scan files for REMIND-ME-TO comments and check their conditions
    Check(CheckArgs),
}

#[derive(clap::Args, Debug)]
struct CheckArgs {
    /// Paths to scan (defaults to current directory)
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,

    /// Output format
    #[arg(long, default_value = "text")]
    format: OutputFormat,

    /// Additional ignore patterns
    #[arg(long = "ignore")]
    ignore_patterns: Vec<String>,

    /// Don't respect .gitignore/.ignore files
    #[arg(long)]
    no_gitignore: bool,

    /// Find and parse comments without checking external services
    #[arg(long)]
    dry_run: bool,

    /// Increase verbosity (-v debug, -vv trace, -vvv trace with targets)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Suppress all output, only set exit code
    #[arg(short, long)]
    quiet: bool,

    /// Log level: error, warn, info, debug, trace
    #[arg(long)]
    log_level: Option<String>,
}

#[derive(Debug, Clone, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
    Llm,
}

/// Configure the colored crate based on --color flag.
/// Returns whether colors are enabled (for tracing's with_ansi).
fn configure_color(mode: &ColorMode) -> bool {
    match mode {
        ColorMode::Always => {
            colored::control::set_override(true);
            true
        }
        ColorMode::Never => {
            colored::control::set_override(false);
            false
        }
        ColorMode::Auto => {
            // Let the colored crate handle NO_COLOR / CLICOLOR / CLICOLOR_FORCE.
            // We just add tty detection on top: if stderr is not a tty, disable.
            colored::control::unset_override();
            let is_tty = std::io::stderr().is_terminal();
            if !is_tty {
                colored::control::set_override(false);
            }
            is_tty
        }
    }
}

/// Custom tracing formatter that omits the level prefix for INFO events
/// (since info is the default level and the prefix is noise), but shows
/// the level for everything else.
struct CustomFormatter {
    show_targets: bool,
    use_ansi: bool,
}

impl<S, N> fmt::FormatEvent<S, N> for CustomFormatter
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    N: for<'a> fmt::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &fmt::FmtContext<'_, S, N>,
        mut writer: fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let level = *event.metadata().level();

        // Only print level prefix for non-INFO levels
        if level != tracing::Level::INFO {
            if self.use_ansi {
                use colored::Colorize;
                let level_str = match level {
                    tracing::Level::ERROR => "ERROR".red().bold().to_string(),
                    tracing::Level::WARN => " WARN".yellow().to_string(),
                    tracing::Level::DEBUG => "DEBUG".blue().to_string(),
                    tracing::Level::TRACE => "TRACE".purple().to_string(),
                    _ => level.to_string(),
                };
                write!(writer, "{level_str} ")?;
            } else {
                write!(writer, "{level:>5} ")?;
            }
        }

        if self.show_targets
            && let Some(target) = event.metadata().module_path()
        {
            if self.use_ansi {
                use colored::Colorize;
                write!(writer, "{} ", target.dimmed())?;
            } else {
                write!(writer, "{target} ")?;
            }
        }

        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

fn init_tracing(verbosity: u8, quiet: bool, log_level: &Option<String>, use_ansi: bool) {
    let default_directive = if quiet {
        "error"
    } else if let Some(level) = log_level {
        level.as_str()
    } else {
        match verbosity {
            0 => "info",
            1 => "debug",
            2 => "trace",
            _ => "trace",
        }
    };

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!(
            "remind_lib={default_directive},cli={default_directive}"
        ))
    });

    let show_targets = verbosity >= 3;

    let fmt_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(use_ansi)
        .with_target(show_targets)
        .with_span_events(fmt::format::FmtSpan::CLOSE)
        .event_format(CustomFormatter {
            show_targets,
            use_ansi,
        });

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();
}

/// Resolve the GitHub token from environment variables.
fn resolve_github_token() -> Option<String> {
    std::env::var("GITHUB_TOKEN")
        .ok()
        .or_else(|| std::env::var("GH_TOKEN").ok())
}

fn run() -> Result<ExitCode, FatalError> {
    let cli = Cli::parse();

    let use_ansi = configure_color(&cli.color);

    let CheckArgs {
        paths,
        format,
        ignore_patterns,
        no_gitignore,
        dry_run,
        verbose,
        quiet,
        log_level,
    } = match cli.command {
        Some(Commands::Check(args)) => args,
        None => cli.check_args,
    };

    init_tracing(verbose, quiet, &log_level, use_ansi);

    let path_refs: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();

    if path_refs.is_empty() {
        return Err(FatalError::NoInput);
    }

    // Scan for reminders
    let scan_result = remind_lib::scanner::scan(&path_refs, !no_gitignore, &ignore_patterns);

    // Report parse errors
    if !quiet {
        for error in &scan_result.errors {
            eprintln!("{} {error}", "error:".red().bold());
        }
    }

    if dry_run {
        if !quiet {
            match format {
                OutputFormat::Json => {
                    let json = serde_json::json!({
                        "reminders": scan_result.reminders,
                        "errors": scan_result.errors.iter().map(|e| e.to_string()).collect::<Vec<_>>(),
                    });
                    println!("{}", serde_json::to_string_pretty(&json).unwrap());
                }
                OutputFormat::Text | OutputFormat::Llm => {
                    if scan_result.reminders.is_empty() {
                        println!("No REMIND-ME-TO comments found.");
                    } else {
                        println!(
                            "Found {} (dry run, no conditions checked):\n",
                            format!("{} reminder(s)", scan_result.reminders.len()).bold()
                        );
                        for reminder in &scan_result.reminders {
                            println!(
                                "{}{}{} {}",
                                reminder.file.display().to_string().cyan(),
                                ":".dimmed(),
                                reminder.line,
                                reminder.description
                            );
                            for op in &reminder.operations {
                                println!("  {}", format!("{op}").dimmed());
                            }
                            println!();
                        }
                    }
                }
            }
        }

        let exit = if !scan_result.errors.is_empty() {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        };
        return Ok(exit);
    }

    // Full check mode
    let token = resolve_github_token();
    let client: Box<dyn ForgeClient> = Box::new(GitHubClient::new(token));

    let check_result =
        remind_lib::ops::checker::check_all(&scan_result.reminders, client.as_ref(), 8);

    if !quiet {
        match format {
            OutputFormat::Text => {
                remind_lib::output::text::format_text(&check_result, verbose);
            }
            OutputFormat::Json => {
                remind_lib::output::json::format_json(&check_result);
            }
            OutputFormat::Llm => {
                remind_lib::output::llm::format_llm(&check_result);
            }
        }
    }

    let has_triggered = check_result.reminders.iter().any(|r| r.triggered);
    let has_errors = !scan_result.errors.is_empty() || !check_result.errors.is_empty();

    if has_errors {
        Ok(ExitCode::from(2))
    } else if has_triggered {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{} {e}", "error:".red().bold());
            ExitCode::from(2)
        }
    }
}
