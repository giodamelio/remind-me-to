use std::path::PathBuf;

/// Result of scanning files — contains both successes and errors.
/// A parse error in one file does not stop the scan.
pub struct ScanResult {
    pub reminders: Vec<crate::ops::types::Reminder>,
    pub errors: Vec<ScanError>,
}

/// Non-fatal errors during file scanning/parsing
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("parse error in {file}:{line}: {message}")]
    Parse {
        file: PathBuf,
        line: usize,
        col: usize,
        message: String,
        span: std::ops::Range<usize>,
        source_line: String,
        expected: Vec<String>,
        found: Option<String>,
    },

    #[error("failed to read {path}: {source}")]
    FileRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to walk directory {path}: {message}")]
    Walk { path: PathBuf, message: String },
}

/// Non-fatal errors during operation checking (per-operation, doesn't stop run)
#[derive(Debug, Clone, thiserror::Error)]
pub enum CheckError {
    #[error("API request failed for {operation}: {message}")]
    ApiError {
        operation: String,
        message: String,
        status: Option<u16>,
        retryable: bool,
    },

    #[error("rate limited by {forge}, resets at {reset_at}")]
    RateLimited { forge: String, reset_at: String },

    #[error("authentication required for {forge}")]
    AuthRequired { forge: String },

    #[error("network error: {message}")]
    Network { message: String },
}

/// Fatal errors that prevent the tool from running at all
#[derive(Debug, thiserror::Error)]
pub enum FatalError {
    #[error("configuration error: {message}")]
    Config { message: String },

    #[error("no files to scan")]
    NoInput,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn scan_error_parse_display() {
        let err = ScanError::Parse {
            file: PathBuf::from("src/main.rs"),
            line: 42,
            col: 10,
            message: "unexpected token".to_string(),
            span: 100..115,
            source_line: "// REMIND-ME-TO: fix this pr_merged=bad".to_string(),
            expected: vec!["forge reference".to_string()],
            found: Some("bad".to_string()),
        };
        assert_eq!(
            err.to_string(),
            "parse error in src/main.rs:42: unexpected token"
        );
    }

    #[test]
    fn scan_error_file_read_display() {
        let err = ScanError::FileRead {
            path: PathBuf::from("/tmp/missing.rs"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
        };
        assert!(err.to_string().contains("failed to read /tmp/missing.rs"));
    }

    #[test]
    fn scan_error_walk_display() {
        let err = ScanError::Walk {
            path: PathBuf::from("/tmp"),
            message: "permission denied".to_string(),
        };
        assert!(err.to_string().contains("failed to walk directory /tmp"));
    }

    #[test]
    fn check_error_api_display() {
        let err = CheckError::ApiError {
            operation: "pr_merged=github:foo/bar#1".to_string(),
            message: "not found".to_string(),
            status: Some(404),
            retryable: false,
        };
        assert!(err.to_string().contains("API request failed"));
    }

    #[test]
    fn check_error_rate_limited_display() {
        let err = CheckError::RateLimited {
            forge: "github".to_string(),
            reset_at: "2025-06-01T00:00:00Z".to_string(),
        };
        assert!(err.to_string().contains("rate limited by github"));
    }

    #[test]
    fn check_error_auth_display() {
        let err = CheckError::AuthRequired {
            forge: "github".to_string(),
        };
        assert!(err.to_string().contains("authentication required"));
    }

    #[test]
    fn check_error_network_display() {
        let err = CheckError::Network {
            message: "connection refused".to_string(),
        };
        assert!(err.to_string().contains("network error"));
    }

    #[test]
    fn fatal_error_config_display() {
        let err = FatalError::Config {
            message: "missing token".to_string(),
        };
        assert!(err.to_string().contains("configuration error"));
    }

    #[test]
    fn fatal_error_no_input_display() {
        let err = FatalError::NoInput;
        assert_eq!(err.to_string(), "no files to scan");
    }
}
