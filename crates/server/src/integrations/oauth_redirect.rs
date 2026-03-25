//! OAuth 2.0 redirect URI resolution.
//!
//! Both Jira and GitHub use **HTTPS bridge URLs** from `oauth-bridge/redirect-uris.json`
//! (embedded at compile time). That matches [GitHub’s single callback URL](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps)
//! per OAuth app and keeps one mental model: browser → Pages → `molthub://` → local API.
//!
//! Override without rebuilding: `MOLTHUB_JIRA_REDIRECT_URI`, `MOLTHUB_GITHUB_REDIRECT_URI`.

use std::sync::LazyLock;

use serde::Deserialize;
use tracing::error;

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

fn required_https_bridge(field: &'static str, opt: &Option<String>) -> Option<String> {
    let val = opt.as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if val.is_none() {
        error!(
            "oauth-bridge/redirect-uris.json: set a non-empty \"{field}\" URL (HTTPS bridge) \
             or set MOLTHUB_{}_REDIRECT_URI — OAuth for {field} will be disabled",
            field.to_uppercase()
        );
    }
    val
}

/// Atlassian and GitHub OAuth apps must register a **public** HTTPS callback (see
/// `oauth-bridge/`). Loopback URLs produce `redirect_uri is not registered` from the
/// provider because the bridge URL is what gets sent in `/authorize`, not
/// `http://127.0.0.1:…/api/…/callback` (that path is only hit after `molthub://` deep link).
fn is_loopback_http_oauth_redirect(uri: &str) -> bool {
    let lower = uri.trim().to_ascii_lowercase();
    lower.starts_with("http://127.0.0.1")
        || lower.starts_with("http://localhost")
        || lower.starts_with("http://[::1]")
}

/// Returns `false` and logs an error if `uri` is a loopback HTTP URL (which
/// OAuth providers reject). The caller should treat `false` as "OAuth disabled".
fn reject_loopback_oauth_redirect(provider_env_prefix: &'static str, uri: &str) -> bool {
    let u = uri.trim();
    if is_loopback_http_oauth_redirect(u) {
        error!(
            "{provider_env_prefix}_REDIRECT_URI must not be a loopback HTTP URL (got {u:?}). \
             Unset it to use oauth-bridge/redirect-uris.json, or set it to the same HTTPS \
             bridge URL you registered in the developer console \
             (…/oauth-bridge/jira.html or github.html). \
             OAuth for this provider will be disabled."
        );
        return false;
    }
    true
}

/// Jira (Atlassian 3LO) redirect URI — must match the developer console entry exactly.
///
/// Returns `None` if the URI is missing or invalid (OAuth for Jira will be disabled).
pub fn jira_redirect_uri() -> Option<String> {
    let uri = if let Ok(v) = std::env::var("MOLTHUB_JIRA_REDIRECT_URI") {
        v
    } else {
        required_https_bridge("jira", &redirect_table().jira)?
    };
    if !reject_loopback_oauth_redirect("MOLTHUB_JIRA", &uri) {
        return None;
    }
    Some(uri)
}

/// GitHub OAuth App redirect URI (single callback per app).
///
/// Returns `None` if the URI is missing or invalid (OAuth for GitHub will be disabled).
pub fn github_redirect_uri() -> Option<String> {
    let uri = if let Ok(v) = std::env::var("MOLTHUB_GITHUB_REDIRECT_URI") {
        v
    } else {
        required_https_bridge("github", &redirect_table().github)?
    };
    if !reject_loopback_oauth_redirect("MOLTHUB_GITHUB", &uri) {
        return None;
    }
    Some(uri)
}

#[cfg(test)]
mod tests {
    use super::is_loopback_http_oauth_redirect;

    #[test]
    fn loopback_http_redirect_detected() {
        for bad in [
            "http://127.0.0.1:13401/api/integrations/jira/oauth/callback",
            "http://localhost:13401/cb",
            "  HTTP://LOCALHOST/x  ",
            "http://[::1]:8080/callback",
        ] {
            assert!(
                is_loopback_http_oauth_redirect(bad),
                "expected loopback: {bad:?}"
            );
        }
    }

    #[test]
    fn public_https_redirect_allowed() {
        for ok in [
            "https://ferndot.github.io/molt-hub/oauth-bridge/jira.html",
            "http://192.168.1.10/callback",
        ] {
            assert!(
                !is_loopback_http_oauth_redirect(ok),
                "expected non-loopback: {ok:?}"
            );
        }
    }
}
