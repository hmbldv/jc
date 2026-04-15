use bytes::Bytes;
use reqwest::multipart::Form;
use reqwest::redirect::Policy;
use reqwest::{Method, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tracing::debug;
use url::Url;

use crate::error::{ApiError, Result};
use crate::retry::{RetryPolicy, send_with_retry};

/// Binary response with content-type metadata. Used for attachment downloads.
#[derive(Debug)]
pub struct DownloadedBlob {
    pub bytes: Bytes,
    pub content_type: Option<String>,
}

/// Cap for non-download response bodies (JSON and error bodies). Far
/// larger than any legitimate Atlassian response; prevents a hostile
/// endpoint from OOMing the process with a chunked, content-length-less
/// stream.
const RESPONSE_BODY_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Client {
    http: reqwest::Client,
    base: Url,
    email: String,
    token: String,
}

impl Client {
    pub fn new(base: Url, email: String, token: String) -> Result<Self> {
        // Explicit redirect policy: up to 10 hops, reqwest strips sensitive
        // headers (Authorization, Cookie, Proxy-Authorization) on cross-
        // origin redirects by default — we lock that behavior in explicitly
        // so an upstream dependency change can't silently leak basic auth
        // to S3 on the attachment download flow.
        let http = reqwest::Client::builder()
            .user_agent(concat!("jc/", env!("CARGO_PKG_VERSION")))
            .redirect(Policy::limited(10))
            .https_only(true)
            .build()
            .map_err(ApiError::transport)?;
        Ok(Self {
            http,
            base,
            email,
            token,
        })
    }

    pub fn base(&self) -> &Url {
        &self.base
    }

    /// Pick a retry policy appropriate for the given HTTP method. GETs
    /// and HEADs are idempotent and safe to replay on read-side 5xx;
    /// everything else retries only on 429, which Atlassian guarantees
    /// means the request was rejected before processing.
    fn policy_for(method: &Method) -> RetryPolicy {
        if method == Method::GET || method == Method::HEAD {
            RetryPolicy::Read
        } else {
            RetryPolicy::IdempotencySafe
        }
    }

    pub async fn request_json<T: DeserializeOwned>(&self, method: Method, path: &str) -> Result<T> {
        let url = self.base.join(path).map_err(ApiError::url)?;
        trace_request(method.as_str(), &url);
        let policy = Self::policy_for(&method);
        let resp = send_with_retry(
            || {
                self.http
                    .request(method.clone(), url.clone())
                    .basic_auth(&self.email, Some(&self.token))
                    .header("Accept", "application/json")
            },
            policy,
        )
        .await?;
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
        trace_request("POST", &url);
        let resp = send_with_retry(
            || {
                self.http
                    .post(url.clone())
                    .basic_auth(&self.email, Some(&self.token))
                    .header("Accept", "application/json")
                    .json(body)
            },
            RetryPolicy::IdempotencySafe,
        )
        .await?;
        parse_response(resp).await
    }

    /// PUT a JSON body and parse a JSON response.
    pub async fn put_json<B, T>(&self, path: &str, body: &B) -> Result<T>
    where
        B: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let url = self.base.join(path).map_err(ApiError::url)?;
        trace_request("PUT", &url);
        let resp = send_with_retry(
            || {
                self.http
                    .put(url.clone())
                    .basic_auth(&self.email, Some(&self.token))
                    .header("Accept", "application/json")
                    .json(body)
            },
            RetryPolicy::IdempotencySafe,
        )
        .await?;
        parse_response(resp).await
    }

    /// POST a JSON body for an endpoint that returns 204 No Content
    /// (e.g. issue transitions).
    pub async fn post_no_content<B>(&self, path: &str, body: &B) -> Result<()>
    where
        B: Serialize + ?Sized,
    {
        let url = self.base.join(path).map_err(ApiError::url)?;
        trace_request("POST", &url);
        let resp = send_with_retry(
            || {
                self.http
                    .post(url.clone())
                    .basic_auth(&self.email, Some(&self.token))
                    .header("Accept", "application/json")
                    .json(body)
            },
            RetryPolicy::IdempotencySafe,
        )
        .await?;
        parse_empty(resp).await
    }

    /// PUT a JSON body for an endpoint that returns 204 No Content
    /// (e.g. issue edit).
    pub async fn put_no_content<B>(&self, path: &str, body: &B) -> Result<()>
    where
        B: Serialize + ?Sized,
    {
        let url = self.base.join(path).map_err(ApiError::url)?;
        trace_request("PUT", &url);
        let resp = send_with_retry(
            || {
                self.http
                    .put(url.clone())
                    .basic_auth(&self.email, Some(&self.token))
                    .header("Accept", "application/json")
                    .json(body)
            },
            RetryPolicy::IdempotencySafe,
        )
        .await?;
        parse_empty(resp).await
    }

    /// DELETE an endpoint that returns 204 No Content.
    pub async fn delete_no_content(&self, path: &str) -> Result<()> {
        let url = self.base.join(path).map_err(ApiError::url)?;
        trace_request("DELETE", &url);
        let resp = send_with_retry(
            || {
                self.http
                    .delete(url.clone())
                    .basic_auth(&self.email, Some(&self.token))
                    .header("Accept", "application/json")
            },
            RetryPolicy::IdempotencySafe,
        )
        .await?;
        parse_empty(resp).await
    }

    /// GET raw bytes from a path. Used for attachment downloads.
    ///
    /// The Atlassian attachment content endpoint issues a 303 redirect to
    /// signed cloud storage. reqwest follows redirects (limit 10, hardened
    /// by the explicit Policy::limited(10) above) and strips the
    /// Authorization header on cross-origin redirects so the Atlassian
    /// basic auth never reaches the signed storage URL.
    pub async fn download_bytes(&self, path: &str) -> Result<DownloadedBlob> {
        let url = self.base.join(path).map_err(ApiError::url)?;
        trace_request("GET", &url);
        let resp = send_with_retry(
            || {
                self.http
                    .get(url.clone())
                    .basic_auth(&self.email, Some(&self.token))
            },
            RetryPolicy::Read,
        )
        .await?;

        let status = resp.status();
        trace_response(status, resp.url());
        if !status.is_success() {
            let body = read_bounded(resp, RESPONSE_BODY_LIMIT).await?;
            return Err(ApiError::from_response(status, &body));
        }

        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        // Downloads are intentionally unbounded — users explicitly ask for
        // the bytes of an attachment they chose. The memory cost is paid
        // by the caller, not a hostile third party.
        let bytes = resp.bytes().await.map_err(ApiError::transport)?;
        Ok(DownloadedBlob {
            bytes,
            content_type,
        })
    }

    /// POST a multipart/form-data body, parsing a JSON response. Sets
    /// `X-Atlassian-Token: no-check` which Atlassian requires for CSRF-
    /// exempt file uploads.
    ///
    /// This path is NOT retried because `reqwest::multipart::Form` is
    /// move-consumed on send and cannot be rebuilt without re-reading
    /// the source file. A rate-limited upload surfaces as a 429 that
    /// the caller must handle.
    pub async fn post_multipart<T>(&self, path: &str, form: Form) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let url = self.base.join(path).map_err(ApiError::url)?;
        trace_request("POST", &url);
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

/// Log an outgoing HTTP request. Strips query string since signed-URL
/// redirect targets and other sensitive tokens can live in query params.
fn trace_request(method: &str, url: &Url) {
    let scrubbed = scrub_url(url);
    debug!(method, url = %scrubbed, "http request");
}

fn trace_response(status: reqwest::StatusCode, url: &Url) {
    let scrubbed = scrub_url(url);
    debug!(%status, url = %scrubbed, "http response");
}

fn scrub_url(url: &Url) -> String {
    let mut s = String::new();
    s.push_str(url.scheme());
    s.push_str("://");
    if let Some(host) = url.host_str() {
        s.push_str(host);
    }
    if let Some(port) = url.port() {
        s.push(':');
        s.push_str(&port.to_string());
    }
    s.push_str(url.path());
    if url.query().is_some() {
        s.push_str("?<redacted>");
    }
    s
}

async fn parse_empty(resp: Response) -> Result<()> {
    let status = resp.status();
    trace_response(status, resp.url());
    if status.is_success() {
        Ok(())
    } else {
        let body = read_bounded(resp, RESPONSE_BODY_LIMIT).await?;
        Err(ApiError::from_response(status, &body))
    }
}

async fn parse_response<T: DeserializeOwned>(resp: Response) -> Result<T> {
    let status = resp.status();
    trace_response(status, resp.url());
    let body = read_bounded(resp, RESPONSE_BODY_LIMIT).await?;
    if status.is_success() {
        serde_json::from_slice(&body).map_err(ApiError::decode)
    } else {
        Err(ApiError::from_response(status, &body))
    }
}

/// Read a response body in chunks, refusing to buffer more than `limit`
/// bytes total. Guards against hostile or buggy servers streaming
/// unbounded bodies without a `content-length` header.
async fn read_bounded(mut resp: Response, limit: usize) -> Result<Bytes> {
    // Fast-fail when the server declares an oversized body up front.
    if let Some(declared) = resp.content_length()
        && declared > limit as u64
    {
        return Err(ApiError::config(format!(
            "response body declared {declared} bytes, exceeds {limit} cap"
        )));
    }

    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(ApiError::transport)? {
        if buf.len() + chunk.len() > limit {
            return Err(ApiError::config(format!(
                "response body exceeded {limit}-byte cap"
            )));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_url_strips_query() {
        let url = Url::parse("https://example.com/path?sig=secret&exp=123").unwrap();
        assert_eq!(scrub_url(&url), "https://example.com/path?<redacted>");
    }

    #[test]
    fn scrub_url_preserves_path() {
        let url = Url::parse("https://example.com/a/b/c").unwrap();
        assert_eq!(scrub_url(&url), "https://example.com/a/b/c");
    }

    #[test]
    fn scrub_url_preserves_port() {
        let url = Url::parse("https://example.com:8443/path").unwrap();
        assert_eq!(scrub_url(&url), "https://example.com:8443/path");
    }

    #[test]
    fn policy_for_get_is_read() {
        assert_eq!(Client::policy_for(&Method::GET), RetryPolicy::Read);
        assert_eq!(Client::policy_for(&Method::HEAD), RetryPolicy::Read);
    }

    #[test]
    fn policy_for_mutation_is_idempotency_safe() {
        assert_eq!(
            Client::policy_for(&Method::POST),
            RetryPolicy::IdempotencySafe
        );
        assert_eq!(
            Client::policy_for(&Method::PUT),
            RetryPolicy::IdempotencySafe
        );
        assert_eq!(
            Client::policy_for(&Method::DELETE),
            RetryPolicy::IdempotencySafe
        );
        assert_eq!(
            Client::policy_for(&Method::PATCH),
            RetryPolicy::IdempotencySafe
        );
    }
}
