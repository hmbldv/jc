//! Jira Cloud REST v3 typed client.
//!
//! Module map (stubs land here as they are implemented):
//! - `issue` — get/list/create/edit/transition
//! - `comment` — list/add/edit/delete
//! - `search` — new /rest/api/3/search/jql cursor pagination
//! - `fields` — custom field name<->ID resolution
//! - `transitions` — fuzzy name -> transition ID
//! - `attachments` — list/get/upload
//! - `users` — /myself, user search (powers mention resolution)
//! - `types` — shared request/response types

pub mod comment;
pub mod issue;
pub mod jql;
pub mod search;
pub mod types;
pub mod users;
