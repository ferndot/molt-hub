//! Project domain entity — first-class representation of a monitored codebase.
//!
//! A `Project` groups pipelines, tasks, and agents for a single repository.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::ProjectId;

// ---------------------------------------------------------------------------
// ProjectStatus
// ---------------------------------------------------------------------------

/// Lifecycle state of a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    /// Project is actively monitored.
    Active,
    /// Project has been soft-deleted / archived.
    Archived,
}

impl Default for ProjectStatus {
    fn default() -> Self {
        Self::Active
    }
}

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

/// A monitored repository / codebase managed by Molt Hub.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub repo_path: PathBuf,
    pub status: ProjectStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Errors that can occur when creating or updating a project.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProjectValidationError {
    #[error("project name must not be empty")]
    EmptyName,
    #[error("repo_path does not exist: {0}")]
    RepoPathNotFound(PathBuf),
}

impl Project {
    /// Validate that the project's fields are consistent.
    ///
    /// - Name must be non-empty.
    /// - `repo_path` must point to an existing directory (skipped when
    ///   `check_path` is false, e.g. during deserialization from store).
    pub fn validate(&self, check_path: bool) -> Result<(), ProjectValidationError> {
        if self.name.trim().is_empty() {
            return Err(ProjectValidationError::EmptyName);
        }
        if check_path && !self.repo_path.exists() {
            return Err(ProjectValidationError::RepoPathNotFound(
                self.repo_path.clone(),
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProjectId;
    use chrono::Utc;

    fn sample_project() -> Project {
        Project {
            id: ProjectId::new(),
            name: "my-app".into(),
            repo_path: PathBuf::from("/tmp"),
            status: ProjectStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn valid_project_passes_validation() {
        let p = sample_project();
        assert!(p.validate(true).is_ok());
    }

    #[test]
    fn empty_name_fails_validation() {
        let mut p = sample_project();
        p.name = "".into();
        assert_eq!(
            p.validate(false).unwrap_err(),
            ProjectValidationError::EmptyName
        );
    }

    #[test]
    fn whitespace_name_fails_validation() {
        let mut p = sample_project();
        p.name = "   ".into();
        assert_eq!(
            p.validate(false).unwrap_err(),
            ProjectValidationError::EmptyName
        );
    }

    #[test]
    fn nonexistent_path_fails_when_checked() {
        let mut p = sample_project();
        p.repo_path = PathBuf::from("/nonexistent/path/abc123");
        assert!(matches!(
            p.validate(true).unwrap_err(),
            ProjectValidationError::RepoPathNotFound(_)
        ));
    }

    #[test]
    fn nonexistent_path_passes_when_not_checked() {
        let mut p = sample_project();
        p.repo_path = PathBuf::from("/nonexistent/path/abc123");
        assert!(p.validate(false).is_ok());
    }

    #[test]
    fn project_status_default_is_active() {
        assert_eq!(ProjectStatus::default(), ProjectStatus::Active);
    }

    #[test]
    fn project_serialises_to_json() {
        let p = sample_project();
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("my-app"));
        assert!(json.contains("active"));
    }

    #[test]
    fn project_roundtrips_json() {
        let p = sample_project();
        let json = serde_json::to_string(&p).unwrap();
        let p2: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(p2.name, p.name);
        assert_eq!(p2.status, p.status);
    }
}
