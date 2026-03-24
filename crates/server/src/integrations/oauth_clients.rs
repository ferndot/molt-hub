//! OAuth **app** credentials (GitHub + Jira) are **fixed at compile time**.
//!
//! Set variables in the environment when you run **`cargo build`** / **`cargo tauri build`**, or put
//! them in a **`.env`** file anywhere from `crates/server/` up to the repository root (see
//! `build.rs` — earlier `.env` files on the path win per key).
//!
//! | Variable | Purpose |
//! |----------|---------|
//! | `GITHUB_CLIENT_ID` / `MOLTHUB_GITHUB_CLIENT_ID` | Optional; defaults to the upstream public app id |
//! | `GITHUB_CLIENT_SECRET` / `MOLTHUB_GITHUB_CLIENT_SECRET` | Required for GitHub token exchange |
//! | `JIRA_CLIENT_ID` / `MOLTHUB_JIRA_CLIENT_ID` | Optional; defaults to the upstream public app id |
//! | `JIRA_CLIENT_SECRET` / `MOLTHUB_JIRA_CLIENT_SECRET` | Required for Atlassian 3LO token exchange |

include!(concat!(env!("OUT_DIR"), "/oauth_clients_embed.rs"));

/// Same as [`BUILT_GITHUB_CLIENT_ID`] (for re-exports and tests).
pub const DEFAULT_GITHUB_OAUTH_CLIENT_ID: &str = BUILT_GITHUB_CLIENT_ID;

/// Same as [`BUILT_JIRA_CLIENT_ID`].
pub const DEFAULT_JIRA_OAUTH_CLIENT_ID: &str = BUILT_JIRA_CLIENT_ID;

/// GitHub OAuth app credentials baked into this binary.
pub fn github_client_credentials() -> (String, Option<String>) {
    (
        BUILT_GITHUB_CLIENT_ID.to_string(),
        BUILT_GITHUB_CLIENT_SECRET.map(String::from),
    )
}

/// Jira (Atlassian 3LO) app credentials baked into this binary.
pub fn jira_client_credentials() -> (String, Option<String>) {
    (
        BUILT_JIRA_CLIENT_ID.to_string(),
        BUILT_JIRA_CLIENT_SECRET.map(String::from),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_ids_are_non_empty() {
        assert!(!BUILT_GITHUB_CLIENT_ID.is_empty());
        assert!(!BUILT_JIRA_CLIENT_ID.is_empty());
    }
}
