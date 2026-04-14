use bytes::Bytes;
use reqwest::multipart::Form;
use reqwest::{Method, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tracing::debug;
use url::Url;

use crate::error::{ApiError, Result};

/// Binary response with content-type metadata. Used for attachment downloads.
#[derive(Debug)]
pub struct DownloadedBlob {
    pub bytes: Bytes,
    pub content_type: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Client {
    http: reqwest::Client,
    base: Url,
    email: String,
    token: String,
}

impl Client {
    pub fn new(base: Url, email: String, token: String) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("jc/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(ApiError::transport)?;
        Ok(Self { http, base, email, token })
    }

    pub fn base(&self) -> &Url {
        &self.base
    }

    pub async fn request_json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
    ) -> Result<T> {
        let url = self.base.join(path).map_err(ApiError::url)?;
        debug!(%method, %url, "http request");
        let resp = self
            .http
            .request(method, url)
            .basic_auth(&self.email, Some(&self.token))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(ApiError::transport)?;
        parse_response(resp).await
    }

    /// POST a JSON body and parse a JSON response. Used for the new Atlassian
    /// search endpoints which take structured request bodies.
    pub async fn post_json<B, T>(&self, path: &str, body: &B) -> Result<T>
    where
        B: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let url = self.base.join(path).map_err(ApiError::url)?;
        debug!(method = "POST", %url, "http request");
        let resp = self
            .http
            .post(url)
            .basic_auth(&self.email, Some(&self.token))
            .header("Accept", "application/json")
            .json(body)
            .send()
            .await
            .map_err(ApiError::transport)?;
        parse_response(resp).await
    }

    /// PUT a JSON body and parse a JSON response.
    pub async fn put_json<B, T>(&self, path: &str, body: &B) -> Result<T>
    where
        B: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let url = self.base.join(path).map_err(ApiError::url)?;
        debug!(method = "PUT", %url, "http request");
        let resp = self
            .http
            .put(url)
            .basic_auth(&self.email, Some(&self.token))
            .header("Accept", "application/json")
            .json(body)
            .send()
            .await
            .map_err(ApiError::transport)?;
        parse_response(resp).await
    }

    /// POST a JSON body for an endpoint that returns 204 No Content
    /// (e.g. issue transitions).
    pub async fn post_no_content<B>(&self, path: &str, body: &B) -> Result<()>
    where
        B: Serialize + ?Sized,
    {
        let url = self.base.join(path).map_err(ApiError::url)?;
        debug!(method = "POST", %url, "http request");
        let resp = self
            .http
            .post(url)
            .basic_auth(&self.email, Some(&self.token))
            .header("Accept", "application/json")
            .json(body)
            .send()
            .await
            .map_err(ApiError::transport)?;
        parse_empty(resp).await
    }

    /// PUT a JSON body for an endpoint that returns 204 No Content
    /// (e.g. issue edit).
    pub async fn put_no_content<B>(&self, path: &str, body: &B) -> Result<()>
    where
        B: Serialize + ?Sized,
    {
        let url = self.base.join(path).map_err(ApiError::url)?;
        debug!(method = "PUT", %url, "http request");
        let resp = self
            .http
            .put(url)
            .basic_auth(&self.email, Some(&self.token))
            .header("Accept", "application/json")
            .json(body)
            .send()
            .await
            .map_err(ApiError::transport)?;
        parse_empty(resp).await
    }

    /// DELETE an endpoint that returns 204 No Content.
    pub async fn delete_no_content(&self, path: &str) -> Result<()> {
        let url = self.base.join(path).map_err(ApiError::url)?;
        debug!(method = "DELETE", %url, "http request");
        let resp = self
            .http
            .delete(url)
            .basic_auth(&self.email, Some(&self.token))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(ApiError::transport)?;
        parse_empty(resp).await
    }

    /// GET raw bytes from a path. Used for attachment downloads.
    ///
    /// The Atlassian attachment content endpoint issues a 303 redirect to
    /// signed cloud storage. reqwest follows redirects by default and
    /// strips the Authorization header on cross-origin redirects, which
    /// is exactly the behavior we want — the signed URL does its own auth.
    pub async fn download_bytes(&self, path: &str) -> Result<DownloadedBlob> {
        let url = self.base.join(path).map_err(ApiError::url)?;
        debug!(method = "GET", %url, "http download");
        let resp = self
            .http
            .get(url)
            .basic_auth(&self.email, Some(&self.token))
            .send()
            .await
            .map_err(ApiError::transport)?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.bytes().await.map_err(ApiError::transport)?;
            return Err(ApiError::from_response(status, &body));
        }

        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let bytes = resp.bytes().await.map_err(ApiError::transport)?;
        Ok(DownloadedBlob { bytes, content_type })
    }

    /// POST a multipart/form-data body, parsing a JSON response. Sets
    /// `X-Atlassian-Token: no-check` which Atlassian requires for CSRF-
    /// exempt file uploads.
    pub async fn post_multipart<T>(&self, path: &str, form: Form) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let url = self.base.join(path).map_err(ApiError::url)?;
        debug!(method = "POST", %url, "http multipart");
        let resp = self
            .http
            .post(url)
            .basic_auth(&self.email, Some(&self.token))
            .header("Accept", "application/json")
            .header("X-Atlassian-Token", "no-check")
            .multipart(form)
            .send()
            .await
            .map_err(ApiError::transport)?;
        parse_response(resp).await
    }
}

async fn parse_empty(resp: Response) -> Result<()> {
    let status = resp.status();
    let url = resp.url().clone();
    debug!(%status, %url, "http response");
    if status.is_success() {
        Ok(())
    } else {
        let body = resp.bytes().await.map_err(ApiError::transport)?;
        Err(ApiError::from_response(status, &body))
    }
}

async fn parse_response<T: DeserializeOwned>(resp: Response) -> Result<T> {
    let status = resp.status();
    let url = resp.url().clone();
    debug!(%status, %url, "http response");
    let body = resp.bytes().await.map_err(ApiError::transport)?;
    if status.is_success() {
        serde_json::from_slice(&body).map_err(ApiError::decode)
    } else {
        Err(ApiError::from_response(status, &body))
    }
}
