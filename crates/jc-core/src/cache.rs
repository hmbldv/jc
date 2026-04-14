//! Local cache under `~/.cache/jc/`.
//!
//! Pure cache, never authoritative. Rebuildable at any time. The primary
//! use is the Jira custom-field name↔ID map — `customfield_10042` means
//! nothing to a human, but `"Story Points"` does, and Jira forces us to
//! do the translation ourselves.

use std::io;
use std::path::PathBuf;

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Resolved cache directory — `$XDG_CACHE_HOME/jc` on Linux,
/// `~/Library/Caches/jc` on macOS, etc. Returns `None` if the platform
/// cache directory is not discoverable.
pub fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("jc"))
}

/// Read and deserialize a cache file by name. Returns `None` if the file
/// is missing or fails to parse — the caller should treat that as a cold
/// cache and refresh from the API.
pub fn read_json<T: DeserializeOwned>(name: &str) -> Option<T> {
    let path = cache_dir()?.join(name);
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Serialize a value and write it to the named cache file, creating the
/// cache directory if necessary.
pub fn write_json<T: Serialize>(name: &str, value: &T) -> io::Result<PathBuf> {
    let dir = cache_dir().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "cache directory not discoverable")
    })?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, json)?;
    Ok(path)
}
