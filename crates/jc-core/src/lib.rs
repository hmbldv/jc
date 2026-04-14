//! Shared infrastructure for jc: HTTP client, auth, retry, error parsing, cache.
//!
//! This crate has no knowledge of Atlassian-specific endpoints. The product
//! client crates (jc-jira, jc-conf) layer on top of it.

pub mod cache;
pub mod client;
pub mod error;
pub mod paginate;
pub mod retry;

pub use client::Client;
pub use error::{ApiError, Result};
