//! Markdown ↔ Atlassian Document Format (ADF) converter.
//!
//! Pure functions, no I/O. Load-bearing for every read and write path in
//! `jc`.
//!
//! ## Coverage
//!
//! - **Full round-trip:** paragraphs, headings (H1–H6), text marks (strong,
//!   em, code, strike), links, bullet and ordered lists (including
//!   tight-list inline handling), fenced code blocks with language hints,
//!   blockquotes, horizontal rules, hard breaks, GFM tables (header + body,
//!   inline marks preserved; column alignment is dropped because ADF has
//!   no per-column alignment).
//! - **Read-only:** `@user` mentions (rendered as `@name`), `mediaSingle`
//!   images (rendered as `![alt](attachment:ID)` sidecar references),
//!   `inlineCard`, `emoji`.
//! - **Lossless escape hatch:** any ADF node type the converter doesn't
//!   explicitly handle is rendered as a ` ```adf:<type>` fenced code block
//!   whose body is the raw node JSON. When that markdown is re-parsed by
//!   [`to_adf`], the fenced block re-inflates verbatim. Fence length is
//!   chosen dynamically so nested backticks in the serialized JSON can't
//!   break out of the block.
//!
//! ## Deferred
//!
//! Write-path implementations for: generated table of contents, typed
//! user mentions (requires an async accountId lookup hook), and the
//! inline-image upload pipeline. All of these round-trip losslessly
//! via the escape hatch; they just lack friendly markdown syntax.

pub mod from_adf;
pub mod to_adf;

/// ADF is kept as `serde_json::Value` internally. That keeps the converter
/// small and trivially extensible — new node types are one match arm away.
pub type AdfDocument = serde_json::Value;

pub use from_adf::to_markdown;
pub use to_adf::to_adf;
