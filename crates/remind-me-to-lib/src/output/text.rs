use colored::Colorize;

use crate::ops::types::{CheckResult, CheckedReminder, OperationStatus};

/// Format check results as human-readable text output.
pub fn format_text(result: &CheckResult, verbose: bool) {
    let triggered: Vec<&CheckedReminder> =
        result.reminders.iter().filter(|r| r.triggered).collect();
    let pending: Vec<&CheckedReminder> =
        result.reminders.iter().filter(|r| !r.triggered).collect();

    if triggered.is_empty() && !verbose {
        println!(
            "{} reminder(s) found, {} triggered.",
            result.reminders.len().to_string().bold(),
            "0".green().bold(),
        );
        return;
    }

    for reminder in &triggered {
        print_reminder(reminder);
    }

    if verbose && !pending.is_empty() {
        println!("{}\n", "--- Pending reminders ---".dimmed());
        for reminder in &pending {
            print_reminder(reminder);
        }
    }

    if !triggered.is_empty() {
        println!(
            "{} triggered, {} pending.",
            triggered.len().to_string().yellow().bold(),
            pending.len().to_string().dimmed(),
        );
    }
}

fn print_reminder(reminder: &CheckedReminder) {
    println!(
        "{}{}{} {}",
        reminder.reminder.file.display().to_string().cyan(),
        ":".dimmed(),
        reminder.reminder.line.to_string().yellow(),
        reminder.reminder.description,
    );
    for op_result in &reminder.results {
        let indicator = match op_result.status {
            OperationStatus::Triggered => "\u{2713}".green().bold().to_string(),
            OperationStatus::Pending => "\u{00b7}".dimmed().to_string(),
            OperationStatus::Error => "!".red().bold().to_string(),
        };
        let detail = op_result
            .detail
            .as_deref()
            .map(|d| format!(" ({})", d.dimmed()))
            .unwrap_or_default();
        println!("  {indicator} {}{detail}", op_result.operation);
    }
    println!();
}
