use jc_core::{ApiError, Client, Result};
use url::Url;

/// Namespaced keyring service name. The `dev.hmbldv.jc` form is stable
/// and unlikely to collide with another tool also claiming "jc".
const KEYRING_SERVICE: &str = "dev.hmbldv.jc";

/// Resolved runtime config. Populated from env vars first, keychain fallback.
#[derive(Debug, Clone)]
pub struct Config {
    pub site: String,
    pub email: String,
    pub token: String,
}

impl Config {
    /// Load config, preferring env vars and falling back to the OS keychain.
    ///
    /// Required fields: `site`, `email`, `token`.
    /// - Env vars:    `JC_SITE`,  `JC_EMAIL`,  `JC_TOKEN`
    /// - Keychain:    service `jc`, accounts `site` / `email` / `token`
    pub fn from_env() -> Result<Self> {
        let site = load_field("site", "JC_SITE")?;
        let email = load_field("email", "JC_EMAIL")?;
        let token = load_field("token", "JC_TOKEN")?;
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
            "source": config_source(),
        })
    }
}

fn load_field(key: &str, env_var: &str) -> Result<String> {
    if let Ok(v) = std::env::var(env_var) {
        if !v.is_empty() {
            return Ok(v);
        }
    }
    match read_keychain(key) {
        Some(v) if !v.is_empty() => Ok(v),
        _ => Err(ApiError::config(format!(
            "{env_var} not set (checked env var and keychain entry `{KEYRING_SERVICE}/{key}`)"
        ))),
    }
}

fn read_keychain(account: &str) -> Option<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, account).ok()?;
    entry.get_password().ok()
}

pub fn write_keychain(account: &str, value: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, account)
        .map_err(|e| ApiError::config(format!("keyring open: {e}")))?;
    entry
        .set_password(value)
        .map_err(|e| ApiError::config(format!("keyring set: {e}")))
}

fn config_source() -> Vec<&'static str> {
    let mut sources = Vec::new();
    if std::env::var_os("JC_SITE").is_some()
        || std::env::var_os("JC_EMAIL").is_some()
        || std::env::var_os("JC_TOKEN").is_some()
    {
        sources.push("env");
    }
    if keyring::Entry::new(KEYRING_SERVICE, "token")
        .ok()
        .and_then(|e| e.get_password().ok())
        .is_some()
    {
        sources.push("keychain");
    }
    sources
}
