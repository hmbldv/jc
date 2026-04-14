use jc_core::{Client, Result};
use reqwest::Method;

use crate::types::Myself;

/// GET /rest/api/3/myself — verifies auth and returns the caller's identity.
pub async fn myself(client: &Client) -> Result<Myself> {
    client.request_json(Method::GET, "rest/api/3/myself").await
}
