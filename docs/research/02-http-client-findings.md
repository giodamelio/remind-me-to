# Findings: HTTP Client

## Summary

**Recommendation: `ureq` 3.x (sync) with `rayon` or `std::thread::scope` for parallel API calls.**

For 10-50 API calls with rate limiting, async provides no meaningful advantage over sync parallelism. ureq 3.x gives us proxy support, connection pooling, an injectable Transport trait for testing, significantly faster compile times, and no async infection of the library API. The concurrency we need (bounded parallel requests to avoid rate limits) is trivially achieved with a thread pool or scoped threads.

## Candidate Analysis

### `reqwest` (async)

**Pros:**
- Most popular Rust HTTP client (~250M downloads)
- Excellent proxy support (HTTP_PROXY, HTTPS_PROXY, NO_PROXY auto-detected)
- Connection pooling built-in
- Full HTTP/2 support
- Large ecosystem of middleware (tower layers, retry crates)
- JSON deserialization built-in via serde

**Cons:**
- Requires tokio runtime — async infects the entire call chain
- Heavy dependency tree: pulls in tokio, hyper, h2, tower, pin-project, etc.
- Compile time: roughly 200-300+ transitive dependencies; clean builds measured at 30-60s+ depending on hardware
- Async makes the library trait boundary more complex (`async_trait` or manual futures)
- Overkill for our use case of 10-50 sequential/bounded-parallel requests

**Proxy support:** Automatic from env vars (enabled by default).

**Testability:** Can mock via tower middleware layers, or use trait-based injection. No built-in Transport trait — you'd define your own trait wrapping the Client.

### `ureq`

**Version:** 3.x (current stable is 3.3.x as of early 2026)

**Pros:**
- Sync-only, minimal dependencies (~30-50 transitive deps vs reqwest's 200+)
- Compile time: roughly 5-15s for ureq itself (much faster than reqwest)
- No async runtime needed — no async infection
- Built-in proxy support from env vars (`proxy-from-env` is default in 3.x)
- Connection pooling via Agent (keep-alive reuse)
- **New in 3.x: `Transport` and `Resolver` traits** — allows injecting custom transports for testing
- Sans-IO architecture (ureq-proto crate) — protocol logic is testable independently
- Uses the standard `http` crate types (Request, Response) — interoperable
- rustls by default (no OpenSSL dependency), native-certs for system CA roots
- Small binary size

**Cons:**
- No HTTP/2 support (HTTP/1.1 only — fine for REST API calls)
- No built-in retry logic (removed in 3.x — we'd implement our own, which is preferable anyway for rate limit backoff)
- Blocking I/O means each concurrent request needs its own OS thread
- Smaller ecosystem than reqwest (fewer middleware options)
- Transport trait API is marked "unversioned" — may change in minor releases

**Proxy support:** `proxy-from-env` is a default feature in ureq 3.x. Reads ALL_PROXY, HTTPS_PROXY, HTTP_PROXY (and lowercase variants). NO_PROXY is also respected. For custom Agent creation, use `try_proxy_from_env()` on AgentBuilder.

**Testability:** The Transport trait in ureq 3.x allows injecting a mock transport directly. However, for our use case, defining our own higher-level trait (e.g., `ForgeClient`) is better — it decouples from the HTTP layer entirely and works regardless of which HTTP client we use underneath.

### `reqwest::blocking`

**Pros:**
- Same feature set as async reqwest (proxy, connection pooling, JSON)
- Sync API — no async infection
- Well-tested, well-maintained

**Cons:**
- Still pulls in tokio as a dependency (spawns a runtime internally)
- Same heavy compile time as async reqwest (~200+ deps)
- Cannot be used inside an async context (will panic)
- No real advantage over ureq for our use case — same sync API but heavier

**Verdict:** Worst of both worlds. You get the compile time of async reqwest without the concurrency benefits. If going sync, ureq is strictly better.

### Others

**`attohttpc`:**
- Minimal sync client, similar philosophy to ureq
- Less maintained than ureq, smaller community
- No clear advantage over ureq 3.x

**`isahc`:**
- Based on libcurl (via curl-rust)
- Requires system libcurl — hurts portability
- Async support via its own runtime
- Not recommended for our use case

**`hyper`:**
- Low-level HTTP implementation
- Requires building your own client on top
- Only makes sense if you need custom protocol handling
- Massive overkill for REST API calls

## Answers to Questions

### 1. For 10-50 API calls, is async concurrency actually faster than sync with a thread pool?

**No, not meaningfully.** For 10-50 requests to the same host (GitHub API), the bottleneck is network latency and rate limits, not thread overhead. Spawning 5-10 OS threads to make parallel requests is trivial and adds ~microseconds of overhead. Async shines at thousands of concurrent connections where thread-per-connection would exhaust OS resources. At our scale, the difference is unmeasurable.

Additionally, GitHub's rate limit is 5000 requests/hour (authenticated). We'll likely self-throttle to 5-10 concurrent requests max. A thread pool handles this perfectly.

### 2. Can we use `reqwest::blocking` to avoid async runtime while getting connection pooling and proxy support?

**Yes, but it's not worth it.** `reqwest::blocking` works and provides connection pooling + proxy support. However, it still compiles tokio internally (a hidden runtime is spawned). You pay the full compile-time cost of the async ecosystem without using it. ureq 3.x provides the same features (pooling, proxy) without the weight.

### 3. What's the compile time difference between `ureq` and `reqwest`?

**Roughly 3-5x faster with ureq.** Exact numbers depend on hardware and features, but:
- ureq 3.x with rustls: ~30-50 transitive dependencies, ~5-15s clean build
- reqwest with tokio + rustls: ~200-300 transitive dependencies, ~30-60s clean build

The difference is most noticeable on clean builds and CI. Incremental builds are less affected since the deps are cached after first compile. But for a CLI tool where fast iteration matters, ureq is a clear win.

### 4. If we go sync, can we still parallelize API calls easily?

**Yes, trivially.** Three good options:

1. **`std::thread::scope`** (no extra dependency): Spawn scoped threads, join all. Perfect for bounded parallelism with 5-10 threads.

2. **`rayon`** (already likely in the project for file scanning): Use `par_iter()` on the list of API calls. Rayon's work-stealing thread pool handles it efficiently.

3. **Crossbeam scoped threads**: Similar to std::thread::scope but with more control.

Example with std::thread::scope:
```rust
let results: Vec<_> = std::thread::scope(|s| {
    let handles: Vec<_> = operations
        .chunks(ops_per_thread)
        .map(|chunk| s.spawn(|| check_operations(client, chunk)))
        .collect();
    handles.into_iter().map(|h| h.join().unwrap()).collect()
});
```

### 5. Does `ureq` support proxy env vars?

**Yes.** In ureq 3.x, `proxy-from-env` is a **default feature**. The default Agent automatically reads HTTP_PROXY, HTTPS_PROXY, ALL_PROXY (and lowercase variants), and respects NO_PROXY. This works out of the box with no configuration needed.

For custom Agent builders, call `.try_proxy_from_env()` to opt in.

### 6. What's the impact on the library API? Does async infect the entire API boundary?

**With ureq (sync), there is zero async infection.** The library exposes a simple synchronous API:

```rust
pub fn check_reminders(reminders: Vec<Reminder>, client: &dyn ForgeClient) -> Vec<CheckResult>
```

If we used async reqwest, the library would need:
- `async fn check_reminders(...)` or return a Future
- Callers would need a tokio runtime
- The `ForgeClient` trait would need `async_trait` or manual `Pin<Box<dyn Future>>`
- Testing would require `#[tokio::test]`

Keeping it sync means the library is usable from any context — sync CLI, async server, WASM (future), etc. The checking phase can internally use threads for parallelism without exposing that to callers.

### 7. If we go async, do we need tokio full or just specific features?

**If we went async (not recommended), we'd need:**
- `tokio` with features: `rt`, `rt-multi-thread`, `macros`, `time` (for timeouts/backoff)
- Not the full feature flag — `io-util`, `net`, `fs` etc. are unnecessary

But since the recommendation is ureq, this is moot.

## Recommendation

**Use `ureq` 3.x as the HTTP client.**

Rationale:
1. **Right-sized for the problem.** 10-50 API calls with rate limiting is not a high-concurrency problem. Sync + thread pool is the simplest correct solution.
2. **No async infection.** The library API stays sync, making it simpler to use, test, and reason about.
3. **Fast compile times.** 3-5x fewer dependencies than reqwest. Matters for iteration speed and CI.
4. **Proxy support out of the box.** Default feature in 3.x — HTTP_PROXY, HTTPS_PROXY, NO_PROXY all work automatically.
5. **Injectable for testing.** Define a `ForgeClient` trait at the library boundary. In production, implement it with ureq. In tests, implement it with a mock that returns canned responses. The Transport trait in ureq 3.x is a bonus for lower-level testing if needed.
6. **Connection pooling.** ureq's Agent reuses connections (keep-alive). Create one Agent, share it across calls to the same host.
7. **Standard types.** ureq 3.x uses the `http` crate's Request/Response types — good interop.

**Parallelism strategy:** Use `std::thread::scope` with a bounded number of worker threads (e.g., 5-10). Group operations by host/repo, assign groups to threads. This naturally limits concurrent connections per host and makes rate limiting straightforward.

**Tokio features needed:** None. No async runtime required.

**Cargo.toml:**
```toml
[dependencies]
ureq = { version = "3", features = ["json"] }  # proxy-from-env is default
```

## Example Usage

```rust
use ureq::Agent;
use std::sync::Arc;

/// Trait for forge API interactions — injectable for testing
pub trait ForgeClient: Send + Sync {
    fn get_pr_status(&self, owner: &str, repo: &str, number: u64) -> Result<PrStatus, ApiError>;
    fn get_tags(&self, owner: &str, repo: &str) -> Result<Vec<Tag>, ApiError>;
    // ... other operations
}

/// Production implementation using ureq
pub struct GitHubClient {
    agent: Agent,
    token: Option<String>,
}

impl GitHubClient {
    pub fn new(token: Option<String>) -> Self {
        let agent = Agent::new_with_defaults();  // picks up proxy from env automatically
        Self { agent, token }
    }

    fn request(&self, url: &str) -> Result<ureq::Response, ApiError> {
        let mut req = self.agent.get(url);
        req = req.header("Accept", "application/vnd.github+json");
        req = req.header("User-Agent", "remind-me-to");
        if let Some(ref token) = self.token {
            req = req.header("Authorization", &format!("Bearer {token}"));
        }
        let response = req.call()?;
        Ok(response)
    }
}

impl ForgeClient for GitHubClient {
    fn get_pr_status(&self, owner: &str, repo: &str, number: u64) -> Result<PrStatus, ApiError> {
        let url = format!("https://api.github.com/repos/{owner}/{repo}/pulls/{number}");
        let resp = self.request(&url)?;
        let pr: GitHubPr = resp.body_mut().read_json()?;
        Ok(pr.into())
    }
    // ...
}

/// Parallel checking with bounded concurrency
pub fn check_all(
    operations: &[Operation],
    client: &dyn ForgeClient,
    max_concurrent: usize,
) -> Vec<CheckResult> {
    std::thread::scope(|s| {
        let chunks: Vec<_> = operations.chunks(
            (operations.len() / max_concurrent).max(1)
        ).collect();

        let handles: Vec<_> = chunks.into_iter().map(|chunk| {
            s.spawn(|| {
                chunk.iter().map(|op| check_one(op, client)).collect::<Vec<_>>()
            })
        }).collect();

        handles.into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    })
}

/// Mock for testing — no HTTP needed
#[cfg(test)]
struct MockForgeClient {
    pr_responses: HashMap<(String, String, u64), PrStatus>,
}

#[cfg(test)]
impl ForgeClient for MockForgeClient {
    fn get_pr_status(&self, owner: &str, repo: &str, number: u64) -> Result<PrStatus, ApiError> {
        self.pr_responses
            .get(&(owner.to_string(), repo.to_string(), number))
            .cloned()
            .ok_or(ApiError::NotFound)
    }
    // ...
}
```

## Sources

- [ureq GitHub repository](https://github.com/algesten/ureq)
- [ureq 3.x docs (docs.rs)](https://docs.rs/ureq/latest/ureq/)
- [ureq Transport trait](https://docs.rs/ureq/3.0.0-rc1/ureq/transport/index.html)
- [ureq Proxy documentation](https://docs.rs/ureq/latest/ureq/struct.Proxy.html)
- [ureq 2-to-3 migration guide](https://github.com/algesten/ureq/blob/main/MIGRATE-2-to-3.md)
- [reqwest crate](https://crates.io/crates/reqwest)
- [reqwest::blocking documentation](https://docs.rs/reqwest/latest/reqwest/blocking/index.html)
- [How to choose the right Rust HTTP client (LogRocket)](https://blog.logrocket.com/best-rust-http-client/)
- [Async Rust: When to Use It and When to Avoid It (WyeWorks)](https://wyeworks.com/blog/2025/02/25/async-rust-when-to-use-it-when-to-avoid-it/)
- [The State of Async Rust: Runtimes (corrode.dev)](https://corrode.dev/blog/async/)
