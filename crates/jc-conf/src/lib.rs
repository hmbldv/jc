//! Confluence Cloud REST v2 typed client.
//!
//! Module map (stubs land here as they are implemented):
//! - `page` — get/list/search/create/update/move/delete/children
//!   Reads request `body-format=atlas_doc_format` so one ADF converter
//!   serves both Jira and Confluence.
//! - `space` — list/get
//! - `search` — CQL
//! - `attachments` — list/get/upload
//! - `types` — shared request/response types

pub mod types;
