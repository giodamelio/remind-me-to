use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A forge reference like `github:owner/repo`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ForgeRef {
    pub forge: String,
    pub owner: String,
    pub repo: String,
}

impl std::fmt::Display for ForgeRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}/{}", self.forge, self.owner, self.repo)
    }
}

/// An issue-like reference (PRs, issues): `github:owner/repo#123`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IssueRef {
    pub forge_ref: ForgeRef,
    pub number: u64,
}

impl std::fmt::Display for IssueRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}", self.forge_ref, self.number)
    }
}

/// A ref-like reference (versions, branches, commits): `github:owner/repo@value`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RefRef {
    pub forge_ref: ForgeRef,
    pub value: String,
}

impl std::fmt::Display for RefRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.forge_ref, self.value)
    }
}

/// A parsed operation from a REMIND-ME-TO comment
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Operation {
    PrMerged(IssueRef),
    PrClosed(IssueRef),
    TagExists(RefRef),
    CommitReleased(RefRef),
    PrReleased(IssueRef),
    IssueClosed(IssueRef),
    BranchDeleted(RefRef),
    DatePassed(String),
}

impl Operation {
    /// Returns the operation name as used in comment syntax
    pub fn name(&self) -> &'static str {
        match self {
            Operation::PrMerged(_) => "pr_merged",
            Operation::PrClosed(_) => "pr_closed",
            Operation::TagExists(_) => "tag_exists",
            Operation::CommitReleased(_) => "commit_released",
            Operation::PrReleased(_) => "pr_released",
            Operation::IssueClosed(_) => "issue_closed",
            Operation::BranchDeleted(_) => "branch_deleted",
            Operation::DatePassed(_) => "date_passed",
        }
    }
}

impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Operation::PrMerged(r) => write!(f, "pr_merged={r}"),
            Operation::PrClosed(r) => write!(f, "pr_closed={r}"),
            Operation::TagExists(r) => write!(f, "tag_exists={r}"),
            Operation::CommitReleased(r) => write!(f, "commit_released={r}"),
            Operation::PrReleased(r) => write!(f, "pr_released={r}"),
            Operation::IssueClosed(r) => write!(f, "issue_closed={r}"),
            Operation::BranchDeleted(r) => write!(f, "branch_deleted={r}"),
            Operation::DatePassed(d) => write!(f, "date_passed={d}"),
        }
    }
}

/// A parsed reminder from a source file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reminder {
    pub file: PathBuf,
    pub line: usize,
    pub description: String,
    pub operations: Vec<Operation>,
}

/// The result of checking a single operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResult {
    pub operation: Operation,
    pub status: OperationStatus,
    pub detail: Option<String>,
}

/// Status of an individual operation check
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationStatus {
    /// The condition is met (e.g., PR is merged)
    Triggered,
    /// The condition is not yet met
    Pending,
    /// Could not check (error occurred)
    Error,
}

/// The result of checking all operations for a reminder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckedReminder {
    pub reminder: Reminder,
    pub results: Vec<OperationResult>,
    /// Whether the reminder is triggered (any operation triggered = true, OR semantics)
    pub triggered: bool,
}

/// Overall result of the check phase
#[derive(Debug, Serialize, Deserialize)]
pub struct CheckResult {
    pub reminders: Vec<CheckedReminder>,
    pub errors: Vec<String>,
}

/// Status of a PR from the forge API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrStatus {
    pub number: u64,
    pub state: PrState,
    pub merged: bool,
    pub merged_at: Option<String>,
    pub merge_commit_sha: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrState {
    Open,
    Closed,
}

/// Status of an issue from the forge API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueStatus {
    pub number: u64,
    pub state: IssueState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueState {
    Open,
    Closed,
}

/// A tag from the forge API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub commit_sha: String,
}

/// A release from the forge API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub target_commitish: String,
}

/// Result of comparing two commits
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AncestorStatus {
    /// The commit is an ancestor of (or identical to) the target
    Ancestor,
    /// The commit is NOT an ancestor of the target
    NotAncestor,
}

/// The ForgeClient trait — injectable for testing
pub trait ForgeClient: Send + Sync {
    fn get_pr_status(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<PrStatus, crate::errors::CheckError>;

    fn get_tags(&self, owner: &str, repo: &str) -> Result<Vec<Tag>, crate::errors::CheckError>;

    fn get_issue_status(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<IssueStatus, crate::errors::CheckError>;

    fn branch_exists(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
    ) -> Result<bool, crate::errors::CheckError>;

    fn get_latest_release(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Option<Release>, crate::errors::CheckError>;

    fn is_ancestor(
        &self,
        owner: &str,
        repo: &str,
        commit: &str,
        of: &str,
    ) -> Result<AncestorStatus, crate::errors::CheckError>;
}
