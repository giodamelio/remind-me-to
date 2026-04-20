# Research: HTTP Client (Async vs Sync)

## Context

`remind-me-to` needs to make HTTP requests to forge APIs (GitHub REST API initially, others later). The usage pattern:

- After scanning files (parallel, CPU-bound), we have a list of operations to check
- Operations are deduplicated — we batch by repo (e.g., 5 PRs in the same repo)
- We might have 10-50 unique API calls per run (could be more for large scans)
- We need to respect rate limits and back off gracefully
- Must respect `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY` environment variables
- Must support custom headers (auth tokens)
- The HTTP client needs to be injectable via a trait for testing

The key architectural question: should the checking phase be async (tokio + reqwest) or sync (ureq/blocking reqwest)?

## Candidates

### `reqwest` (async, with tokio)

- https://crates.io/crates/reqwest
- Most popular Rust HTTP client
- Full async with tokio runtime
- Also has a blocking API (`reqwest::blocking`)

### `ureq`

- https://crates.io/crates/ureq
- Sync-only, minimal dependencies
- No async runtime needed
- Simpler mental model

### `reqwest::blocking`

- Same crate as async reqwest but sync API
- Still pulls in tokio as a dependency
- Middle ground?

### Others?

- `hyper` (low-level, probably overkill)
- `attohttpc` (minimal sync client)
- `isahc` (libcurl-based)

## Evaluation Criteria

1. **Concurrency model** — can we make multiple API calls in parallel? (important for batching)
2. **Dependency weight** — tokio + reqwest adds a lot of compile time. Is it worth it?
3. **Proxy support** — does it respect standard proxy env vars out of the box?
4. **API ergonomics** — how easy to use for simple REST API calls with JSON?
5. **Testability** — how easy to mock/inject for tests?
6. **TLS backend** — rustls vs native-tls? (affects portability and compile)
7. **Connection pooling** — does it reuse connections across multiple calls to the same host?
8. **Timeout/retry support** — built-in or manual?
9. **Maintenance and ecosystem** — how well supported? Middleware ecosystem?

## Questions to Answer

1. For 10-50 API calls, is async concurrency actually faster than sync with a thread pool? What's the breakeven point?
2. Can we use `reqwest::blocking` to avoid the async runtime while still getting connection pooling and proxy support?
3. What's the compile time difference between `ureq` and `reqwest` (roughly)?
4. If we go sync, can we still parallelize API calls easily (e.g., with rayon or std threads)?
5. Does `ureq` support proxy env vars?
6. What's the impact on the library API? Does async infect the entire API boundary, or can we contain it to the checking phase?
7. If we go async, do we need `tokio` full or just `tokio` with specific features?

## Findings

<!-- To be filled in by research agent -->

## Recommendation

<!-- To be filled in by research agent -->
