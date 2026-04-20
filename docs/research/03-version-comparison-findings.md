# Findings: Version Comparison

## Summary

Use the `versions` crate as the primary solution. It provides a `Versioning` enum that auto-detects version schemes (strict SemVer, structured versions, and chaotic "Mess" formats), supports constraint matching via `Requirement`, and gracefully handles non-semver tags. Supplement with custom `v` prefix stripping and a "skip with debug log" policy for completely unparseable tags.

## Candidate Analysis

### `semver`

- **Crate:** https://crates.io/crates/semver
- **What it does:** Strict semver 2.0 parsing with `Version` and `VersionReq` types. Implements Cargo's interpretation of semver.
- **Constraint operators:** `=`, `>`, `>=`, `<`, `<=`, `~`, `^`, `*`
- **Strengths:**
  - De facto standard in Rust ecosystem
  - Correct pre-release handling (1.2.3-alpha < 1.2.3)
  - Well-maintained (by dtolnay)
  - `VersionReq` gives full constraint parsing
- **Weaknesses:**
  - Rejects anything that isn't strict semver (no `v` prefix, no 4-segment, no CalVer)
  - Cannot parse `1.2` (requires all three segments)
  - Not suitable as a standalone solution for our use case
- **Verdict:** Excellent for projects that follow semver, but too strict to be our only parser.

### `lenient_semver`

- **Crate:** https://crates.io/crates/lenient_semver (v0.4.2)
- **What it does:** Lenient parser that coerces non-standard strings into `semver::Version`.
- **Leniency features:**
  - `v` prefix allowed (`v1.2.3` -> `1.2.3`)
  - Minor/patch optional (`1` -> `1.0.0`, `1.2` -> `1.2.0`)
  - Extra segments become build metadata (`1.2.3.4.5` -> `1.2.3+4.5`)
  - Dot-separated pre-release (`1.2.3.rc1` -> `1.2.3-rc1`)
  - Recognized suffixes like `.Final` become build metadata
- **Strengths:**
  - Outputs `semver::Version`, so compatible with `semver::VersionReq`
  - Handles many real-world tag formats
- **Weaknesses:**
  - Lossy: extra segments (4th, 5th) become build metadata and are **ignored in comparison** (semver spec says build metadata has no precedence)
  - `1.2.3.4` and `1.2.3.5` would compare as equal (both become `1.2.3+...`)
  - This is a dealbreaker for 4-segment versions (Chrome, Java projects)
  - CalVer like `2025.01.15` becomes `2025.1.15` which works for comparison but `^` and `~` semantics are meaningless
- **Verdict:** Good bridge between strict semver and lenient parsing, but the lossy 4-segment handling is problematic.

### `version-compare`

- **Crate:** https://crates.io/crates/version-compare
- **What it does:** Generic "best-effort" version string comparison. Splits on dots/separators and compares segments left to right.
- **Supported formats:** `1`, `3.10.4.1`, `1.2.alpha`, `1.2.dev.4`, arbitrary segments
- **API:** `compare()` returns ordering, `compare_to()` tests against an operator (`Eq`, `Lt`, `Gt`, etc.)
- **Strengths:**
  - Handles any number of segments
  - Numeric segments compared numerically
  - Alpha segments compared lexicographically
  - Very permissive
- **Weaknesses:**
  - No constraint/range syntax (no `^`, `~`, `>=`)
  - Only basic comparison operators
  - No semver-aware pre-release handling
  - "Best effort" means behavior can be surprising
  - Would need to build our own constraint system on top
- **Verdict:** Useful as a fallback comparison engine but lacks constraint infrastructure.

### `node-semver`

- **Crate:** https://crates.io/crates/node-semver (also `nodejs-semver` fork)
- **What it does:** Rust implementation of npm's node-semver, including full range syntax.
- **Range syntax:** `^1.2.3`, `~1.2.3`, `>=1.0.0 <2.0.0`, `1.x`, `*`, `||` unions
- **Strengths:**
  - Rich range/constraint syntax (richer than Cargo's semver)
  - Handles `v` prefix
  - Well-documented behavior (matches npm exactly)
  - Pre-release handling matches npm rules
- **Weaknesses:**
  - Still fundamentally semver-based (3 segments)
  - Non-semver strings fail to parse
  - Designed for JS ecosystem compatibility, not generic version handling
  - Slightly heavier API surface than needed
- **Verdict:** Good if we wanted npm-compatible ranges, but doesn't solve the non-semver problem.

### `versions` (fosskers/versions)

- **Crate:** https://crates.io/crates/versions
- **What it does:** Multi-strategy version parser with three tiers:
  1. `SemVer` - strict semantic versions
  2. `Version` - structured numeric versions (any number of segments)
  3. `Mess` - chaotic/unstructured formats (e.g., `2:10.2+0.0093r3+1-1`)
- **Auto-detection:** `Versioning::new()` tries SemVer first, then Version, then Mess
- **Constraint support:** `Requirement` type with `Op` enum supporting `>=`, `>`, `<`, `<=`, `=`, `^`, `~`, `*`
- **Cross-type comparison:** Can compare a SemVer against a Mess and get meaningful results
- **Strengths:**
  - Handles virtually any version string
  - Graceful degradation (strict -> structured -> chaotic)
  - Built-in constraint matching via `Requirement::matches()`
  - nom parser combinators for embedding in custom parsers
  - Serde support
  - Actively maintained
- **Weaknesses:**
  - `^` and `~` semantics for non-semver versions may be surprising
  - Less ecosystem adoption than `semver`
  - Constraint system is simpler than npm-style ranges (no `||` unions, no `x` ranges)
- **Verdict:** Best fit for our use case. Handles the full spectrum of version formats with built-in constraint matching.

### Others

- **`compare-version`** - Simple semver comparison, no advantages over `semver`
- **`pep440`** - Python-specific, not relevant
- **`semver_rs`** - Another node-semver port, less maintained

## How Other Tools Handle This

### Renovate

Renovate's approach is the most relevant to our problem:

- **51+ versioning modules**, each implementing a common interface
- **Default: `semver-coerced`** - finds the first digit in a string, extracts up to 3 segments, pads missing segments with 0. E.g., `v3.4` -> `3.4.0`, `4.6.3.9.2-alpha2` -> `4.6.3` (truncates!)
- **Explicit configuration:** Users specify versioning per package via `packageRules`
- **Key insight:** Renovate does NOT try to be smart about auto-detecting schemes for constraint semantics. It either coerces to semver (lossy) or uses an explicitly configured scheme.
- **CalVer:** No native CalVer constraint semantics; uses `regex` versioning module for custom patterns
- **Lesson for us:** A "coerce to semver" default with documented lossy behavior is acceptable to users if the behavior is predictable and documented.

### Dependabot

- Less flexible than Renovate
- Uses ecosystem-native versioning (npm semver for JS, PEP 440 for Python, etc.)
- For non-semver: `versioning-strategy` config controls update behavior but not comparison
- **Known gap:** Documentation doesn't fully specify behavior when `ignore: update-types: semver-major` is used with non-semver versions
- **Lesson for us:** Ecosystem-native parsing is ideal but we're cross-ecosystem, so we need a generic approach.

### Nix

- Uses its own version comparison: splits on `.` and `-`, compares segments lexicographically with special numeric handling
- Pre-release suffixes like `pre`, `alpha`, `beta`, `rc` are understood to sort before releases
- Very simple and predictable

### Arch Linux (what `versions` crate is based on)

- The `versions` crate was originally written for Arch Linux package management
- Handles the full chaos of Linux package versions (epochs, multi-segment, mixed alpha-numeric)

## Answers to Questions

### 1. How does Renovate/Dependabot handle version comparison for non-semver projects?

**Renovate:** Uses a pluggable versioning system with 51+ modules. Default is `semver-coerced` which extracts up to 3 numeric segments from any string and pads to semver. Users can override per-package. For truly custom schemes, `regex` versioning with named capture groups is available.

**Dependabot:** Relies on ecosystem-native versioning. Less flexible for cross-ecosystem use. Non-semver handling is acknowledged as a gap.

**Takeaway:** Both accept that coercion is lossy and rely on user configuration for edge cases. Neither attempts fully automatic scheme detection for constraint semantics.

### 2. Can we use `semver` for constraint parsing but a more lenient parser for matching actual tags?

Yes, and this is essentially what `lenient_semver` enables: parse tags leniently into `semver::Version`, then match against `semver::VersionReq`. However, this is lossy for 4+ segment versions.

Better approach: Use the `versions` crate which has its own `Requirement` type that works with `Versioning` (which handles any format natively without lossy coercion).

### 3. What's sane behavior when a tag doesn't parse as any version?

Skip it with a debug/trace log. This matches how every tool handles it:
- Tags like `nightly`, `latest`, `stable`, `release-candidate` are not versions
- Tags like `docs-v2` or `feature-foo` are not versions
- Log at debug level: `skipping tag "nightly": not a valid version`
- Never error on unparseable tags; the tag list is external input we don't control

### 4. Should we support CalVer constraints or just treat as "numeric segments compared left to right"?

**Treat CalVer as numeric segments compared left to right.** This is what Renovate does (via coercion or `loose` versioning). CalVer like `2025.01.15` naturally sorts correctly when compared segment-by-segment numerically.

The `^` operator is meaningless for CalVer (what's "compatible" with `2025.01`?), so for non-semver versions:
- `>=`, `>`, `<`, `<=`, `=` work naturally
- `^` and `~` should either: (a) be rejected with a clear error for non-semver, or (b) fall back to `>=` behavior with a warning
- Recommendation: (a) - reject with a helpful error message suggesting `>=` instead

### 5. Is there a crate that does "try semver, fall back to numeric segment comparison"?

**Yes: the `versions` crate.** Its `Versioning::new()` does exactly this:
1. Try strict SemVer parsing
2. Fall back to structured `Version` (any number of numeric segments)
3. Fall back to `Mess` (handles anything with numbers)

And its `Requirement` type applies constraints against any `Versioning` variant.

### 6. How to handle pre-release versions?

For semver-parsed versions, follow the spec: `1.2.3-alpha < 1.2.3`. Pre-releases are only matched by constraints that explicitly include pre-release identifiers or use `>=` with a pre-release floor.

For non-semver versions with suffixes like `-rc1`, `.Final`:
- The `versions` crate's `Version` type handles common suffixes
- Numeric comparison of suffix numbers (rc1 < rc2)
- Known release suffixes (alpha < beta < rc < release/final)

**Policy:** By default, constraints like `>=1.2.0` should NOT match `1.3.0-beta.1` unless the user explicitly opts in (e.g., `>=1.2.0-0` or a `--include-prereleases` flag). This matches npm/Cargo behavior and prevents users from being surprised by unstable versions.

### 7. What constraint operators make sense for non-semver?

| Operator | SemVer | Non-SemVer |
|----------|--------|------------|
| `=`      | Exact match | Exact match |
| `>`      | Greater than | Greater than (segment-by-segment) |
| `>=`     | Greater or equal | Greater or equal |
| `<`      | Less than | Less than |
| `<=`     | Less or equal | Less or equal |
| `^`      | Compatible (same major) | **Error/warning** - meaningless without semver semantics |
| `~`      | Approximately (same minor) | **Error/warning** - meaningless without semver semantics |
| `*`      | Any | Any |

## Recommendation

**Use the `versions` crate** as the primary version parsing and comparison library, with the following strategy:

1. It handles the full spectrum from strict semver to chaotic version strings
2. Its `Requirement` type provides constraint matching out of the box
3. It gracefully degrades through parsing tiers
4. It's actively maintained and battle-tested (Arch Linux package management heritage)

Supplement with:
- Custom `v`/`V` prefix stripping before parsing (though `versions` may handle this)
- Clear documentation of behavior differences between semver and non-semver constraints
- Debug logging for skipped unparseable tags
- Validation that warns when `^`/`~` are used with detected non-semver versions

## Proposed Strategy

```
Input: constraint string (e.g., ">=2.0.0") + list of git tags

1. Parse the constraint:
   - Use `versions::Requirement::from_str(constraint)`
   - This gives us an operator + a Versioning value

2. For each tag in the tag list:
   a. Strip known prefixes: "v", "V", "release-", etc.
   b. Attempt to parse with `Versioning::new(stripped_tag)`
   c. If parsing fails -> skip, emit debug log
   d. If parsing succeeds -> check `requirement.matches(&versioning)`
   e. If pre-release and constraint doesn't explicitly allow pre-release -> skip

3. Collect all matching versions.

4. Return whether any match exists (for tag_exists).

5. Optionally: return the highest matching version (for future use).
```

### Detecting version scheme for validation:

```
After parsing the constraint version:
- If it parses as Versioning::SemVer -> allow all operators
- If it parses as Versioning::Version or Versioning::Mess:
  - If operator is ^ or ~ -> emit warning:
    "Constraint uses ^ / ~ but version does not appear to be semver.
     These operators assume semver semantics (major.minor.patch).
     Consider using >= instead."
  - Still attempt matching (versions crate handles it), but warn
```

## Example Usage

```rust
use versions::{Requirement, Versioning};

/// Check if any tag in the list satisfies the given version constraint.
fn tag_exists(constraint: &str, tags: &[String]) -> bool {
    let req = match Requirement::from_str(constraint) {
        Ok(r) => r,
        Err(e) => {
            error!("Invalid version constraint '{}': {}", constraint, e);
            return false;
        }
    };

    // Warn if using ^/~ with non-semver constraint version
    if let Some(ref ver) = req.version {
        if matches!(req.op, Op::Tilde | Op::Caret) {
            if !matches!(ver, Versioning::SemVer(_)) {
                warn!(
                    "Constraint '{}' uses ^/~ but version doesn't look like semver. \
                     Consider using >= instead.",
                    constraint
                );
            }
        }
    }

    for tag in tags {
        // Strip common prefixes
        let stripped = tag.strip_prefix('v')
            .or_else(|| tag.strip_prefix('V'))
            .unwrap_or(tag);

        // Try to parse
        let versioning = match Versioning::new(stripped) {
            Some(v) => v,
            None => {
                debug!("Skipping tag '{}': not a valid version", tag);
                continue;
            }
        };

        // Skip pre-releases unless constraint explicitly targets them
        if is_prerelease(&versioning) && !constraint_targets_prerelease(&req) {
            debug!("Skipping pre-release tag '{}'", tag);
            continue;
        }

        // Check match
        if req.matches(&versioning) {
            debug!("Tag '{}' satisfies constraint '{}'", tag, constraint);
            return true;
        }
    }

    false
}

fn is_prerelease(v: &Versioning) -> bool {
    match v {
        Versioning::SemVer(s) => !s.pre.is_empty(),
        // For non-semver, check for common pre-release indicators
        Versioning::Version(v) => {
            // Check if any chunk contains alpha/beta/rc/dev
            v.to_string().contains("-alpha")
                || v.to_string().contains("-beta")
                || v.to_string().contains("-rc")
                || v.to_string().contains("-dev")
        }
        Versioning::Mess(_) => false, // Can't reliably detect
    }
}

fn constraint_targets_prerelease(req: &Requirement) -> bool {
    match &req.version {
        Some(Versioning::SemVer(s)) => !s.pre.is_empty(),
        _ => false,
    }
}
```

### Cargo.toml addition:

```toml
[dependencies]
versions = "6"   # Check latest version
tracing = "0.1"  # For debug/warn logging
```

### Behavior examples:

```
Constraint: >=2.0.0
Tags: ["v1.9.0", "v2.0.0", "v2.1.3", "v3.0.0-beta.1"]
Result: true (matches v2.0.0 and v2.1.3; skips beta)

Constraint: ^2.0
Tags: ["v1.9.0", "v2.0.0", "v2.5.1", "v3.0.0"]
Result: true (matches v2.0.0, v2.5.1; not v3.0.0)

Constraint: >=2025.01
Tags: ["2024.12.01", "2025.01.15", "2025.03.01"]
Result: true (matches 2025.01.15 and 2025.03.01)

Constraint: ^2025.01
Tags: ["2025.01.15", "2025.03.01", "2026.01.01"]
Result: WARNING emitted; behavior depends on versions crate interpretation

Constraint: >=1.2.3.4
Tags: ["1.2.3.3", "1.2.3.4", "1.2.3.5"]
Result: true (matches 1.2.3.4 and 1.2.3.5, segment-by-segment comparison)
```
