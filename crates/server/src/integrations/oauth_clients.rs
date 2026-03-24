//! OAuth **app** credentials (GitHub + Jira) from a single JSON file.
//!
//! The only supported source is:
//!
//! **`{dirs::config_dir()}/molt-hub/oauth-clients.json`**
//!
//! On first run (non-test builds), if that file is missing, it is created with default public
//! client IDs and empty `client_secret` fields — add secrets for the providers you use, then restart.
//! Use **mode 600** on Unix so other users cannot read the file.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;
use tracing::{info, warn};

/// Default filename under `dirs::config_dir()/molt-hub/`.
pub const OAUTH_CLIENTS_FILENAME: &str = "oauth-clients.json";

/// Built-in GitHub OAuth app id (public); override in JSON if you use your own app.
pub const DEFAULT_GITHUB_OAUTH_CLIENT_ID: &str = "Iv23lip4ZuqkEmT9Z2U0";

/// Built-in Atlassian 3LO client id (public); override in JSON if you use your own app.
pub const DEFAULT_JIRA_OAUTH_CLIENT_ID: &str = "3yQWy34WyjCn0wtOfawofBTMmtK3gUgs";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OAuthClientsFile {
    #[serde(default)]
    pub github: OAuthProviderEntry,
    #[serde(default)]
    pub jira: OAuthProviderEntry,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct OAuthProviderEntry {
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
}

fn trim_opt(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// Canonical path for the OAuth clients file (may not exist yet).
pub fn oauth_clients_json_path() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("molt-hub")
            .join(OAUTH_CLIENTS_FILENAME),
    )
}

fn cached_clients_file() -> Option<&'static OAuthClientsFile> {
    static CACHE: OnceLock<Option<OAuthClientsFile>> = OnceLock::new();
    CACHE.get_or_init(try_load_oauth_clients_file).as_ref()
}

/// Load (and on first run, create) `oauth-clients.json`. Call after [`crate::env_bootstrap::load_dotenv_files`].
pub fn warm_oauth_clients_cache() {
    let _ = cached_clients_file();
}

fn try_load_oauth_clients_file() -> Option<OAuthClientsFile> {
    let path = oauth_clients_json_path()?;
    #[cfg(not(test))]
    {
        if let Err(e) = ensure_oauth_clients_file(&path) {
            warn!(
                path = %path.display(),
                error = %e,
                "could not create oauth-clients.json template"
            );
        }
    }
    if !path.is_file() {
        return None;
    }
    match load_oauth_clients_from_path(&path) {
        Ok(mut f) => {
            if f.github.client_id.is_none() {
                f.github.client_id = Some(DEFAULT_GITHUB_OAUTH_CLIENT_ID.to_string());
            }
            if f.jira.client_id.is_none() {
                f.jira.client_id = Some(DEFAULT_JIRA_OAUTH_CLIENT_ID.to_string());
            }
            Some(f)
        }
        Err(e) => {
            warn!(path = %path.display(), error = %e, "invalid oauth-clients.json");
            None
        }
    }
}

#[cfg(not(test))]
fn ensure_oauth_clients_file(path: &Path) -> std::io::Result<()> {
    if path.is_file() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let template = serde_json::json!({
        "github": {
            "client_id": DEFAULT_GITHUB_OAUTH_CLIENT_ID,
            "client_secret": ""
        },
        "jira": {
            "client_id": DEFAULT_JIRA_OAUTH_CLIENT_ID,
            "client_secret": ""
        }
    });
    let body = serde_json::to_string_pretty(&template)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    std::fs::write(path, body)?;
    info!(
        path = %path.display(),
        "created oauth-clients.json; add client_secret for GitHub and/or Jira, then restart"
    );
    Ok(())
}

/// Load and parse `oauth-clients.json` (tests and tooling).
pub fn load_oauth_clients_from_path(path: &Path) -> Result<OAuthClientsFile, String> {
    #[cfg(unix)]
    warn_if_permissions_too_open(path);

    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut parsed: OAuthClientsFile =
        serde_json::from_str(&raw).map_err(|e| format!("invalid JSON: {e}"))?;
    parsed.github.client_id = trim_opt(parsed.github.client_id.take());
    parsed.github.client_secret = trim_opt(parsed.github.client_secret.take());
    parsed.jira.client_id = trim_opt(parsed.jira.client_id.take());
    parsed.jira.client_secret = trim_opt(parsed.jira.client_secret.take());
    Ok(parsed)
}

#[cfg(unix)]
fn warn_if_permissions_too_open(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        warn!(
            path = %path.display(),
            mode = format!("{mode:o}"),
            "oauth-clients.json should be user-only (chmod 600)"
        );
    }
}

#[cfg(not(unix))]
fn warn_if_permissions_too_open(_path: &Path) {}

/// GitHub OAuth app `client_id` and optional `client_secret` from the JSON file.
pub fn github_client_credentials() -> (String, Option<String>) {
    if let Some(f) = cached_clients_file() {
        let id = f
            .github
            .client_id
            .clone()
            .unwrap_or_else(|| DEFAULT_GITHUB_OAUTH_CLIENT_ID.to_string());
        (id, f.github.client_secret.clone())
    } else {
        (DEFAULT_GITHUB_OAUTH_CLIENT_ID.to_string(), None)
    }
}

/// Jira (Atlassian 3LO) `client_id` and optional `client_secret` from the JSON file.
pub fn jira_client_credentials() -> (String, Option<String>) {
    if let Some(f) = cached_clients_file() {
        let id = f
            .jira
            .client_id
            .clone()
            .unwrap_or_else(|| DEFAULT_JIRA_OAUTH_CLIENT_ID.to_string());
        (id, f.jira.client_secret.clone())
    } else {
        (DEFAULT_JIRA_OAUTH_CLIENT_ID.to_string(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_json() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.json");
        std::fs::write(
            &p,
            r#"{"github":{"client_id":"gh_id","client_secret":"gh_sec"},"jira":{"client_id":"j_id","client_secret":"j_sec"}}"#,
        )
        .unwrap();
        let f = load_oauth_clients_from_path(&p).unwrap();
        assert_eq!(f.github.client_id.as_deref(), Some("gh_id"));
        assert_eq!(f.github.client_secret.as_deref(), Some("gh_sec"));
        assert_eq!(f.jira.client_id.as_deref(), Some("j_id"));
    }

    #[test]
    fn trims_whitespace_in_json() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.json");
        std::fs::write(&p, r#"{"github":{"client_secret":"  x  "}}"#).unwrap();
        let f = load_oauth_clients_from_path(&p).unwrap();
        assert_eq!(f.github.client_secret.as_deref(), Some("x"));
    }
}
