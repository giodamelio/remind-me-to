use http::Response;
use serde::Deserialize;
use ureq::{Agent, Body};

use crate::errors::CheckError;
use crate::ops::types::{ForgeClient, IssueState, IssueStatus, PrState, PrStatus, Release, Tag};

/// GitHub API client using ureq.
pub struct GitHubClient {
    agent: Agent,
    token: Option<String>,
    base_url: String,
}

impl GitHubClient {
    /// Create a new GitHub client with an optional auth token.
    pub fn new(token: Option<String>) -> Self {
        let agent = Agent::new_with_defaults();
        Self {
            agent,
            token,
            base_url: "https://api.github.com".to_string(),
        }
    }

    /// Create a client with a custom base URL (for testing with mock servers).
    pub fn new_with_base_url(base_url: &str, token: Option<String>) -> Self {
        let agent = Agent::new_with_defaults();
        Self {
            agent,
            token,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Make a GET request to the GitHub API.
    fn get(&self, path: &str) -> Result<Response<Body>, CheckError> {
        let url = format!("{}{}", self.base_url, path);

        let mut req = self
            .agent
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "remind-me-to");

        if let Some(ref token) = self.token {
            req = req.header("Authorization", &format!("Bearer {token}"));
        }

        let response = req.call().map_err(|e| self.map_ureq_error(e, path))?;

        // Check rate limit headers
        self.check_rate_limit_headers(&response);

        Ok(response)
    }

    /// Make a GET request with retry on 429.
    fn get_with_retry(&self, path: &str) -> Result<Response<Body>, CheckError> {
        match self.get(path) {
            Ok(resp) => Ok(resp),
            Err(CheckError::RateLimited { forge, reset_at }) => {
                if let Ok(secs) = reset_at.parse::<u64>() {
                    let wait = secs.min(30);
                    tracing::info!("rate limited, waiting {}s before retry", wait);
                    std::thread::sleep(std::time::Duration::from_secs(wait));
                }
                self.get(path)
                    .map_err(|_| CheckError::RateLimited { forge, reset_at })
            }
            Err(e) => Err(e),
        }
    }

    /// Map a ureq error to our CheckError type.
    fn map_ureq_error(&self, error: ureq::Error, path: &str) -> CheckError {
        match error {
            ureq::Error::StatusCode(status_code) => {
                if status_code == 429 {
                    CheckError::RateLimited {
                        forge: "github".to_string(),
                        reset_at: "unknown".to_string(),
                    }
                } else if status_code == 401 || status_code == 403 {
                    CheckError::AuthRequired {
                        forge: "github".to_string(),
                    }
                } else {
                    CheckError::ApiError {
                        operation: path.to_string(),
                        message: format!("HTTP {status_code}"),
                        status: Some(status_code),
                        retryable: status_code >= 500,
                    }
                }
            }
            _ => CheckError::Network {
                message: error.to_string(),
            },
        }
    }

    /// Check rate limit headers on a response and log warnings.
    fn check_rate_limit_headers(&self, response: &Response<Body>) {
        if let Some(remaining) = response.headers().get("X-RateLimit-Remaining")
            && let Ok(remaining_str) = remaining.to_str()
            && let Ok(n) = remaining_str.parse::<u64>()
        {
            if n == 0 {
                let reset = response
                    .headers()
                    .get("X-RateLimit-Reset")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("unknown");
                tracing::warn!("GitHub rate limit exhausted, resets at {}", reset);
            } else if n < 10 {
                tracing::debug!("GitHub rate limit remaining: {}", n);
            }
        }
    }

    /// Fetch all tags with pagination.
    fn fetch_all_tags(&self, owner: &str, repo: &str) -> Result<Vec<Tag>, CheckError> {
        let mut all_tags = Vec::new();
        let mut page = 1;

        loop {
            let path = format!("/repos/{owner}/{repo}/tags?per_page=100&page={page}");
            let mut response = self.get_with_retry(&path)?;
            let github_tags: Vec<GitHubTag> =
                response
                    .body_mut()
                    .read_json()
                    .map_err(|e| CheckError::Network {
                        message: format!("failed to parse tags response: {e}"),
                    })?;

            if github_tags.is_empty() {
                break;
            }

            for gt in &github_tags {
                all_tags.push(Tag {
                    name: gt.name.clone(),
                    commit_sha: gt.commit.sha.clone(),
                });
            }

            if github_tags.len() < 100 {
                break;
            }

            page += 1;
        }

        Ok(all_tags)
    }
}

impl ForgeClient for GitHubClient {
    fn get_pr_status(&self, owner: &str, repo: &str, number: u64) -> Result<PrStatus, CheckError> {
        let path = format!("/repos/{owner}/{repo}/pulls/{number}");
        let mut response = self.get_with_retry(&path)?;
        let pr: GitHubPr = response
            .body_mut()
            .read_json()
            .map_err(|e| CheckError::Network {
                message: format!("failed to parse PR response: {e}"),
            })?;

        Ok(PrStatus {
            number: pr.number,
            state: if pr.state == "open" {
                PrState::Open
            } else {
                PrState::Closed
            },
            merged: pr.merged.unwrap_or(false),
            merged_at: pr.merged_at,
            merge_commit_sha: pr.merge_commit_sha,
        })
    }

    fn get_tags(&self, owner: &str, repo: &str) -> Result<Vec<Tag>, CheckError> {
        self.fetch_all_tags(owner, repo)
    }

    fn get_issue_status(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<IssueStatus, CheckError> {
        let path = format!("/repos/{owner}/{repo}/issues/{number}");
        let mut response = self.get_with_retry(&path)?;
        let issue: GitHubIssue =
            response
                .body_mut()
                .read_json()
                .map_err(|e| CheckError::Network {
                    message: format!("failed to parse issue response: {e}"),
                })?;

        Ok(IssueStatus {
            number: issue.number,
            state: if issue.state == "closed" {
                IssueState::Closed
            } else {
                IssueState::Open
            },
        })
    }

    fn branch_exists(&self, owner: &str, repo: &str, branch: &str) -> Result<bool, CheckError> {
        let path = format!("/repos/{owner}/{repo}/branches/{branch}");
        match self.get_with_retry(&path) {
            Ok(_) => Ok(true),
            Err(CheckError::ApiError {
                status: Some(404), ..
            }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn get_commit_releases(
        &self,
        owner: &str,
        repo: &str,
        _sha: &str,
    ) -> Result<Vec<Release>, CheckError> {
        let path = format!("/repos/{owner}/{repo}/releases?per_page=100");
        let mut response = self.get_with_retry(&path)?;
        let releases: Vec<GitHubRelease> =
            response
                .body_mut()
                .read_json()
                .map_err(|e| CheckError::Network {
                    message: format!("failed to parse releases response: {e}"),
                })?;

        Ok(releases
            .into_iter()
            .map(|r| Release {
                tag_name: r.tag_name,
                target_commitish: r.target_commitish,
            })
            .collect())
    }
}

// ---- GitHub API response types (private) ----

#[derive(Debug, Deserialize)]
struct GitHubPr {
    number: u64,
    state: String,
    merged: Option<bool>,
    merged_at: Option<String>,
    merge_commit_sha: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubIssue {
    number: u64,
    state: String,
}

#[derive(Debug, Deserialize)]
struct GitHubTag {
    name: String,
    commit: GitHubTagCommit,
}

#[derive(Debug, Deserialize)]
struct GitHubTagCommit {
    sha: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    target_commitish: String,
}

// ---- Mock client for testing ----

pub mod mock {
    use std::collections::HashMap;
    use std::sync::{RwLock, atomic::AtomicUsize, atomic::Ordering};

    use crate::errors::CheckError;
    use crate::ops::types::*;

    /// A mock ForgeClient for unit testing.
    pub struct MockForgeClient {
        pub pr_responses: HashMap<(String, String, u64), Result<PrStatus, CheckError>>,
        pub tag_responses: HashMap<(String, String), Result<Vec<Tag>, CheckError>>,
        pub issue_responses: HashMap<(String, String, u64), Result<IssueStatus, CheckError>>,
        pub branch_responses: HashMap<(String, String, String), Result<bool, CheckError>>,
        pub release_responses: HashMap<(String, String), Result<Vec<Release>, CheckError>>,
        call_counts: RwLock<HashMap<String, AtomicUsize>>,
    }

    impl Default for MockForgeClient {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockForgeClient {
        pub fn new() -> Self {
            Self {
                pr_responses: HashMap::new(),
                tag_responses: HashMap::new(),
                issue_responses: HashMap::new(),
                branch_responses: HashMap::new(),
                release_responses: HashMap::new(),
                call_counts: RwLock::new(HashMap::new()),
            }
        }

        fn record_call(&self, method: &str) {
            let counts = self.call_counts.read().unwrap();
            if let Some(counter) = counts.get(method) {
                counter.fetch_add(1, Ordering::Relaxed);
                return;
            }
            drop(counts);
            let mut counts = self.call_counts.write().unwrap();
            counts
                .entry(method.to_string())
                .or_insert_with(|| AtomicUsize::new(0))
                .fetch_add(1, Ordering::Relaxed);
        }

        pub fn call_count(&self, method: &str) -> usize {
            let counts = self.call_counts.read().unwrap();
            counts
                .get(method)
                .map(|c| c.load(Ordering::Relaxed))
                .unwrap_or(0)
        }
    }

    impl ForgeClient for MockForgeClient {
        fn get_pr_status(
            &self,
            owner: &str,
            repo: &str,
            number: u64,
        ) -> Result<PrStatus, CheckError> {
            self.record_call("get_pr_status");
            self.pr_responses
                .get(&(owner.into(), repo.into(), number))
                .cloned()
                .unwrap_or(Err(CheckError::ApiError {
                    operation: format!("github:{owner}/{repo}#{number}"),
                    message: "not found in mock".into(),
                    status: Some(404),
                    retryable: false,
                }))
        }

        fn get_tags(&self, owner: &str, repo: &str) -> Result<Vec<Tag>, CheckError> {
            self.record_call("get_tags");
            self.tag_responses
                .get(&(owner.into(), repo.into()))
                .cloned()
                .unwrap_or(Ok(vec![]))
        }

        fn get_issue_status(
            &self,
            owner: &str,
            repo: &str,
            number: u64,
        ) -> Result<IssueStatus, CheckError> {
            self.record_call("get_issue_status");
            self.issue_responses
                .get(&(owner.into(), repo.into(), number))
                .cloned()
                .unwrap_or(Err(CheckError::ApiError {
                    operation: format!("github:{owner}/{repo}#{number}"),
                    message: "not found in mock".into(),
                    status: Some(404),
                    retryable: false,
                }))
        }

        fn branch_exists(&self, owner: &str, repo: &str, branch: &str) -> Result<bool, CheckError> {
            self.record_call("branch_exists");
            self.branch_responses
                .get(&(owner.into(), repo.into(), branch.into()))
                .cloned()
                .unwrap_or(Ok(false))
        }

        fn get_commit_releases(
            &self,
            owner: &str,
            repo: &str,
            _sha: &str,
        ) -> Result<Vec<Release>, CheckError> {
            self.record_call("get_commit_releases");
            self.release_responses
                .get(&(owner.into(), repo.into()))
                .cloned()
                .unwrap_or(Ok(vec![]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mock::MockForgeClient;
    use crate::ops::types::*;

    #[test]
    fn mock_pr_status_merged() {
        let mut mock = MockForgeClient::new();
        mock.pr_responses.insert(
            ("tokio-rs".into(), "tokio".into(), 5432),
            Ok(PrStatus {
                number: 5432,
                state: PrState::Closed,
                merged: true,
                merged_at: Some("2025-01-15T00:00:00Z".into()),
                merge_commit_sha: Some("abc123".into()),
            }),
        );

        let result = mock.get_pr_status("tokio-rs", "tokio", 5432).unwrap();
        assert!(result.merged);
        assert_eq!(result.state, PrState::Closed);
        assert_eq!(mock.call_count("get_pr_status"), 1);
    }

    #[test]
    fn mock_pr_not_found() {
        let mock = MockForgeClient::new();
        let result = mock.get_pr_status("foo", "bar", 999);
        assert!(result.is_err());
    }

    #[test]
    fn mock_tags() {
        let mut mock = MockForgeClient::new();
        mock.tag_responses.insert(
            ("owner".into(), "repo".into()),
            Ok(vec![
                Tag {
                    name: "v1.0.0".into(),
                    commit_sha: "aaa".into(),
                },
                Tag {
                    name: "v2.0.0".into(),
                    commit_sha: "bbb".into(),
                },
            ]),
        );

        let tags = mock.get_tags("owner", "repo").unwrap();
        assert_eq!(tags.len(), 2);
    }

    #[test]
    fn mock_issue_closed() {
        let mut mock = MockForgeClient::new();
        mock.issue_responses.insert(
            ("owner".into(), "repo".into(), 42),
            Ok(IssueStatus {
                number: 42,
                state: IssueState::Closed,
            }),
        );

        let result = mock.get_issue_status("owner", "repo", 42).unwrap();
        assert_eq!(result.state, IssueState::Closed);
    }

    #[test]
    fn mock_branch_exists() {
        let mut mock = MockForgeClient::new();
        mock.branch_responses
            .insert(("owner".into(), "repo".into(), "main".into()), Ok(true));

        assert!(mock.branch_exists("owner", "repo", "main").unwrap());
        assert!(
            !mock
                .branch_exists("owner", "repo", "deleted-branch")
                .unwrap()
        );
    }

    #[test]
    fn mock_call_counting() {
        let mock = MockForgeClient::new();
        let _ = mock.get_tags("a", "b");
        let _ = mock.get_tags("a", "b");
        let _ = mock.get_tags("c", "d");
        assert_eq!(mock.call_count("get_tags"), 3);
    }
}
