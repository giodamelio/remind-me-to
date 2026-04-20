# Findings: HTTP Mocking / Testing

## Summary

Use a **two-layer strategy**: trait-based mocking for unit tests (zero extra dependencies) and `httpmock` as the mock HTTP server for integration tests. Skip recording/replay (rvcr) for now -- the trait layer covers most needs, and httpmock fills the gap for full HTTP round-trip testing. This keeps the dependency footprint small while giving thorough coverage of API interactions, error handling, rate limiting, and proxy behavior.

## Candidate Analysis

### Trait-based mocking

The architecture already calls for an injectable `ForgeClient` trait. This is the most valuable testing layer and requires no extra crates.

**How it works:** Define a trait like `ForgeClient` with methods for each API operation (get PR status, list tags, etc.). In production, implement it with a real HTTP client. In tests, implement it with a struct that returns canned responses.

```rust
#[cfg_attr(test, mockall::automock)]
trait ForgeClient {
    fn get_pr(&self, owner: &str, repo: &str, number: u64) -> Result<PrInfo, ForgeError>;
    fn list_tags(&self, owner: &str, repo: &str) -> Result<Vec<Tag>, ForgeError>;
    // ...
}
```

**Pros:**
- Zero dependencies (or use `mockall` for auto-generated mocks)
- Extremely fast -- no network, no server startup
- Tests logic and response parsing directly
- Easy to simulate error cases (return `Err(RateLimit { retry_after: 60 })`)
- Easy to test batching/deduplication logic (count how many times each method is called)
- Works regardless of whether the real HTTP client is sync or async

**Cons:**
- Does not test actual HTTP request construction (headers, URL paths, query params)
- Does not test JSON serialization/deserialization from raw HTTP responses
- Does not test that the real HTTP client is configured correctly (proxy, timeouts, TLS)
- Mocks can drift from real API behavior over time

**Verdict:** Essential. Use for all unit tests of business logic, operation checking, batching, deduplication, and error handling.

### `wiremock`

- **Crate:** https://crates.io/crates/wiremock (v0.6.5, actively maintained by Luca Palmieri)
- **Model:** Async-only. Spins up a real HTTP server on a random port per test.
- **Runtime:** Works with both tokio and async-std.
- **Downloads:** ~2.6M total, very popular in the Rust ecosystem.

**Key features:**
- Rich request matching (method, path, headers, body, query params, custom matchers via `Match` trait)
- Response templating with `ResponseTemplate`
- Expectation verification (assert a mock was called N times)
- Each `MockServer` gets its own random port -- full test isolation
- Scoped mocks that verify on drop

**Sync client compatibility:** wiremock itself is async, but it works fine with sync HTTP clients like `ureq` or `reqwest::blocking` -- the mock server runs on its own async runtime internally. The test code needs to be async (use `#[tokio::test]`), but the HTTP client under test can be sync.

**Pros:**
- Battle-tested, well-documented (from "Zero to Production in Rust")
- Excellent test isolation via random ports
- Rich matching and verification API
- Active maintenance

**Cons:**
- Async-only API means test functions must be async (`#[tokio::test]`)
- Pulls in tokio as a dev-dependency even if the main app is sync
- Heavier than needed if the trait-based layer covers most cases
- No built-in record/replay

### `mockito`

- **Crate:** https://crates.io/crates/mockito (v1.x, actively maintained)
- **Model:** Both sync and async APIs. Spins up a real HTTP server.

**Key features:**
- Simple, straightforward API
- Sync and async interfaces
- Pool of mock servers to avoid port conflicts
- Mocks are scoped to a `Server` instance -- cleaned up when server goes out of scope
- Request matching on method, path, headers, body

**Concurrent test handling:** Since v1.0, mockito creates a separate server per `Server::new()` call with its own random port. Tests run fully in parallel without port conflicts. This was a major improvement over older versions that used a single shared server.

**Pros:**
- Simpler API than wiremock -- less boilerplate per test
- Sync API available (no `#[tokio::test]` needed)
- Good test isolation since v1.0

**Cons:**
- Less powerful matching than wiremock (no custom matchers)
- Less expressive expectation/verification API
- Fewer features for complex scenarios (no response sequencing out of the box)

### `httpmock`

- **Crate:** https://crates.io/crates/httpmock (v0.8.x, actively maintained)
- **Model:** Async core with both sync and async APIs. Spins up a real HTTP server.

**Key features:**
- Sync API (`MockServer::start()`) and async API (`MockServer::start_async()`)
- Rich built-in request matchers (regex, JSON body, JSONPath, cookies, form data)
- Response sequencing (return different responses for repeated calls -- perfect for rate limit simulation)
- **Record and playback mode** -- can act as a proxy, record real requests, replay them
- **Forward/proxy mode** -- can forward requests to a real server
- **YAML mock configuration** -- define mocks in YAML files
- Standalone server mode (can run as a separate process)
- Request verification/expectations

**Concurrent test handling:** Each `MockServer::start()` creates a server on a random port. Tests are fully isolated and can run in parallel.

**Pros:**
- Both sync and async APIs -- works naturally regardless of HTTP client choice
- Response sequencing built-in (critical for testing rate limit back-off)
- Built-in record/playback (eliminates need for rvcr)
- Most feature-rich of the three mock server options
- Standalone mode could be useful for E2E testing

**Cons:**
- Slightly larger dependency than mockito
- API is more verbose than mockito for simple cases
- Fewer total downloads than wiremock (but healthy ecosystem)

### Recording/replay approaches

#### `rvcr`

- **Crate:** https://crates.io/crates/rvcr
- **Model:** Middleware for `reqwest` (via `reqwest-middleware`). Records HTTP interactions to JSON "cassette" files and replays them.
- **Inspired by:** Ruby VCR / Python vcrpy

**How it works:**
1. First run: requests go to the real API and responses are saved to a cassette file
2. Subsequent runs: responses are replayed from the cassette -- no network needed

**Pros:**
- Tests against real API responses -- highest fidelity
- Cassettes serve as documentation of expected API behavior
- Fast replay (no network)
- Can filter sensitive data (tokens) before recording

**Cons:**
- Couples to `reqwest-middleware` -- won't work if we use `ureq`
- Low adoption (368 downloads/month) -- maintenance risk
- Cassettes need regeneration when API changes
- Requires actual API access (and tokens) to generate initial recordings
- Only works with `reqwest`, not with trait-level abstraction
- Adds complexity to CI (need to manage cassette files, decide when to re-record)

#### `surf-vcr`

- Similar concept but for the `surf` HTTP client (less relevant)

#### Custom recording proxy

- Set `HTTP_PROXY` to a recording proxy (e.g., mitmproxy with scripts)
- Record real traffic, replay from files
- Most flexible but most complex to set up
- Better suited for manual exploratory testing than automated CI

#### httpmock's built-in record/playback

- httpmock itself supports record and playback mode
- Can act as a proxy that records responses and then replays them
- Avoids needing a separate crate
- This is a strong advantage of httpmock over wiremock/mockito

## Testing Strategy

### Layer 1: Unit tests (trait-based mocking)

**What to test:**
- Operation checking logic (given a PR response, is the reminder triggered?)
- Response parsing (deserialize GitHub API JSON into our types)
- Error handling (rate limit errors, 404s, 403s, network errors)
- Batching and deduplication (multiple reminders for the same PR make one API call)
- Rate limit back-off logic (given rate limit state, do we wait or proceed?)

**How:**
- `MockForgeClient` implementing the `ForgeClient` trait
- Either hand-written mock struct or use `mockall` for auto-generated mocks
- `mockall` is useful for verifying call counts (deduplication testing)
- No extra crate needed for hand-written mocks

**Coverage target:** This layer should cover 80%+ of the testing surface.

### Layer 2: Integration tests (httpmock)

**What to test:**
- Correct HTTP request construction (method, URL path, headers, query params)
- JSON deserialization from actual HTTP response bodies
- HTTP error code handling (real 404/403/429 responses)
- Rate limit header parsing (X-RateLimit-Remaining, Retry-After)
- Request sequencing (first call returns 429, second call succeeds after back-off)
- That the HTTP client is configured correctly (timeouts, user-agent)

**How:**
- `httpmock::MockServer` with assertions on received requests
- Use response sequencing for rate limit simulation
- Test against the real HTTP client implementation (not the trait mock)
- Use sync API if we go with ureq, async if we go with reqwest

### Layer 3: Proxy/E2E tests (optional, deferred)

**What to test:**
- That `HTTP_PROXY`/`HTTPS_PROXY` env vars are respected
- Full scan-check-report pipeline against realistic API responses

**How:**
- Use httpmock in standalone/proxy mode, or
- Set `HTTP_PROXY` to point to the httpmock server
- Could also use httpmock's record/playback to capture real GitHub API responses once and replay them

**Note:** Testing proxy env var support is tricky. The most reliable approach:
1. Start an httpmock server
2. Set `HTTP_PROXY` env var to point to that server
3. Make a request through the real HTTP client
4. Verify httpmock received the request (proving the proxy was used)

## Answers to Questions

### 1. If we use trait-based mocking for unit tests, do we still need a mock HTTP server?

**Yes, but for a narrow set of integration tests.** Trait-based mocking tests business logic but not HTTP mechanics. You still want integration tests to verify:
- URL construction (`/repos/{owner}/{repo}/pulls/{number}`)
- Header construction (Authorization, Accept, User-Agent)
- JSON deserialization from real HTTP response bodies
- HTTP status code handling
- Rate limit header parsing

Without a mock server, bugs in these areas would only surface against the real API. The mock server layer can be small (10-20 tests covering the HTTP client implementation) while trait mocking covers the bulk.

### 2. Does wiremock work with sync HTTP clients, or only async?

**It works with sync HTTP clients for the HTTP traffic**, but the test setup code must be async. The `MockServer` requires `#[tokio::test]` to start. Once started, a sync client (ureq, reqwest::blocking) can make requests to `mock_server.uri()` without issues -- it is just a regular HTTP server from the client's perspective.

If you want fully sync test code (no tokio in tests at all), use **mockito** or **httpmock** instead -- both offer sync APIs for test setup.

### 3. Is there a Rust equivalent to Ruby VCR / Python vcrpy?

**Yes:**
- **`rvcr`** -- middleware for `reqwest` that records/replays HTTP interactions to JSON cassette files. Low adoption (368 downloads/month) and couples to `reqwest-middleware`.
- **`httpmock` record/playback** -- httpmock has built-in record and playback mode. It can proxy real requests, record them, and replay them later. This is more mature and does not couple to a specific HTTP client.
- **`surf-vcr`** -- for the `surf` client only (not relevant here).

The most practical option is httpmock's built-in record/playback, as it does not require an additional dependency and works with any HTTP client.

### 4. How do mockito and httpmock handle concurrent tests?

**Both handle concurrency well since their recent major versions:**

- **mockito (v1.0+):** Each `Server::new()` creates a new server on a random port. Servers are fully isolated. Mocks are scoped to their server instance and cleaned up on drop. Tests run in parallel without interference.

- **httpmock (v0.7+):** Each `MockServer::start()` creates a server on a random port. Full isolation between tests. No shared state. Parallel-safe by default.

- **wiremock:** Same model -- random port per `MockServer::start()`. Pools servers internally for efficiency.

All three are safe for `cargo test` parallel execution and `cargo-nextest`.

### 5. Could we get away with just trait mocking and skip mock servers entirely?

**Mostly yes, with caveats.** For a CLI tool like remind-me-to where the HTTP client is injected via a trait, trait-based mocking covers the vast majority of test scenarios:
- All business logic (operation checking, batching, deduplication)
- Error handling flows
- Rate limit back-off decisions
- Output formatting

What you would miss:
- Bugs in URL/header construction
- JSON serialization/deserialization mismatches
- HTTP client configuration issues (proxy, TLS, timeouts)
- Real HTTP edge cases (chunked encoding, connection drops)

**Practical recommendation:** Start with trait-only mocking. Add a small number of httpmock integration tests for the HTTP client implementation layer only. You can defer the integration tests until the HTTP client code stabilizes.

### 6. How to test that proxy env vars are respected?

**Use a mock HTTP server as a proxy:**

```rust
#[test]
fn test_proxy_env_var_is_respected() {
    // Start a mock server that will act as the proxy
    let proxy_server = MockServer::start();
    
    // Set up the mock to expect any request forwarded through the proxy
    proxy_server.mock(|when, then| {
        when.method(GET)
            .path("/repos/owner/repo/pulls/123");
        then.status(200)
            .json_body(json!({"state": "open", "merged": false}));
    });
    
    // Set the proxy env var to point to our mock server
    std::env::set_var("HTTP_PROXY", proxy_server.base_url());
    
    // Create the real HTTP client (which should read HTTP_PROXY)
    let client = RealHttpClient::new();
    
    // Make a request to a non-existent host -- it should go through the proxy
    let result = client.get("http://api.github.com/repos/owner/repo/pulls/123");
    
    // If we got a response, the proxy was used (the real host doesn't exist / 
    // we're not hitting the real GitHub)
    assert!(result.is_ok());
    
    // Verify the proxy received the request
    proxy_server.verify();
}
```

**Important caveats:**
- `HTTPS_PROXY` testing is harder because the proxy needs to handle TLS (CONNECT tunneling). For basic testing, use `HTTP_PROXY` with non-TLS URLs.
- Use `std::env::set_var` carefully in tests -- env vars are process-global, so these proxy tests should not run in parallel with other tests that depend on env vars. Use `serial_test` crate or `cargo-nextest` (which runs each test in its own process).
- httpmock's proxy/forward mode can help here.

## Recommendation

### Primary: Trait-based mocking + httpmock

1. **Unit tests: Trait-based mocking (no extra crate, or mockall)**
   - Hand-write a `MockForgeClient` struct for simple cases
   - Consider `mockall` if you need to verify call counts frequently (deduplication tests)
   - This covers 80%+ of testing needs

2. **Integration tests: `httpmock`**
   - Best fit for this project because:
     - **Sync and async APIs** -- works regardless of whether we choose reqwest or ureq
     - **Response sequencing** -- essential for rate limit back-off testing
     - **Built-in record/playback** -- eliminates need for rvcr
     - **Proxy mode** -- can test HTTP_PROXY support
   - Use for HTTP client implementation tests only (URL construction, headers, JSON parsing, error codes)
   - Small test surface (~10-20 tests)

3. **Skip rvcr** -- low adoption, couples to reqwest-middleware, and httpmock's record/playback covers the same need if we ever want it.

4. **Defer proxy/E2E testing** -- add after the HTTP client implementation stabilizes. Use httpmock as a proxy target.

### Why httpmock over wiremock or mockito

| Criterion | httpmock | wiremock | mockito |
|-----------|----------|----------|---------|
| Sync API for test setup | Yes | No (async only) | Yes |
| Async API for test setup | Yes | Yes | Yes |
| Response sequencing | Yes | Yes | Limited |
| Record/playback | Yes (built-in) | No | No |
| Proxy mode | Yes | No | No |
| Custom matchers | Yes | Yes (Match trait) | Limited |
| Test isolation | Random port | Random port | Random port |
| Maintenance | Active | Active | Active |

httpmock is the most versatile option. It works regardless of the sync/async HTTP client decision, has the richest feature set for our specific needs (rate limit simulation, record/playback, proxy mode), and its sync API means simpler test code if we go with a sync HTTP client like ureq.

## Example Code

### Trait-based mock (unit test)

```rust
use std::collections::HashMap;

// The trait (in src/ops/types.rs)
pub trait ForgeClient {
    fn get_pr(&self, owner: &str, repo: &str, number: u64) -> Result<PrInfo, ForgeError>;
    fn list_tags(&self, owner: &str, repo: &str) -> Result<Vec<TagInfo>, ForgeError>;
}

// The mock (in tests or behind #[cfg(test)])
#[cfg(test)]
pub struct MockForgeClient {
    pub pr_responses: HashMap<(String, String, u64), Result<PrInfo, ForgeError>>,
    pub call_count: std::cell::RefCell<HashMap<String, usize>>,
}

#[cfg(test)]
impl MockForgeClient {
    pub fn new() -> Self {
        Self {
            pr_responses: HashMap::new(),
            call_count: std::cell::RefCell::new(HashMap::new()),
        }
    }

    pub fn with_pr(mut self, owner: &str, repo: &str, number: u64, response: Result<PrInfo, ForgeError>) -> Self {
        self.pr_responses.insert((owner.into(), repo.into(), number), response);
        self
    }

    pub fn times_called(&self, method: &str) -> usize {
        *self.call_count.borrow().get(method).unwrap_or(&0)
    }
}

#[cfg(test)]
impl ForgeClient for MockForgeClient {
    fn get_pr(&self, owner: &str, repo: &str, number: u64) -> Result<PrInfo, ForgeError> {
        *self.call_count.borrow_mut().entry("get_pr".into()).or_default() += 1;
        self.pr_responses
            .get(&(owner.into(), repo.into(), number))
            .cloned()
            .unwrap_or(Err(ForgeError::NotFound))
    }

    fn list_tags(&self, _owner: &str, _repo: &str) -> Result<Vec<TagInfo>, ForgeError> {
        *self.call_count.borrow_mut().entry("list_tags".into()).or_default() += 1;
        Ok(vec![])
    }
}

// Usage in a test
#[test]
fn test_pr_merged_triggers_reminder() {
    let client = MockForgeClient::new()
        .with_pr("tokio-rs", "tokio", 5432, Ok(PrInfo {
            state: PrState::Closed,
            merged: true,
            merged_at: Some("2025-01-15T00:00:00Z".into()),
            merge_commit_sha: Some("abc123".into()),
        }));

    let op = Operation::PrMerged {
        forge: Forge::GitHub,
        owner: "tokio-rs".into(),
        repo: "tokio".into(),
        number: 5432,
    };

    let result = check_operation(&client, &op).unwrap();
    assert!(result.triggered);
    assert_eq!(client.times_called("get_pr"), 1);
}

#[test]
fn test_deduplication_makes_one_api_call() {
    let client = MockForgeClient::new()
        .with_pr("tokio-rs", "tokio", 5432, Ok(PrInfo {
            state: PrState::Closed,
            merged: true,
            merged_at: Some("2025-01-15T00:00:00Z".into()),
            merge_commit_sha: Some("abc123".into()),
        }));

    // Same operation referenced from multiple files
    let ops = vec![
        Operation::PrMerged { forge: Forge::GitHub, owner: "tokio-rs".into(), repo: "tokio".into(), number: 5432 },
        Operation::PrMerged { forge: Forge::GitHub, owner: "tokio-rs".into(), repo: "tokio".into(), number: 5432 },
    ];

    let results = check_operations_batched(&client, &ops).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(client.times_called("get_pr"), 1); // Only one API call despite two operations
}
```

### Integration test with httpmock

```rust
use httpmock::prelude::*;
use serde_json::json;

#[test]
fn test_github_pr_request_construction() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/tokio-rs/tokio/pulls/5432")
            .header("Accept", "application/vnd.github.v3+json")
            .header("Authorization", "Bearer test-token");
        then.status(200)
            .header("Content-Type", "application/json")
            .json_body(json!({
                "number": 5432,
                "state": "closed",
                "merged": true,
                "merged_at": "2025-01-15T00:00:00Z",
                "merge_commit_sha": "abc123def456"
            }));
    });

    // Create the real GitHub client pointing at our mock server
    let client = GitHubClient::new_with_base_url(
        &server.base_url(),
        "test-token",
    );

    let pr = client.get_pr("tokio-rs", "tokio", 5432).unwrap();
    assert!(pr.merged);
    assert_eq!(pr.merge_commit_sha, Some("abc123def456".into()));

    // Verify the mock was called exactly once
    mock.assert();
}

#[test]
fn test_rate_limit_backoff() {
    let server = MockServer::start();

    // First request: rate limited
    let rate_limit_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/owner/repo/pulls/1");
        then.status(429)
            .header("Retry-After", "1")
            .header("X-RateLimit-Remaining", "0")
            .header("X-RateLimit-Reset", "1700000000")
            .json_body(json!({"message": "API rate limit exceeded"}));
    });

    // Second request (after back-off): success
    let success_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/owner/repo/pulls/1");
        then.status(200)
            .json_body(json!({
                "number": 1,
                "state": "open",
                "merged": false
            }));
    });

    let client = GitHubClient::new_with_base_url(&server.base_url(), "token");
    let pr = client.get_pr_with_retry("owner", "repo", 1).unwrap();

    assert_eq!(pr.state, PrState::Open);
    rate_limit_mock.assert();
    success_mock.assert();
}

#[test]
fn test_404_returns_not_found_error() {
    let server = MockServer::start();

    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/owner/repo/pulls/99999");
        then.status(404)
            .json_body(json!({"message": "Not Found"}));
    });

    let client = GitHubClient::new_with_base_url(&server.base_url(), "token");
    let result = client.get_pr("owner", "repo", 99999);

    assert!(matches!(result, Err(ForgeError::NotFound)));
}
```

### Proxy env var test

```rust
use httpmock::prelude::*;
use serde_json::json;
use serial_test::serial;

#[test]
#[serial] // env vars are process-global; run this test serially
fn test_http_proxy_is_respected() {
    let proxy = MockServer::start();

    proxy.mock(|when, then| {
        when.method(GET);
        then.status(200)
            .json_body(json!({"proxied": true}));
    });

    // Temporarily set the proxy env var
    std::env::set_var("HTTP_PROXY", proxy.base_url());

    // Create client that should respect HTTP_PROXY
    let client = create_http_client("token");

    // Request to any HTTP URL should go through our proxy
    let response = client.get("http://api.github.com/repos/test/test/pulls/1");
    assert!(response.is_ok());

    // Clean up
    std::env::remove_var("HTTP_PROXY");

    // The proxy server received the request -- proving the env var was respected
    proxy.verify();
}
```
