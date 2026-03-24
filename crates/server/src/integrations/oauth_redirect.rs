//! OAuth 2.0 redirect URI resolution.
//!
//! - **Debug builds** always use the local Axum callback on `127.0.0.1` (PKCE + dev OAuth apps).
//! - **Release builds** use HTTPS URLs from `oauth-bridge/redirect-uris.json` (embedded at compile time)
//!   when `jira` / `github` are set, so end users do not need environment variables.
//! - Optional overrides: `MOLTHUB_JIRA_REDIRECT_URI`, `MOLTHUB_GITHUB_REDIRECT_URI` (e.g. CI or experiments).

/// Default port for the Molt Hub API (embedded server and `molt-hub serve`).
pub const DEFAULT_LOCAL_API_PORT: u16 = 13401;

fn default_local_origin() -> String {
    let port: u16 = std::env::var("MOLTHUB_LOCAL_API_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_LOCAL_API_PORT);
    format!("http://127.0.0.1:{port}")
}

fn local_jira_callback() -> String {
    format!(
        "{}/api/integrations/jira/oauth/callback",
        default_local_origin()
    )
}

fn local_github_callback() -> String {
    format!(
        "{}/api/integrations/github/oauth/callback",
        default_local_origin()
    )
}

#[cfg(not(debug_assertions))]
mod embedded {
    use std::sync::LazyLock;

    use serde::Deserialize;

    #[derive(Debug, Deserialize, Default)]
    struct PublicRedirectUris {
        #[serde(default)]
        jira: Option<String>,
        #[serde(default)]
        github: Option<String>,
    }

    fn table() -> &'static PublicRedirectUris {
        static PARSED: LazyLock<PublicRedirectUris> = LazyLock::new(|| {
            const RAW: &str = include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../oauth-bridge/redirect-uris.json"
            ));
            serde_json::from_str(RAW).unwrap_or_default()
        });
        &*PARSED
    }

    fn pick_public_or_local(explicit: Option<&String>, local: String) -> String {
        explicit
            .map(|s| s.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or(local)
    }

    pub(super) fn jira(local: String) -> String {
        pick_public_or_local(table().jira.as_ref(), local)
    }

    pub(super) fn github(local: String) -> String {
        pick_public_or_local(table().github.as_ref(), local)
    }
}

/// Jira (Atlassian 3LO) redirect URI — must match the developer console entry exactly.
pub fn jira_redirect_uri() -> String {
    if let Ok(v) = std::env::var("MOLTHUB_JIRA_REDIRECT_URI") {
        return v;
    }
    let local = local_jira_callback();
    #[cfg(debug_assertions)]
    {
        return local;
    }
    #[cfg(not(debug_assertions))]
    {
        embedded::jira(local)
    }
}

/// GitHub OAuth App redirect URI.
pub fn github_redirect_uri() -> String {
    if let Ok(v) = std::env::var("MOLTHUB_GITHUB_REDIRECT_URI") {
        return v;
    }
    let local = local_github_callback();
    #[cfg(debug_assertions)]
    {
        return local;
    }
    #[cfg(not(debug_assertions))]
    {
        embedded::github(local)
    }
}
