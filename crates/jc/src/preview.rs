//! Dry-run / confirm layer.
//!
//! Every mutating command will build a `PreviewedRequest` describing the
//! outgoing HTTP request (method, URL, redacted headers, body as JSON) and
//! optionally a unified diff against current remote state for edit operations.
//!
//! - `--dry-run`: serialize `PreviewedRequest` to stdout, exit 0, no send
//! - `--confirm`: render preview to stderr, block on stdin y/N, then send
//! - default: send
//!
//! Stub — to be implemented alongside the first mutation command.
