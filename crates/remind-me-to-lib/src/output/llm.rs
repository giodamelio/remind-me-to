use crate::ops::types::{CheckResult, OperationStatus};

/// Format check results as an LLM-friendly prompt.
pub fn format_llm(result: &CheckResult) {
    let triggered: Vec<_> = result.reminders.iter().filter(|r| r.triggered).collect();

    if triggered.is_empty() {
        println!("No reminders are currently triggered. No action needed.");
        return;
    }

    println!(
        "The following {} reminder(s) need attention:\n",
        triggered.len()
    );

    for (i, reminder) in triggered.iter().enumerate() {
        println!("## Reminder {}", i + 1);
        println!("- **File:** {}", reminder.reminder.file.display());
        println!("- **Line:** {}", reminder.reminder.line);
        println!(
            "- **Description:** {}",
            reminder.reminder.description
        );
        println!("- **Conditions met:**");

        for op_result in &reminder.results {
            let status = match op_result.status {
                OperationStatus::Triggered => "MET",
                OperationStatus::Pending => "NOT MET",
                OperationStatus::Error => "ERROR",
            };
            let detail = op_result
                .detail
                .as_deref()
                .unwrap_or("no details");
            println!("  - `{}`: {} — {}", op_result.operation, status, detail);
        }

        println!();
    }

    println!("Please review each reminder and take the described action. The conditions that triggered these reminders indicate it's time to remove workarounds, update code, or complete cleanup tasks as described.");
}
