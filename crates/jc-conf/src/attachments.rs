//! Confluence attachments.
//!
//! Reads use v2 endpoints; uploads still go through the v1
//! `/wiki/rest/api/content/{page-id}/child/attachment` multipart route
//! (v2 has no attachment upload surface yet).

use jc_core::{ApiError, Client, DownloadedBlob, Result};
use reqwest::Method;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct AttachmentMeta {
    pub id: String,
    pub title: String,
    #[serde(rename = "mediaType", default)]
    pub media_type: Option<String>,
    #[serde(rename = "fileSize", default)]
    pub file_size: Option<u64>,
    #[serde(rename = "pageId", default)]
    pub page_id: Option<String>,
    #[serde(rename = "downloadLink", default)]
    pub download_link: Option<String>,
    #[serde(rename = "webuiLink", default)]
    pub webui_link: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AttachmentList {
    #[serde(default)]
    results: Vec<AttachmentMeta>,
    #[serde(rename = "_links", default)]
    links: Option<Links>,
}

#[derive(Debug, Deserialize)]
struct Links {
    #[serde(default)]
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UploadedAttachment {
    pub id: String,
    pub title: String,
    #[serde(rename = "type", default)]
    pub content_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UploadResponse {
    #[serde(default)]
    results: Vec<UploadedAttachment>,
}

/// GET /wiki/api/v2/attachments/{id}
pub async fn get_meta(client: &Client, id: &str) -> Result<AttachmentMeta> {
    let path = format!("wiki/api/v2/attachments/{id}");
    client.request_json(Method::GET, &path).await
}

/// GET /wiki/api/v2/pages/{page-id}/attachments — auto-paginated.
pub async fn list_on_page(
    client: &Client,
    page_id: &str,
    limit: usize,
) -> Result<Vec<AttachmentMeta>> {
    let mut results: Vec<AttachmentMeta> = Vec::new();
    let mut path = format!("wiki/api/v2/pages/{page_id}/attachments?limit=250");

    loop {
        let page: AttachmentList = client.request_json(Method::GET, &path).await?;
        let got = page.results.len();
        results.extend(page.results);

        if limit > 0 && results.len() >= limit {
            results.truncate(limit);
            break;
        }
        match page.links.and_then(|l| l.next) {
            Some(next) if got > 0 => {
                let trimmed = next.trim_start_matches('/').trim_start_matches("wiki/");
                path = format!("wiki/{trimmed}");
            }
            _ => break,
        }
    }
    Ok(results)
}

/// Download attachment bytes by ID. Uses the `downloadLink` from the
/// attachment metadata — that link is relative to the site root.
pub async fn download(client: &Client, id: &str) -> Result<(AttachmentMeta, DownloadedBlob)> {
    let meta = get_meta(client, id).await?;
    let link = meta
        .download_link
        .as_ref()
        .ok_or_else(|| ApiError::config(format!("attachment {id} has no downloadLink")))?;
    let trimmed = link.trim_start_matches('/').trim_start_matches("wiki/");
    let full_path = format!("wiki/{trimmed}");
    let blob = client.download_bytes(&full_path).await?;
    Ok((meta, blob))
}

/// POST /wiki/rest/api/content/{page-id}/child/attachment — multipart upload.
pub async fn upload(
    client: &Client,
    page_id: &str,
    filename: &str,
    bytes: Vec<u8>,
    mime_type: Option<&str>,
) -> Result<Vec<UploadedAttachment>> {
    let mut part = Part::bytes(bytes).file_name(filename.to_string());
    if let Some(mt) = mime_type {
        part = part
            .mime_str(mt)
            .map_err(|e| ApiError::config(format!("invalid mime type '{mt}': {e}")))?;
    }
    let form = Form::new().part("file", part);
    let path = format!("wiki/rest/api/content/{page_id}/child/attachment");
    let resp: UploadResponse = client.post_multipart(&path, form).await?;
    Ok(resp.results)
}
