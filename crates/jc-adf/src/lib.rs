//! Markdown <-> Atlassian Document Format (ADF) converter.
//!
//! Pure functions, no I/O. Load-bearing for every write path in jc.
//!
//! Fidelity rules:
//! - GFM tables: full round-trip, alignment preserved
//! - Table of contents: generated and detected
//! - Inline images: `![alt](attachment:name.png)` sidecar pattern
//! - Links: full round-trip
//! - Mentions: `@user` -> ADF mention (requires resolver hook injection)
//! - Code blocks: full fidelity with language hints
//! - Exotic nodes (panel, status, expand, layout): fenced blocks with
//!   `adf:<type>:<variant>` marker, lossless round-trip
//!
//! This crate exposes a small core plus trait hooks for:
//! - User mention resolution (`MentionResolver`)
//! - Image upload orchestration (`AttachmentUploader`)
//!
//! Consumers inject concrete implementations from jc-jira / jc-conf.

pub mod attachments;
pub mod from_adf;
pub mod mentions;
pub mod tables;
pub mod to_adf;
pub mod toc;
pub mod unknown;

/// ADF is kept as `serde_json::Value` internally. That keeps the converter
/// small and trivially extensible — new node types are one match arm away.
pub type AdfDocument = serde_json::Value;

pub use from_adf::to_markdown;
pub use to_adf::to_adf;
