//! OAuth **app** credentials (client id + client secret) for GitHub and Jira.
//!
//! # Practices this follows
//!
//! - **Client secret is never compiled into the binary.** Embedding secrets in release artifacts is
//!   extractable; use process env, launcher-injected env, or a user-local JSON file instead.
//! - **Client id is public** (it appears in authorize URLs). Optional compile-time defaults via
//!   `option_env!` are acceptable for upstream dev convenience; production forks should set env or file.
//! - **Precedence** (highest first): environment variables → [`MOLTHUB_OAUTH_CLIENTS_FILE`] or
//!   config-dir `oauth-clients.json` → optional compile-time client id → baked-in default client id.
//! - **User config file** lives next to `.env` under the OS config directory (`oauth-clients.json`);
//!   keep it **mode 600** on Unix so other users cannot read your client secret.
//!
//! [`MOLTHUB_OAUTH_CLIENTS_FILE`]: env

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;
use tracing::warn;

use super::oauth_common::first_env_trimmed;

/// Env var: absolute path to a JSON file (see [`OAuthClientsFile`]).
pub const ENV_OAUTH_CLIENTS_FILE: &str = "MOLTHUB_OAUTH_CLIENTS_FILE";

/// Default filename under `dirs::config_dir()/molt-hub/`.
pub const OAUTH_CLIENTS_FILENAME: &str = "oauth-clients.json";

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

fn cached_clients_file() -> Option<&'static OAuthClientsFile> {
    static CACHE: OnceLock<Option<OAuthClientsFile>> = OnceLock::new();
    CACHE.get_or_init(try_load_oauth_clients_file).as_ref()
}

/// Parse and cache `oauth-clients.json` once. Call after [`crate::env_bootstrap::load_dotenv_files`]
/// so `MOLTHUB_OAUTH_CLIENTS_FILE` from `.env` is visible.
pub fn warm_oauth_clients_cache() {
    let _ = cached_clients_file();
}

fn try_load_oauth_clients_file() -> Option<OAuthClientsFile> {
    let path = oauth_clients_path()?;
    match load_oauth_clients_from_path(&path) {
        Ok(f) => Some(f),
        Err(e) => {
            warn!(path = %path.display(), error = %e, "failed to load OAuth clients file");
            None
        }
    }
}

fn oauth_clients_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var(ENV_OAUTH_CLIENTS_FILE) {
        let t = p.trim();
        if !t.is_empty() {
            return Some(PathBuf::from(t));
        }
    }
    let base = dirs::config_dir()?
        .join("molt-hub")
        .join(OAUTH_CLIENTS_FILENAME);
    if base.is_file() {
        Some(base)
    } else {
        None
    }
}

/// Load and parse `oauth-clients.json`. Used at startup and in tests.
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
            "oauth clients file is readable by group or others; use chmod 600 (secrets may be exposed)"
        );
    }
}

#[cfg(not(unix))]
fn warn_if_permissions_too_open(_path: &Path) {}

fn file_github() -> &'static OAuthProviderEntry {
    static EMPTY: OAuthProviderEntry = OAuthProviderEntry {
        client_id: None,
        client_secret: None,
    };
    cached_clients_file().map(|f| &f.github).unwrap_or(&EMPTY)
}

fn file_jira() -> &'static OAuthProviderEntry {
    static EMPTY: OAuthProviderEntry = OAuthProviderEntry {
        client_id: None,
        client_secret: None,
    };
    cached_clients_file().map(|f| &f.jira).unwrap_or(&EMPTY)
}

/// Resolve GitHub OAuth app client id and optional client secret.
pub fn github_client_credentials(
    default_client_id: &'static str,
    compile_client_id: Option<&'static str>,
) -> (String, Option<String>) {
    let file = file_github();
    let client_id = first_env_trimmed(&["MOLTHUB_GITHUB_CLIENT_ID", "GITHUB_CLIENT_ID"])
        .or_else(|| file.client_id.clone())
        .or_else(|| compile_client_id.map(str::to_owned))
        .unwrap_or_else(|| default_client_id.to_owned());

    let client_secret =
        first_env_trimmed(&["MOLTHUB_GITHUB_CLIENT_SECRET", "GITHUB_CLIENT_SECRET"])
            .or_else(|| file.client_secret.clone());

    (client_id, client_secret)
}

/// Resolve Jira (Atlassian 3LO) client id and optional client secret.
pub fn jira_client_credentials(
    default_client_id: &'static str,
    compile_client_id: Option<&'static str>,
) -> (String, Option<String>) {
    let file = file_jira();
    let client_id = first_env_trimmed(&["MOLTHUB_JIRA_CLIENT_ID", "JIRA_CLIENT_ID"])
        .or_else(|| file.client_id.clone())
        .or_else(|| compile_client_id.map(str::to_owned))
        .unwrap_or_else(|| default_client_id.to_owned());

    let client_secret = first_env_trimmed(&["MOLTHUB_JIRA_CLIENT_SECRET", "JIRA_CLIENT_SECRET"])
        .or_else(|| file.client_secret.clone());

    (client_id, client_secret)
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
