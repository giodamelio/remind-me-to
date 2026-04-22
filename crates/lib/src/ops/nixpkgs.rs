use serde::Deserialize;
use ureq::Agent;

use crate::errors::CheckError;
use crate::ops::types::NixpkgsBackend;

/// Nixpkgs client that queries the Devbox Search API (search.devbox.sh)
/// to look up package versions available in nixpkgs.
pub struct NixpkgsClient {
    agent: Agent,
    base_url: String,
}

impl Default for NixpkgsClient {
    fn default() -> Self {
        Self::new()
    }
}

impl NixpkgsClient {
    pub fn new() -> Self {
        let agent = Agent::new_with_defaults();
        Self {
            agent,
            base_url: "https://search.devbox.sh".to_string(),
        }
    }

    /// Create a client with a custom base URL (for testing with mock servers).
    pub fn new_with_base_url(base_url: &str) -> Self {
        let agent = Agent::new_with_defaults();
        Self {
            agent,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    fn map_ureq_error(&self, error: ureq::Error, path: &str) -> CheckError {
        match error {
            ureq::Error::StatusCode(status_code) => {
                if status_code == 429 {
                    CheckError::RateLimited {
                        forge: "nixhub".to_string(),
                        reset_at: "unknown".to_string(),
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
}

impl NixpkgsBackend for NixpkgsClient {
    fn get_package_versions(&self, package: &str) -> Result<Vec<String>, CheckError> {
        let url = format!("{}/v1/search?q={}", self.base_url, package);

        let mut response = self
            .agent
            .get(&url)
            .header("Accept", "application/json")
            .header("User-Agent", "remind-me-to")
            .call()
            .map_err(|e| self.map_ureq_error(e, &url))?;

        let search_results: SearchResults =
            response
                .body_mut()
                .read_json()
                .map_err(|e| CheckError::Network {
                    message: format!("failed to parse nixhub search response: {e}"),
                })?;

        // Find the exact package match
        for pkg in &search_results.packages {
            if pkg.name == package {
                let versions: Vec<String> = pkg
                    .versions
                    .iter()
                    .filter_map(|v| v.version.clone())
                    .collect();
                return Ok(versions);
            }
        }

        // Package not found in results
        Ok(vec![])
    }
}

// ---- Nixhub API response types (private) ----

#[derive(Debug, Deserialize)]
struct SearchResults {
    #[serde(default)]
    packages: Vec<SearchPackage>,
}

#[derive(Debug, Deserialize)]
struct SearchPackage {
    name: String,
    #[serde(default)]
    versions: Vec<SearchVersion>,
}

#[derive(Debug, Deserialize)]
struct SearchVersion {
    #[serde(default)]
    version: Option<String>,
}

// ---- Mock client for testing ----

pub mod mock {
    use std::collections::HashMap;
    use std::sync::{RwLock, atomic::AtomicUsize, atomic::Ordering};

    use crate::errors::CheckError;
    use crate::ops::types::NixpkgsBackend;

    /// A mock NixpkgsBackend for unit testing.
    pub struct MockNixpkgsClient {
        pub version_responses: HashMap<String, Result<Vec<String>, CheckError>>,
        call_counts: RwLock<HashMap<String, AtomicUsize>>,
    }

    impl Default for MockNixpkgsClient {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockNixpkgsClient {
        pub fn new() -> Self {
            Self {
                version_responses: HashMap::new(),
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

    impl NixpkgsBackend for MockNixpkgsClient {
        fn get_package_versions(&self, package: &str) -> Result<Vec<String>, CheckError> {
            self.record_call("get_package_versions");
            self.version_responses
                .get(package)
                .cloned()
                .unwrap_or(Ok(vec![]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mock::MockNixpkgsClient;
    use crate::ops::types::NixpkgsBackend;

    #[test]
    fn mock_returns_versions() {
        let mut mock = MockNixpkgsClient::new();
        mock.version_responses.insert(
            "redis".into(),
            Ok(vec!["7.2.4".into(), "7.0.12".into(), "6.2.6".into()]),
        );

        let versions = mock.get_package_versions("redis").unwrap();
        assert_eq!(versions.len(), 3);
        assert_eq!(versions[0], "7.2.4");
        assert_eq!(mock.call_count("get_package_versions"), 1);
    }

    #[test]
    fn mock_unknown_package_returns_empty() {
        let mock = MockNixpkgsClient::new();
        let versions = mock.get_package_versions("nonexistent").unwrap();
        assert!(versions.is_empty());
    }

    #[test]
    fn mock_call_counting() {
        let mock = MockNixpkgsClient::new();
        let _ = mock.get_package_versions("a");
        let _ = mock.get_package_versions("b");
        let _ = mock.get_package_versions("a");
        assert_eq!(mock.call_count("get_package_versions"), 3);
    }
}
