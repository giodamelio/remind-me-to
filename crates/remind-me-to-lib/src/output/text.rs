use crate::ops::types::{CheckResult, CheckedReminder, OperationStatus};

/// Format check results as human-readable text output.
pub fn format_text(result: &CheckResult, verbose: bool) {
    let triggered: Vec<&CheckedReminder> =
        result.reminders.iter().filter(|r| r.triggered).collect();
    let pending: Vec<&CheckedReminder> =
        result.reminders.iter().filter(|r| !r.triggered).collect();

    if triggered.is_empty() && !verbose {
        println!(
            "{} reminder(s) found, 0 triggered.",
            result.reminders.len()
        );
        return;
    }

    // Show triggered reminders
    for reminder in &triggered {
        println!(
            "{}:{}: {}",
            reminder.reminder.file.display(),
            reminder.reminder.line,
            reminder.reminder.description
        );
        for op_result in &reminder.results {
            let indicator = match op_result.status {
                OperationStatus::Triggered => "\u{2713}",
                OperationStatus::Pending => "\u{00b7}",
                OperationStatus::Error => "!",
            };
            let detail = op_result
                .detail
                .as_deref()
                .map(|d| format!(" ({d})"))
                .unwrap_or_default();
            println!("  {indicator} {}{detail}", op_result.operation);
        }
        println!();
    }

    // Show pending reminders in verbose mode
    if verbose && !pending.is_empty() {
        println!("--- Pending reminders ---\n");
        for reminder in &pending {
            println!(
                "{}:{}: {}",
                reminder.reminder.file.display(),
                reminder.reminder.line,
                reminder.reminder.description
            );
            for op_result in &reminder.results {
                let indicator = match op_result.status {
                    OperationStatus::Triggered => "\u{2713}",
                    OperationStatus::Pending => "\u{00b7}",
                    OperationStatus::Error => "!",
                };
                let detail = op_result
                    .detail
                    .as_deref()
                    .map(|d| format!(" ({d})"))
                    .unwrap_or_default();
                println!("  {indicator} {}{detail}", op_result.operation);
            }
            println!();
        }
    }

    // Summary
    if !triggered.is_empty() {
        println!(
            "{} triggered, {} pending.",
            triggered.len(),
            pending.len()
        );
    }
}
