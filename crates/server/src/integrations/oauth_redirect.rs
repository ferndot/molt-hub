//! OAuth 2.0 redirect URI resolution.
//!
//! Both Jira and GitHub use **HTTPS bridge URLs** from `oauth-bridge/redirect-uris.json`
//! (embedded at compile time). That matches [GitHub’s single callback URL](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps)
//! per OAuth app and keeps one mental model: browser → Pages → `molthub://` → local API.
//!
//! Override without rebuilding: `MOLTHUB_JIRA_REDIRECT_URI`, `MOLTHUB_GITHUB_REDIRECT_URI`.

use std::sync::LazyLock;

use serde::Deserialize;

/// Default port for the Molt Hub API (embedded server and `molt-hub serve`).
pub const DEFAULT_LOCAL_API_PORT: u16 = 13401;

#[derive(Debug, Deserialize, Default)]
struct PublicRedirectUris {
    #[serde(default)]
    jira: Option<String>,
    #[serde(default)]
    github: Option<String>,
}

fn redirect_table() -> &'static PublicRedirectUris {
    static PARSED: LazyLock<PublicRedirectUris> = LazyLock::new(|| {
        const RAW: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oauth-bridge/redirect-uris.json"
        ));
        serde_json::from_str(RAW).unwrap_or_default()
    });
    &*PARSED
}

fn required_https_bridge(field: &'static str, opt: &Option<String>) -> String {
    opt.as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            panic!(
                "oauth-bridge/redirect-uris.json: set a non-empty \"{field}\" URL (HTTPS bridge) \
                 or set MOLTHUB_{}_REDIRECT_URI",
                field.to_uppercase()
            );
        })
}

/// Jira (Atlassian 3LO) redirect URI — must match the developer console entry exactly.
pub fn jira_redirect_uri() -> String {
    if let Ok(v) = std::env::var("MOLTHUB_JIRA_REDIRECT_URI") {
        return v;
    }
    required_https_bridge("jira", &redirect_table().jira)
}

/// GitHub OAuth App redirect URI (single callback per app).
pub fn github_redirect_uri() -> String {
    if let Ok(v) = std::env::var("MOLTHUB_GITHUB_REDIRECT_URI") {
        return v;
    }
    required_https_bridge("github", &redirect_table().github)
}
