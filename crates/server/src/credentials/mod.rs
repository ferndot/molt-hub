//! Alias-based, pipeline-scoped credential storage.
//!
//! Credentials are stored under a structured key: `molt-hub:{scope}:{alias}`.
//! The scope controls visibility: global credentials are accessible to all pipelines,
//! pipeline-scoped credentials are isolated per pipeline, and stage-scoped credentials
//! are further restricted to a single stage within a pipeline.
//!
//! Two implementations are provided:
//! - [`KeyringStore`] — backed by the OS keychain (macOS Keychain, Linux Secret Service,
//!   Windows Credential Manager) via the `keyring` crate.
//! - [`MemoryStore`] — in-memory HashMap, suitable for tests.
//!
//! File descriptors can be injected into agent processes via [`fd_inject`].

pub mod alias_index;
pub mod fd_inject;
pub mod keyring_store;
pub mod memory_store;

pub use alias_index::CredentialAliasIndex;
pub use fd_inject::inject_credential;
pub use keyring_store::KeyringStore;
pub use memory_store::MemoryStore;

use molt_hub_core::model::ProjectId;
use std::fmt;
use thiserror::Error;
use ulid::Ulid;

// ─── CredentialScope ─────────────────────────────────────────────────────────

/// Scoping context for a stored credential.
///
/// The scope determines who can read the credential and how the storage key
/// is formed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CredentialScope {
    /// Accessible from any pipeline.
    Global,
    /// Scoped to a monitored project (integrations, per-repo settings).
    Project(ProjectId),
    /// Scoped to a named pipeline.
    Pipeline(String),
    /// Scoped to a specific stage within a pipeline.
    Stage(String, String),
}

impl fmt::Display for CredentialScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CredentialScope::Global => write!(f, "global"),
            CredentialScope::Project(id) => write!(f, "project:{id}"),
            CredentialScope::Pipeline(name) => write!(f, "pipeline:{name}"),
            CredentialScope::Stage(pipeline, stage) => write!(f, "stage:{pipeline}:{stage}"),
        }
    }
}

/// Resolve integration credential scope from an optional `projectId` query param.
///
/// Missing, empty, `default`, or invalid ULIDs map to [`CredentialScope::Global`] so existing
/// clients keep working; valid project ULIDs use [`CredentialScope::Project`].
pub fn credential_scope_for_integration(project_id_param: Option<&str>) -> CredentialScope {
    let Some(raw) = project_id_param.map(str::trim).filter(|s| !s.is_empty()) else {
        return CredentialScope::Global;
    };
    if raw == "default" {
        return CredentialScope::Global;
    }
    match Ulid::from_string(raw) {
        Ok(u) => CredentialScope::Project(ProjectId(u)),
        Err(_) => CredentialScope::Global,
    }
}

// ─── CredentialError ─────────────────────────────────────────────────────────

/// Errors returned by [`CredentialStore`] operations.
#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("credential not found: {alias} (scope: {scope})")]
    NotFound { alias: String, scope: String },

    #[error("keychain error: {0}")]
    Keychain(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid alias '{0}': must not be empty")]
    InvalidAlias(String),
}

// ─── CredentialStore trait ────────────────────────────────────────────────────

/// Common interface for all credential storage backends.
///
/// Implementations must be `Send + Sync` so they can be shared across async tasks.
pub trait CredentialStore: Send + Sync + 'static {
    /// Store a credential under the given alias and scope.
    ///
    /// If a credential with the same alias and scope already exists it is overwritten.
    fn store(
        &self,
        alias: &str,
        scope: &CredentialScope,
        value: &str,
    ) -> Result<(), CredentialError>;

    /// Retrieve the credential value for the given alias and scope.
    fn retrieve(&self, alias: &str, scope: &CredentialScope) -> Result<String, CredentialError>;

    /// Delete the credential for the given alias and scope.
    ///
    /// Returns `Ok(())` even when the credential did not exist (idempotent).
    fn delete(&self, alias: &str, scope: &CredentialScope) -> Result<(), CredentialError>;

    /// List all alias names stored under the given scope.
    fn list(&self, scope: &CredentialScope) -> Result<Vec<String>, CredentialError>;
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Build the storage key string for a given alias and scope.
///
/// Format: `molt-hub:{scope}:{alias}`
pub(crate) fn storage_key(alias: &str, scope: &CredentialScope) -> String {
    format!("molt-hub:{scope}:{alias}")
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_display_global() {
        assert_eq!(CredentialScope::Global.to_string(), "global");
    }

    #[test]
    fn scope_display_pipeline() {
        assert_eq!(
            CredentialScope::Pipeline("ci".into()).to_string(),
            "pipeline:ci"
        );
    }

    #[test]
    fn scope_display_stage() {
        assert_eq!(
            CredentialScope::Stage("ci".into(), "build".into()).to_string(),
            "stage:ci:build"
        );
    }

    #[test]
    fn scope_display_project() {
        let id = ProjectId::new();
        assert_eq!(
            CredentialScope::Project(id.clone()).to_string(),
            format!("project:{id}")
        );
    }

    #[test]
    fn credential_scope_for_integration_defaults() {
        assert!(matches!(
            credential_scope_for_integration(None),
            CredentialScope::Global
        ));
        assert!(matches!(
            credential_scope_for_integration(Some("")),
            CredentialScope::Global
        ));
        assert!(matches!(
            credential_scope_for_integration(Some("default")),
            CredentialScope::Global
        ));
        assert!(matches!(
            credential_scope_for_integration(Some("not-a-ulid")),
            CredentialScope::Global
        ));
    }

    #[test]
    fn credential_scope_for_integration_parses_ulid() {
        let id = ProjectId::new();
        let s = id.to_string();
        match credential_scope_for_integration(Some(&s)) {
            CredentialScope::Project(p) => assert_eq!(p, id),
            _ => panic!("expected Project scope"),
        }
    }

    #[test]
    fn storage_key_format() {
        let key = storage_key("MY_TOKEN", &CredentialScope::Pipeline("ci".into()));
        assert_eq!(key, "molt-hub:pipeline:ci:MY_TOKEN");
    }

    #[test]
    fn storage_key_global() {
        let key = storage_key("API_KEY", &CredentialScope::Global);
        assert_eq!(key, "molt-hub:global:API_KEY");
    }
}
