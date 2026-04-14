//! Jira attachments.
//!
//! Listing piggybacks on `issue::get` (attachments come back in the standard
//! issue response when `fields=*all` is requested). This module exposes the
//! dedicated metadata, download, and upload endpoints.

use jc_core::{ApiError, Client, DownloadedBlob, Result};
use reqwest::Method;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;

use crate::issue::User;

#[derive(Debug, Deserialize)]
pub struct AttachmentMeta {
    pub id: String,
    pub filename: String,
    #[serde(rename = "mimeType", default)]
    pub mime_type: Option<String>,
    pub size: u64,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub author: Option<User>,
}

/// GET /rest/api/3/attachment/{id}
pub async fn get_meta(client: &Client, id: &str) -> Result<AttachmentMeta> {
    let path = format!("rest/api/3/attachment/{id}");
    client.request_json(Method::GET, &path).await
}

/// GET /rest/api/3/attachment/content/{id} — streams the file bytes.
pub async fn download(client: &Client, id: &str) -> Result<DownloadedBlob> {
    let path = format!("rest/api/3/attachment/content/{id}");
    client.download_bytes(&path).await
}

/// POST /rest/api/3/issue/{key}/attachments — multipart upload.
///
/// Atlassian returns an array of attachment metadata (one entry per file;
/// this helper only uploads a single file at a time).
pub async fn upload(
    client: &Client,
    issue_key: &str,
    filename: &str,
    bytes: Vec<u8>,
    mime_type: Option<&str>,
) -> Result<Vec<AttachmentMeta>> {
    let mut part = Part::bytes(bytes).file_name(filename.to_string());
    if let Some(mt) = mime_type {
        part = part
            .mime_str(mt)
            .map_err(|e| ApiError::config(format!("invalid mime type '{mt}': {e}")))?;
    }
    let form = Form::new().part("file", part);
    let path = format!("rest/api/3/issue/{issue_key}/attachments");
    client.post_multipart(&path, form).await
}
