//! Confluence Cloud REST v2 typed client.
//!
//! Module map:
//! - `page` — get/list/create/update/delete, body-format=atlas_doc_format
//! - `space` — list, get, find_by_key (key→id resolution)
//! - `search` — CQL (served by the v1 endpoint; v2 doesn't cover CQL yet)
//! - `attachments` — list, download, upload (upload still uses v1)

pub mod attachments;
pub mod page;
pub mod search;
pub mod space;
