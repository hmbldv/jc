# jc-core

Shared HTTP client, auth, retry, error parsing, and cache primitives
for the [`jc`](https://github.com/hmbldv/jc) CLI (Jira + Confluence).

This crate has no knowledge of Atlassian-specific endpoints. The
product client crates ([`jc-jira`](https://crates.io/crates/jc-jira)
and [`jc-conf`](https://crates.io/crates/jc-conf)) layer on top of it.

## What's in here

- **`Client`** — a thin `reqwest` wrapper with basic-auth injection,
  query-safe URL scrubbing for logs, bounded response bodies (16 MiB
  cap via chunked reads), and explicit `Policy::limited(10)` +
  `https_only(true)` redirect handling.
- **`RetryPolicy`** — per-verb retry (`Read` / `IdempotencySafe` /
  `None`) with `Retry-After` honoring, exponential backoff, and a
  120-second circuit breaker.
- **`ApiError`** — structured error envelope that parses Atlassian's
  `errorMessages` / `errors` JSON shape.
- **`literal`** — shared JQL/CQL literal escaping and relative-time
  validation used by both product crates.
- **`cache`** — tiny read/write helpers for `~/.cache/jc/*.json`.

## Usage

This crate is primarily an implementation detail of the `jc` binary.
It's published so the four workspace crates can reference it by
version, not because it's a general-purpose library. If you want a
Jira/Confluence Rust client, start with
[`jc-jira`](https://crates.io/crates/jc-jira) or
[`jc-conf`](https://crates.io/crates/jc-conf), or use the
[`jc` CLI](https://github.com/hmbldv/jc) directly.

## License

MIT — see [`LICENSE`](LICENSE).
