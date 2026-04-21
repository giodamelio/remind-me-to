use std::collections::HashMap;

use crate::ops::types::*;
use crate::ops::version::check_version_constraint;

/// Check all reminders against the forge API.
/// Returns a CheckResult with checked reminders and any errors.
pub fn check_all(
    reminders: &[Reminder],
    client: &dyn ForgeClient,
    max_concurrent: usize,
) -> CheckResult {
    tracing::debug!(reminders = reminders.len(), "checking operations");

    // Deduplicate operations across all reminders
    let mut unique_ops: Vec<Operation> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for reminder in reminders {
        for op in &reminder.operations {
            let key = op.to_string();
            if seen.insert(key) {
                unique_ops.push(op.clone());
            }
        }
    }

    tracing::debug!(
        total_operations = unique_ops.len(),
        deduplicated_from = reminders.iter().map(|r| r.operations.len()).sum::<usize>(),
        "deduplicated operations"
    );

    // Check all unique operations (in parallel if there are enough)
    let op_results: HashMap<String, OperationResult> =
        if unique_ops.len() <= 2 || max_concurrent <= 1 {
            // Sequential for small sets
            unique_ops
                .iter()
                .map(|op| {
                    let result = check_one(op, client);
                    (op.to_string(), result)
                })
                .collect()
        } else {
            // Parallel for larger sets
            check_parallel(&unique_ops, client, max_concurrent)
        };

    // Map results back to reminders
    let mut errors = Vec::new();
    let checked_reminders: Vec<CheckedReminder> = reminders
        .iter()
        .map(|reminder| {
            let results: Vec<OperationResult> = reminder
                .operations
                .iter()
                .map(|op| {
                    let key = op.to_string();
                    op_results.get(&key).cloned().unwrap_or(OperationResult {
                        operation: op.clone(),
                        status: OperationStatus::Error,
                        detail: Some("operation not checked".to_string()),
                    })
                })
                .collect();

            let triggered = results
                .iter()
                .any(|r| r.status == OperationStatus::Triggered);

            // Collect errors
            for r in &results {
                if r.status == OperationStatus::Error
                    && let Some(ref detail) = r.detail
                {
                    errors.push(detail.clone());
                }
            }

            CheckedReminder {
                reminder: reminder.clone(),
                results,
                triggered,
            }
        })
        .collect();

    CheckResult {
        reminders: checked_reminders,
        errors,
    }
}

/// Check operations in parallel using scoped threads.
fn check_parallel(
    ops: &[Operation],
    client: &dyn ForgeClient,
    max_concurrent: usize,
) -> HashMap<String, OperationResult> {
    std::thread::scope(|s| {
        let chunk_size = (ops.len() / max_concurrent).max(1);
        let chunks: Vec<_> = ops.chunks(chunk_size).collect();

        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| {
                s.spawn(|| {
                    chunk
                        .iter()
                        .map(|op| {
                            let result = check_one(op, client);
                            (op.to_string(), result)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();

        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    })
}

/// Check a single operation against the forge API.
fn check_one(op: &Operation, client: &dyn ForgeClient) -> OperationResult {
    tracing::debug!(operation = %op, "checking operation");
    let result = match op {
        Operation::PrMerged(issue_ref) => check_pr_merged(issue_ref, client),
        Operation::PrClosed(issue_ref) => check_pr_closed(issue_ref, client),
        Operation::TagExists(ref_ref) => check_tag_exists(ref_ref, client),
        Operation::CommitReleased(ref_ref) => check_commit_released(ref_ref, client),
        Operation::PrReleased(issue_ref) => check_pr_released(issue_ref, client),
        Operation::IssueClosed(issue_ref) => check_issue_closed(issue_ref, client),
        Operation::BranchDeleted(ref_ref) => check_branch_deleted(ref_ref, client),
        Operation::DatePassed(date) => check_date_passed(date),
    };
    tracing::debug!(operation = %op, status = ?result.status, "operation result");
    result
}

fn check_pr_merged(issue_ref: &IssueRef, client: &dyn ForgeClient) -> OperationResult {
    let op = Operation::PrMerged(issue_ref.clone());
    match client.get_pr_status(
        &issue_ref.forge_ref.owner,
        &issue_ref.forge_ref.repo,
        issue_ref.number,
    ) {
        Ok(pr) => {
            if pr.merged {
                OperationResult {
                    operation: op,
                    status: OperationStatus::Triggered,
                    detail: pr.merged_at.map(|d| format!("merged {d}")),
                }
            } else {
                OperationResult {
                    operation: op,
                    status: OperationStatus::Pending,
                    detail: Some(format!("PR is {:?}", pr.state)),
                }
            }
        }
        Err(e) => OperationResult {
            operation: op,
            status: OperationStatus::Error,
            detail: Some(e.to_string()),
        },
    }
}

fn check_pr_closed(issue_ref: &IssueRef, client: &dyn ForgeClient) -> OperationResult {
    let op = Operation::PrClosed(issue_ref.clone());
    match client.get_pr_status(
        &issue_ref.forge_ref.owner,
        &issue_ref.forge_ref.repo,
        issue_ref.number,
    ) {
        Ok(pr) => {
            if pr.state == PrState::Closed {
                OperationResult {
                    operation: op,
                    status: OperationStatus::Triggered,
                    detail: Some(if pr.merged {
                        "closed (merged)".to_string()
                    } else {
                        "closed (not merged)".to_string()
                    }),
                }
            } else {
                OperationResult {
                    operation: op,
                    status: OperationStatus::Pending,
                    detail: Some("PR is open".to_string()),
                }
            }
        }
        Err(e) => OperationResult {
            operation: op,
            status: OperationStatus::Error,
            detail: Some(e.to_string()),
        },
    }
}

fn check_tag_exists(ref_ref: &RefRef, client: &dyn ForgeClient) -> OperationResult {
    let op = Operation::TagExists(ref_ref.clone());
    match client.get_tags(&ref_ref.forge_ref.owner, &ref_ref.forge_ref.repo) {
        Ok(tags) => {
            let tag_names: Vec<String> = tags.iter().map(|t| t.name.clone()).collect();
            match check_version_constraint(&ref_ref.value, &tag_names) {
                Some(matched_tag) => OperationResult {
                    operation: op,
                    status: OperationStatus::Triggered,
                    detail: Some(format!("matched tag: {matched_tag}")),
                },
                None => {
                    let latest = tag_names.first().map(|t| t.as_str()).unwrap_or("none");
                    OperationResult {
                        operation: op,
                        status: OperationStatus::Pending,
                        detail: Some(format!("latest: {latest}, not yet")),
                    }
                }
            }
        }
        Err(e) => OperationResult {
            operation: op,
            status: OperationStatus::Error,
            detail: Some(e.to_string()),
        },
    }
}

/// Check if a commit SHA is included in the latest release.
/// The `op` parameter allows callers to preserve their original operation type.
fn check_sha_released(
    owner: &str,
    repo: &str,
    sha: &str,
    op: Operation,
    client: &dyn ForgeClient,
) -> OperationResult {
    // Fetch the latest release
    let release = match client.get_latest_release(owner, repo) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return OperationResult {
                operation: op,
                status: OperationStatus::Pending,
                detail: Some("no releases found".to_string()),
            };
        }
        Err(e) => {
            return OperationResult {
                operation: op,
                status: OperationStatus::Error,
                detail: Some(e.to_string()),
            };
        }
    };

    // Check if the commit is an ancestor of the latest release tag
    match client.is_ancestor(owner, repo, sha, &release.tag_name) {
        Ok(AncestorStatus::Ancestor) => OperationResult {
            operation: op,
            status: OperationStatus::Triggered,
            detail: Some(format!("included in release {}", release.tag_name)),
        },
        Ok(AncestorStatus::NotAncestor) => OperationResult {
            operation: op,
            status: OperationStatus::Pending,
            detail: Some(format!("not in latest release ({})", release.tag_name)),
        },
        Err(e) => OperationResult {
            operation: op,
            status: OperationStatus::Error,
            detail: Some(e.to_string()),
        },
    }
}

fn check_commit_released(ref_ref: &RefRef, client: &dyn ForgeClient) -> OperationResult {
    let op = Operation::CommitReleased(ref_ref.clone());
    check_sha_released(
        &ref_ref.forge_ref.owner,
        &ref_ref.forge_ref.repo,
        &ref_ref.value,
        op,
        client,
    )
}

fn check_pr_released(issue_ref: &IssueRef, client: &dyn ForgeClient) -> OperationResult {
    let op = Operation::PrReleased(issue_ref.clone());
    // First get the PR to find merge commit SHA
    match client.get_pr_status(
        &issue_ref.forge_ref.owner,
        &issue_ref.forge_ref.repo,
        issue_ref.number,
    ) {
        Ok(pr) => {
            if !pr.merged {
                return OperationResult {
                    operation: op,
                    status: OperationStatus::Pending,
                    detail: Some("PR not yet merged".to_string()),
                };
            }

            let Some(sha) = pr.merge_commit_sha else {
                return OperationResult {
                    operation: op,
                    status: OperationStatus::Error,
                    detail: Some("PR merged but no merge commit SHA".to_string()),
                };
            };

            check_sha_released(
                &issue_ref.forge_ref.owner,
                &issue_ref.forge_ref.repo,
                &sha,
                op,
                client,
            )
        }
        Err(e) => OperationResult {
            operation: op,
            status: OperationStatus::Error,
            detail: Some(e.to_string()),
        },
    }
}

fn check_issue_closed(issue_ref: &IssueRef, client: &dyn ForgeClient) -> OperationResult {
    let op = Operation::IssueClosed(issue_ref.clone());
    match client.get_issue_status(
        &issue_ref.forge_ref.owner,
        &issue_ref.forge_ref.repo,
        issue_ref.number,
    ) {
        Ok(issue) => {
            if issue.state == IssueState::Closed {
                OperationResult {
                    operation: op,
                    status: OperationStatus::Triggered,
                    detail: Some("issue closed".to_string()),
                }
            } else {
                OperationResult {
                    operation: op,
                    status: OperationStatus::Pending,
                    detail: Some("issue is open".to_string()),
                }
            }
        }
        Err(e) => OperationResult {
            operation: op,
            status: OperationStatus::Error,
            detail: Some(e.to_string()),
        },
    }
}

fn check_branch_deleted(ref_ref: &RefRef, client: &dyn ForgeClient) -> OperationResult {
    let op = Operation::BranchDeleted(ref_ref.clone());
    match client.branch_exists(
        &ref_ref.forge_ref.owner,
        &ref_ref.forge_ref.repo,
        &ref_ref.value,
    ) {
        Ok(exists) => {
            if !exists {
                OperationResult {
                    operation: op,
                    status: OperationStatus::Triggered,
                    detail: Some("branch deleted".to_string()),
                }
            } else {
                OperationResult {
                    operation: op,
                    status: OperationStatus::Pending,
                    detail: Some("branch still exists".to_string()),
                }
            }
        }
        Err(e) => OperationResult {
            operation: op,
            status: OperationStatus::Error,
            detail: Some(e.to_string()),
        },
    }
}

fn check_date_passed(date: &str) -> OperationResult {
    let op = Operation::DatePassed(date.to_string());

    // Parse the date (just the YYYY-MM-DD portion)
    let date_str = &date[..10.min(date.len())];
    let parts: Vec<&str> = date_str.split('-').collect();

    if parts.len() != 3 {
        return OperationResult {
            operation: op,
            status: OperationStatus::Error,
            detail: Some(format!("invalid date: {date}")),
        };
    }

    let Ok(year) = parts[0].parse::<i32>() else {
        return OperationResult {
            operation: op,
            status: OperationStatus::Error,
            detail: Some(format!("invalid year in date: {date}")),
        };
    };
    let Ok(month) = parts[1].parse::<u32>() else {
        return OperationResult {
            operation: op,
            status: OperationStatus::Error,
            detail: Some(format!("invalid month in date: {date}")),
        };
    };
    let Ok(day) = parts[2].parse::<u32>() else {
        return OperationResult {
            operation: op,
            status: OperationStatus::Error,
            detail: Some(format!("invalid day in date: {date}")),
        };
    };

    // Get current date
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Convert to days since epoch (approximate)
    let now_days = now / 86400;

    // Convert target date to days since epoch (approximate)
    let target_days = days_since_epoch(year, month, day);

    if now_days >= target_days {
        OperationResult {
            operation: op,
            status: OperationStatus::Triggered,
            detail: Some(format!("date {date} has passed")),
        }
    } else {
        OperationResult {
            operation: op,
            status: OperationStatus::Pending,
            detail: Some(format!("date {date} is in the future")),
        }
    }
}

/// Approximate days since Unix epoch for a date.
fn days_since_epoch(year: i32, month: u32, day: u32) -> u64 {
    // Simple approximation using the same method as mktime
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let m = if month <= 2 {
        month as i64 + 9
    } else {
        month as i64 - 3
    };
    let days = 365 * y + y / 4 - y / 100 + y / 400 + (m * 306 + 5) / 10 + day as i64 - 719469;
    days.max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::github::mock::MockForgeClient;
    use std::path::PathBuf;

    fn make_reminder(ops: Vec<Operation>) -> Reminder {
        Reminder {
            file: PathBuf::from("test.rs"),
            line: 1,
            description: "test reminder".to_string(),
            operations: ops,
        }
    }

    #[test]
    fn pr_merged_triggered() {
        let mut mock = MockForgeClient::new();
        mock.pr_responses.insert(
            ("owner".into(), "repo".into(), 1),
            Ok(PrStatus {
                number: 1,
                state: PrState::Closed,
                merged: true,
                merged_at: Some("2025-01-15".into()),
                merge_commit_sha: Some("abc".into()),
            }),
        );

        let reminders = vec![make_reminder(vec![Operation::PrMerged(IssueRef {
            forge_ref: ForgeRef {
                forge: "github".into(),
                owner: "owner".into(),
                repo: "repo".into(),
            },
            number: 1,
        })])];

        let result = check_all(&reminders, &mock, 1);
        assert!(result.reminders[0].triggered);
        assert_eq!(
            result.reminders[0].results[0].status,
            OperationStatus::Triggered
        );
    }

    #[test]
    fn pr_not_merged_pending() {
        let mut mock = MockForgeClient::new();
        mock.pr_responses.insert(
            ("owner".into(), "repo".into(), 1),
            Ok(PrStatus {
                number: 1,
                state: PrState::Open,
                merged: false,
                merged_at: None,
                merge_commit_sha: None,
            }),
        );

        let reminders = vec![make_reminder(vec![Operation::PrMerged(IssueRef {
            forge_ref: ForgeRef {
                forge: "github".into(),
                owner: "owner".into(),
                repo: "repo".into(),
            },
            number: 1,
        })])];

        let result = check_all(&reminders, &mock, 1);
        assert!(!result.reminders[0].triggered);
    }

    #[test]
    fn or_semantics_one_triggered() {
        let mut mock = MockForgeClient::new();
        mock.pr_responses.insert(
            ("owner".into(), "repo".into(), 1),
            Ok(PrStatus {
                number: 1,
                state: PrState::Closed,
                merged: true,
                merged_at: Some("2025-01-15".into()),
                merge_commit_sha: Some("abc".into()),
            }),
        );
        mock.tag_responses.insert(
            ("owner".into(), "repo".into()),
            Ok(vec![Tag {
                name: "v0.1.0".into(),
                commit_sha: "xxx".into(),
            }]),
        );

        let forge_ref = ForgeRef {
            forge: "github".into(),
            owner: "owner".into(),
            repo: "repo".into(),
        };

        let reminders = vec![make_reminder(vec![
            Operation::PrMerged(IssueRef {
                forge_ref: forge_ref.clone(),
                number: 1,
            }),
            Operation::TagExists(RefRef {
                forge_ref,
                value: ">=2.0.0".into(),
            }),
        ])];

        let result = check_all(&reminders, &mock, 1);
        assert!(result.reminders[0].triggered);
    }

    #[test]
    fn deduplication() {
        let mock = MockForgeClient::new();
        let forge_ref = ForgeRef {
            forge: "github".into(),
            owner: "owner".into(),
            repo: "repo".into(),
        };
        let op = Operation::PrMerged(IssueRef {
            forge_ref,
            number: 1,
        });

        let reminders = vec![
            make_reminder(vec![op.clone()]),
            make_reminder(vec![op.clone()]),
            make_reminder(vec![op]),
        ];

        let result = check_all(&reminders, &mock, 1);
        // Same operation should only be called once
        assert_eq!(mock.call_count("get_pr_status"), 1);
        assert_eq!(result.reminders.len(), 3);
    }

    #[test]
    fn date_passed_past() {
        let result = check_date_passed("2020-01-01");
        assert_eq!(result.status, OperationStatus::Triggered);
    }

    #[test]
    fn date_passed_future() {
        let result = check_date_passed("2099-12-31");
        assert_eq!(result.status, OperationStatus::Pending);
    }

    #[test]
    fn error_isolation() {
        let mut mock = MockForgeClient::new();
        // One operation will error (no response configured)
        // Another will succeed
        mock.issue_responses.insert(
            ("owner".into(), "repo".into(), 1),
            Ok(IssueStatus {
                number: 1,
                state: IssueState::Closed,
            }),
        );

        let forge_ref = ForgeRef {
            forge: "github".into(),
            owner: "owner".into(),
            repo: "repo".into(),
        };

        let reminders = vec![make_reminder(vec![
            Operation::PrMerged(IssueRef {
                forge_ref: forge_ref.clone(),
                number: 999, // no response configured → error
            }),
            Operation::IssueClosed(IssueRef {
                forge_ref,
                number: 1, // configured → success
            }),
        ])];

        let result = check_all(&reminders, &mock, 1);
        // Should still be triggered because issue_closed succeeded
        assert!(result.reminders[0].triggered);
    }

    #[test]
    fn branch_deleted_triggered() {
        let mock = MockForgeClient::new();
        // No branch response configured → defaults to false (not exists)
        let reminders = vec![make_reminder(vec![Operation::BranchDeleted(RefRef {
            forge_ref: ForgeRef {
                forge: "github".into(),
                owner: "owner".into(),
                repo: "repo".into(),
            },
            value: "deleted-branch".into(),
        })])];

        let result = check_all(&reminders, &mock, 1);
        assert!(result.reminders[0].triggered);
    }
}
