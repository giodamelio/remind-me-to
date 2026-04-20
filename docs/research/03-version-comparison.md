# Research: Version Comparison

## Context

`remind-me-to` has a `tag_exists` operation that checks if a release matching a version constraint exists. For example:

```
tag_exists=github:owner/repo@>=2.0.0
tag_exists=github:owner/repo@>1.3.2
tag_exists=github:owner/repo@^2.0
tag_exists=github:owner/repo@~1.5
```

The problem: not all projects follow semver. Real-world version schemes include:

- **Strict semver:** `1.2.3`, `1.2.3-beta.1`
- **Semver-ish with v prefix:** `v1.2.3`
- **CalVer (calendar versioning):** `2025.01.15`, `25.1`, `2025.1`
- **Four-segment:** `1.2.3.4` (e.g., Chrome, some Java projects)
- **Date + build:** `20250115.1`
- **Loose numeric:** `1.2`, `1.2.3.4.5`
- **Non-numeric suffixes:** `1.2.3-rc1`, `1.2.3.Final`

We need version comparison that:
1. Works correctly for strict semver projects (the common case)
2. Doesn't produce surprising results for non-semver projects
3. Has well-documented behavior so users can predict what will happen
4. Supports constraint syntax (>=, >, ^, ~, exact match)

## Candidates

### `semver` (standard crate)

- https://crates.io/crates/semver
- Strict semver 2.0 parsing
- Has `VersionReq` for constraints
- Rejects non-semver versions

### `lenient_semver`

- https://crates.io/crates/lenient_semver
- Parses semver-like strings leniently
- Handles `v` prefix, missing patch version, etc.

### `version-compare`

- https://crates.io/crates/version-compare
- Generic version comparison (not semver-specific)
- Compares numeric segments

### `semver-parser` / custom layered approach

- Use `semver` for strict parsing
- Fall back to numeric segment comparison for non-semver

### Others?

- `node-semver` (npm-style semver with more lenient parsing)
- `pep440` (Python versioning, probably too specific)
- Look at what Nix, Dependabot, Renovate use

## Evaluation Criteria

1. **Semver compliance** — does it correctly implement semver 2.0 for projects that follow it?
2. **Leniency** — how well does it handle non-standard versions?
3. **Constraint syntax** — does it support `>=`, `>`, `<`, `<=`, `^`, `~`, `=`?
4. **Predictability** — can a user predict what `>=1.2.0` means against a set of tags?
5. **Tag prefix handling** — does it strip `v` from `v1.2.3`?
6. **Non-version tags** — what happens with tags like `nightly`, `latest`, `rc1`? (should be skipped)
7. **Sorting** — can we sort a list of versions correctly?
8. **Maintenance** — actively maintained?

## Questions to Answer

1. How does Renovate/Dependabot handle version comparison for non-semver projects? Is there a documented strategy we can borrow?
2. Can we use `semver` for constraint parsing but a more lenient parser for matching against actual tags?
3. What's a sane behavior when a tag doesn't parse as any known version scheme? (skip it with a debug log?)
4. Should we support CalVer constraints? Or just treat CalVer as "numeric segments compared left to right"?
5. Is there a crate that already does "try semver, fall back to numeric segment comparison"?
6. How do we handle pre-release versions? (semver says `1.2.3-alpha < 1.2.3`, but some projects use `-rc1` as a suffix)
7. What constraint operators make sense for non-semver? (probably just `>`, `>=`, `<`, `<=`, `=` — no `^` or `~`)

## Findings

<!-- To be filled in by research agent -->

## Recommendation

<!-- To be filled in by research agent -->
