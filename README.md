# remind-me-to

```
// REMIND-ME-TO: Remove workaround pr_merged=github:tokio-rs/tokio#5432
// REMIND-ME-TO: Clean up compat shim pr_closed=github:owner/repo#99
// REMIND-ME-TO: Upgrade to v2 tag_exists=github:serde-rs/serde@>=2.0.0
// REMIND-ME-TO: Remove hack commit_released=github:foo/bar@abc1234
// REMIND-ME-TO: Update dependency pr_released=github:foo/bar#42
// REMIND-ME-TO: Remove workaround issue_closed=github:foo/bar#456
// REMIND-ME-TO: Delete migration branch_deleted=github:foo/bar@feature-branch
// REMIND-ME-TO: Review this decision date_passed=2025-06-01
// REMIND-ME-TO: Remove workaround nixpkg_version=redis@>=8.0.0
```

## GitHub Action

Add `remind-me-to` to your CI pipeline to automatically check if any reminders have been triggered:

```yaml
- uses: giodamelio/remind-me-to@v0.1.0
  with:
    github-token: ${{ secrets.GITHUB_TOKEN }}
```

### Inputs

| Input | Description | Default |
|-------|-------------|---------|
| `paths` | Paths to scan (space-separated) | `.` |
| `github-token` | GitHub token for API requests | |
| `format` | Output format: `text`, `json`, `llm` | `text` |
| `dry-run` | Parse without checking external services | `false` |
| `ignore` | Additional ignore patterns (newline-separated) | |
| `extra-args` | Additional CLI arguments | |
| `version` | Release version to use | `latest` |
