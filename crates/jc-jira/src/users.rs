use jc_core::{Client, Result};
use reqwest::Method;

use crate::issue::User;
use crate::types::Myself;

/// GET /rest/api/3/myself — verifies auth and returns the caller's identity.
pub async fn myself(client: &Client) -> Result<Myself> {
    client.request_json(Method::GET, "rest/api/3/myself").await
}

/// GET /rest/api/3/user/search?query=...
///
/// `query` is matched against email address, display name, and accountId.
pub async fn search(client: &Client, query: &str, max: usize) -> Result<Vec<User>> {
    let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    let path = format!("rest/api/3/user/search?query={encoded}&maxResults={max}");
    client.request_json(Method::GET, &path).await
}
