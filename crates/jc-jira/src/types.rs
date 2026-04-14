use serde::Deserialize;

/// Response shape for GET /rest/api/3/myself.
/// Used by `jc config test` to verify auth end-to-end.
#[derive(Debug, Deserialize)]
pub struct Myself {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "emailAddress", default)]
    pub email_address: Option<String>,
    #[serde(default)]
    pub active: bool,
}
