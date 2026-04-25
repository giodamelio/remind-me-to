use std::path::Path;
use std::process::Command;

/// Detected forge context from git remotes.
#[derive(Debug, Clone)]
pub struct GitContext {
    pub forge: String,
    pub owner: String,
    pub repo: String,
}

/// Detect the git context (forge, owner, repo) for a given path.
///
/// Strategy:
/// 1. Find the git root for the path
/// 2. List all remotes
/// 3. Pick the best remote: prefer "upstream", then "origin", then any single remote
/// 4. Parse the remote URL to extract forge, owner, repo
pub fn detect_git_context(path: &Path) -> Option<GitContext> {
    let git_root = find_git_root(path)?;
    let remotes = list_remotes(&git_root)?;

    if remotes.is_empty() {
        return None;
    }

    // Pick the best remote
    let remote_name = if remotes.len() == 1 {
        remotes[0].clone()
    } else if remotes.contains(&"upstream".to_string()) {
        "upstream".to_string()
    } else if remotes.contains(&"origin".to_string()) {
        "origin".to_string()
    } else {
        remotes[0].clone()
    };

    let url = get_remote_url(&git_root, &remote_name)?;
    parse_remote_url(&url)
}

/// Find the git root directory for a given path.
fn find_git_root(path: &Path) -> Option<std::path::PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(if path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        })
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let root = String::from_utf8(output.stdout).ok()?.trim().to_string();
    Some(std::path::PathBuf::from(root))
}

/// List all git remotes.
fn list_remotes(git_root: &Path) -> Option<Vec<String>> {
    let output = Command::new("git")
        .args(["remote"])
        .current_dir(git_root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    let remotes: Vec<String> = stdout.lines().map(|s| s.trim().to_string()).collect();
    Some(remotes)
}

/// Get the URL for a specific remote.
fn get_remote_url(git_root: &Path, remote: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", remote])
        .current_dir(git_root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8(output.stdout).ok()?.trim().to_string();
    Some(url)
}

/// Parse a git remote URL into forge, owner, repo.
///
/// Supported formats:
/// - `https://github.com/owner/repo.git`
/// - `https://github.com/owner/repo`
/// - `git@github.com:owner/repo.git`
/// - `git@github.com:owner/repo`
/// - `ssh://git@github.com/owner/repo.git`
pub fn parse_remote_url(url: &str) -> Option<GitContext> {
    // Try SSH format: git@github.com:owner/repo.git
    if let Some(rest) = url.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        let forge = detect_forge(host)?;
        let (owner, repo) = parse_owner_repo(path)?;
        return Some(GitContext { forge, owner, repo });
    }

    // Try SSH URL format: ssh://git@github.com/owner/repo.git
    if let Some(rest) = url.strip_prefix("ssh://git@") {
        let (host, path) = rest.split_once('/')?;
        let forge = detect_forge(host)?;
        let (owner, repo) = parse_owner_repo(path)?;
        return Some(GitContext { forge, owner, repo });
    }

    // Try HTTPS format: https://github.com/owner/repo.git
    if url.starts_with("https://") || url.starts_with("http://") {
        let without_scheme = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))?;
        let (host, path) = without_scheme.split_once('/')?;
        let forge = detect_forge(host)?;
        let (owner, repo) = parse_owner_repo(path)?;
        return Some(GitContext { forge, owner, repo });
    }

    None
}

/// Detect the forge type from a hostname.
fn detect_forge(host: &str) -> Option<String> {
    if host == "github.com" || host.starts_with("github.com:") {
        Some("github".to_string())
    } else {
        // For MVP, only github.com is recognized
        None
    }
}

/// Parse "owner/repo" or "owner/repo.git" from a path string.
fn parse_owner_repo(path: &str) -> Option<(String, String)> {
    let (owner, rest) = path.split_once('/')?;
    let repo = rest
        .strip_suffix(".git")
        .unwrap_or(rest)
        .trim_end_matches('/');

    if owner.is_empty() || repo.is_empty() {
        return None;
    }

    Some((owner.to_string(), repo.to_string()))
}

/// Resolve a shorthand reference like `#123` into a full forge reference.
///
/// Returns `Some("github:owner/repo#123")` if git context is available,
/// or `None` if shorthand can't be resolved.
pub fn resolve_shorthand(shorthand: &str, context: &Option<GitContext>) -> Option<String> {
    let context = context.as_ref()?;

    if let Some(number) = shorthand.strip_prefix('#') {
        Some(format!(
            "{}:{}/{}#{}",
            context.forge, context.owner, context.repo, number
        ))
    } else {
        shorthand.strip_prefix('@').map(|value| {
            format!(
                "{}:{}/{}@{}",
                context.forge, context.owner, context.repo, value
            )
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // ---- URL parsing ----

    #[test]
    fn parse_https_url() {
        let ctx = parse_remote_url("https://github.com/owner/repo.git").unwrap();
        assert_eq!(ctx.forge, "github");
        assert_eq!(ctx.owner, "owner");
        assert_eq!(ctx.repo, "repo");
    }

    #[test]
    fn parse_https_url_no_git_suffix() {
        let ctx = parse_remote_url("https://github.com/owner/repo").unwrap();
        assert_eq!(ctx.forge, "github");
        assert_eq!(ctx.owner, "owner");
        assert_eq!(ctx.repo, "repo");
    }

    #[test]
    fn parse_ssh_url() {
        let ctx = parse_remote_url("git@github.com:owner/repo.git").unwrap();
        assert_eq!(ctx.forge, "github");
        assert_eq!(ctx.owner, "owner");
        assert_eq!(ctx.repo, "repo");
    }

    #[test]
    fn parse_ssh_url_no_git_suffix() {
        let ctx = parse_remote_url("git@github.com:owner/repo").unwrap();
        assert_eq!(ctx.repo, "repo");
    }

    #[test]
    fn parse_ssh_protocol_url() {
        let ctx = parse_remote_url("ssh://git@github.com/owner/repo.git").unwrap();
        assert_eq!(ctx.forge, "github");
        assert_eq!(ctx.owner, "owner");
        assert_eq!(ctx.repo, "repo");
    }

    #[test]
    fn non_github_returns_none() {
        assert!(parse_remote_url("https://gitlab.com/owner/repo").is_none());
    }

    #[test]
    fn malformed_url_returns_none() {
        assert!(parse_remote_url("not-a-url").is_none());
    }

    // ---- Shorthand resolution ----

    #[test]
    fn resolve_issue_shorthand() {
        let ctx = Some(GitContext {
            forge: "github".to_string(),
            owner: "tokio-rs".to_string(),
            repo: "tokio".to_string(),
        });
        let resolved = resolve_shorthand("#5432", &ctx).unwrap();
        assert_eq!(resolved, "github:tokio-rs/tokio#5432");
    }

    #[test]
    fn resolve_ref_shorthand() {
        let ctx = Some(GitContext {
            forge: "github".to_string(),
            owner: "serde-rs".to_string(),
            repo: "serde".to_string(),
        });
        let resolved = resolve_shorthand("@>=2.0.0", &ctx).unwrap();
        assert_eq!(resolved, "github:serde-rs/serde@>=2.0.0");
    }

    #[test]
    fn resolve_without_context() {
        assert!(resolve_shorthand("#123", &None).is_none());
    }

    #[test]
    fn resolve_non_shorthand() {
        let ctx = Some(GitContext {
            forge: "github".to_string(),
            owner: "foo".to_string(),
            repo: "bar".to_string(),
        });
        assert!(resolve_shorthand("not-a-shorthand", &ctx).is_none());
    }

    // ---- Git context detection with temp repos ----

    #[test]
    fn detect_context_from_temp_repo() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path();

        // Init git repo
        Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .unwrap();

        // Add origin remote
        Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/test-owner/test-repo.git",
            ])
            .current_dir(path)
            .output()
            .unwrap();

        let ctx = detect_git_context(path).unwrap();
        assert_eq!(ctx.forge, "github");
        assert_eq!(ctx.owner, "test-owner");
        assert_eq!(ctx.repo, "test-repo");
    }

    #[test]
    fn detect_context_prefers_upstream() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path();

        Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .unwrap();

        Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/fork-owner/repo.git",
            ])
            .current_dir(path)
            .output()
            .unwrap();

        Command::new("git")
            .args([
                "remote",
                "add",
                "upstream",
                "https://github.com/upstream-owner/repo.git",
            ])
            .current_dir(path)
            .output()
            .unwrap();

        let ctx = detect_git_context(path).unwrap();
        assert_eq!(ctx.owner, "upstream-owner");
    }

    #[test]
    fn no_git_repo_returns_none() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(detect_git_context(dir.path()).is_none());
    }
}
