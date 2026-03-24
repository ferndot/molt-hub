//! Optional `projectId` query/body fields (camelCase) for integration OAuth + REST.

use serde::Deserialize;

use crate::credentials::{credential_scope_for_integration, CredentialScope};

/// Query: `?projectId=<ulid>` — scopes OAuth tokens and REST clients to a monitored project.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProjectIdQuery {
    #[serde(default, rename = "projectId")]
    pub project_id: Option<String>,
}

impl ProjectIdQuery {
    pub fn credential_scope(&self) -> CredentialScope {
        credential_scope_for_integration(self.project_id.as_deref())
    }
}
