//! Integration plugin interface — the MoltIntegration trait and shared types.
//!
//! This module defines the extension point for external system integrations
//! (Jira, GitHub, Webhooks, etc.). Each integration implements the
//! `MoltIntegration` trait; concrete implementations live in sub-modules.

pub mod config;
pub mod jira;

use crate::model::TaskId;
use config::{ExternalItem, IntegrationConfig, SyncStatus};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during integration operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum IntegrationError {
    #[error("authentication failed: {0}")]
    AuthFailed(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("not found: {external_id}")]
    NotFound { external_id: String },

    #[error("unsupported operation: {0}")]
    Unsupported(String),

    #[error("integration error: {0}")]
    Other(String),
}

// ---------------------------------------------------------------------------
// HealthStatus
// ---------------------------------------------------------------------------

/// Overall health of an integration connection.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum HealthStatus {
    /// Integration is reachable and authenticated.
    Healthy,
    /// Integration is reachable but partially degraded.
    Degraded { reason: String },
    /// Integration cannot be reached or is not authenticated.
    Unhealthy { reason: String },
}

// ---------------------------------------------------------------------------
// MoltIntegration trait
// ---------------------------------------------------------------------------

/// The plugin interface for external system integrations.
///
/// Implementors connect Molt Hub to external issue trackers, CI systems, or
/// custom webhooks.  All methods are synchronous at this layer; async runtimes
/// are the responsibility of the server crate.
pub trait MoltIntegration: Send + Sync {
    /// Authenticate with the external system using the provided config.
    ///
    /// Should be called once at startup (or when config changes).
    fn authenticate(&self, config: &IntegrationConfig) -> Result<(), IntegrationError>;

    /// Fetch items matching the given query string.
    ///
    /// Query syntax is integration-specific (JQL for Jira, GQL for GitHub,
    /// URL path+params for webhooks, etc.).
    fn fetch_items(&self, query: &str) -> Result<Vec<ExternalItem>, IntegrationError>;

    /// Check the current sync status of a specific task against an external item.
    fn sync_status(
        &self,
        task_id: TaskId,
        external_id: &str,
    ) -> Result<SyncStatus, IntegrationError>;

    /// Perform a lightweight connectivity and auth check.
    fn health_check(&self) -> Result<HealthStatus, IntegrationError>;
}
