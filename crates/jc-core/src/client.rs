use reqwest::{Method, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::error::{ApiError, Result};

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
}

async fn parse_response<T: DeserializeOwned>(resp: Response) -> Result<T> {
    let status = resp.status();
    let body = resp.bytes().await.map_err(ApiError::transport)?;
    if status.is_success() {
        serde_json::from_slice(&body).map_err(ApiError::decode)
    } else {
        Err(ApiError::from_response(status, &body))
    }
}
