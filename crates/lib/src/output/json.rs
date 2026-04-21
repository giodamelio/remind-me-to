use crate::ops::types::CheckResult;

/// Format check results as JSON output.
pub fn format_json(result: &CheckResult) {
    let json = serde_json::to_string_pretty(result).unwrap_or_else(|e| {
        eprintln!("error: failed to serialize JSON: {e}");
        "{}".to_string()
    });
    println!("{json}");
}
