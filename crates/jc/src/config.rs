use jc_core::{ApiError, Client, Result};
use url::Url;

/// Resolved runtime config. Populated from env vars first, keychain fallback.
#[derive(Debug, Clone)]
pub struct Config {
    pub site: String,
    pub email: String,
    pub token: String,
}

impl Config {
    /// Load config from env vars. Keychain fallback is stubbed for now.
    ///
    /// Required env vars:
    /// - `JC_SITE`  (e.g. `your-org.atlassian.net`)
    /// - `JC_EMAIL` (your Atlassian account email)
    /// - `JC_TOKEN` (API token from id.atlassian.com)
    pub fn from_env() -> Result<Self> {
        let site = std::env::var("JC_SITE")
            .map_err(|_| ApiError::config("JC_SITE not set"))?;
        let email = std::env::var("JC_EMAIL")
            .map_err(|_| ApiError::config("JC_EMAIL not set"))?;
        let token = std::env::var("JC_TOKEN")
            .map_err(|_| ApiError::config("JC_TOKEN not set"))?;
        Ok(Self { site, email, token })
    }

    pub fn jira_client(&self) -> Result<Client> {
        let base = Url::parse(&format!("https://{}/", self.site))
            .map_err(ApiError::url)?;
        Client::new(base, self.email.clone(), self.token.clone())
    }

    /// Redacted form for `jc config show`.
    pub fn redacted_json(&self) -> serde_json::Value {
        serde_json::json!({
            "site": self.site,
            "email": self.email,
            "token": "***",
        })
    }
}
