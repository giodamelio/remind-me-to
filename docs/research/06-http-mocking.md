# Research: HTTP Mocking / Testing

## Context

`remind-me-to` makes HTTP requests to forge APIs (GitHub REST API for MVP). We need to test:

- That we make the correct API calls for each operation type
- That we parse responses correctly
- That we handle error responses (404, 403, rate limits, timeouts)
- That we batch/deduplicate requests correctly
- That rate limit back-off works

The library architecture has the HTTP client injectable via traits, so we have two testing layers:

1. **Unit tests (trait-based):** Mock at the Rust trait level — no real HTTP, just return fake responses
2. **Integration tests:** Possibly spin up a real mock HTTP server to test the full request/response cycle

We also want the tool to respect `HTTP_PROXY`/`HTTPS_PROXY` env vars, which opens up recording-proxy style testing.

## Candidates

### Trait-based mocking (no crate needed)

- Define a `ForgeClient` trait, implement a `MockForgeClient` for tests
- No extra dependencies
- Tests the logic but not the HTTP serialization

### `wiremock`

- https://crates.io/crates/wiremock
- Async, spins up a real HTTP server
- Flexible request matching and response stubbing
- From the author of "Zero to Production in Rust"

### `mockito`

- https://crates.io/crates/mockito
- Simpler API than wiremock
- Also spins up a real server
- Sync-friendly

### `httpmock`

- https://crates.io/crates/httpmock
- Both sync and async support
- Standalone mock server option (can record/replay)

### Recording proxy approach

- Use a tool like `vcrpy` equivalent for Rust
- Record real API responses, replay in tests
- Ensures tests match real API behavior
- `rvcr` crate? Or custom with `HTTP_PROXY`?

## Evaluation Criteria

1. **Sync vs async compatibility** — does it work with our HTTP client choice?
2. **Request verification** — can we assert that specific requests were made?
3. **Response sequencing** — can we return different responses for repeated calls (rate limit simulation)?
4. **Setup ergonomics** — how much boilerplate per test?
5. **Test isolation** — do tests interfere with each other? (port conflicts, shared state)
6. **Dependency weight** — dev-dependency only, but still matters for CI time
7. **Recording/replay capability** — can we record real API interactions for regression tests?

## Questions to Answer

1. If we go with trait-based mocking for unit tests, do we still need a mock HTTP server for integration tests?
2. Does `wiremock` work with sync HTTP clients, or only async?
3. Is there a Rust equivalent to Ruby's VCR / Python's vcrpy for recording real API responses?
4. How do `mockito` and `httpmock` handle concurrent tests? (multiple tests binding to the same port?)
5. What's the minimal viable approach — could we get away with just trait mocking and skip mock servers entirely?
6. How do we test that proxy env vars are actually respected?

## Findings

<!-- To be filled in by research agent -->

## Recommendation

<!-- To be filled in by research agent -->
