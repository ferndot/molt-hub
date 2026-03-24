//! Per-project runtime state — supervisor, pipeline config, and event store.
//!
//! [`ProjectRuntime`] holds the live state for a single project.
//! [`ProjectRuntimeRegistry`] is an in-memory map of `project_id → runtime`
//! and is injected into Axum handlers via `axum::Extension`.

use std::sync::Arc;

use dashmap::DashMap;

use molt_hub_harness::supervisor::Supervisor;

use crate::pipeline::handlers::PipelineConfigStore;

// ---------------------------------------------------------------------------
// ProjectRuntime
// ---------------------------------------------------------------------------

/// Live runtime state for a single project.
pub struct ProjectRuntime {
    /// The project's string identifier (matches the ULID stored in the config).
    pub project_id: String,
    /// Supervisor managing agents spawned under this project.
    pub supervisor: Arc<Supervisor>,
    /// Pipeline stage configuration for this project.
    pub pipeline_config: Arc<PipelineConfigStore>,
}

// ---------------------------------------------------------------------------
// ProjectRuntimeRegistry
// ---------------------------------------------------------------------------

/// Registry of all active project runtimes, keyed by `project_id`.
///
/// Inject this into Axum via `axum::Extension<Arc<ProjectRuntimeRegistry>>`.
pub struct ProjectRuntimeRegistry {
    map: DashMap<String, Arc<ProjectRuntime>>,
}

impl Default for ProjectRuntimeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectRuntimeRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            map: DashMap::new(),
        }
    }

    /// Register a runtime for `project_id`, replacing any existing entry.
    pub async fn insert(&self, project_id: String, runtime: Arc<ProjectRuntime>) {
        self.map.insert(project_id, runtime);
    }

    /// Look up the runtime for `project_id`.
    ///
    /// Returns `None` when no runtime has been registered for that project.
    pub async fn get(&self, project_id: &str) -> Option<Arc<ProjectRuntime>> {
        self.map.get(project_id).map(|r| Arc::clone(&*r))
    }

    /// Remove the runtime for `project_id` and return it, if present.
    pub async fn remove(&self, project_id: &str) -> Option<Arc<ProjectRuntime>> {
        self.map.remove(project_id).map(|(_, v)| v)
    }

    /// Return the number of registered runtimes.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Return `true` if no runtimes are registered.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use molt_hub_harness::adapter::AgentEvent;
    use molt_hub_harness::supervisor::SupervisorConfig;
    use tokio::sync::broadcast;

    fn make_runtime(id: &str) -> Arc<ProjectRuntime> {
        let (tx, _rx) = broadcast::channel::<AgentEvent>(64);
        let supervisor = Arc::new(Supervisor::new(SupervisorConfig::default(), tx));
        let pipeline_config = Arc::new(PipelineConfigStore::in_memory());
        Arc::new(ProjectRuntime {
            project_id: id.to_string(),
            supervisor,
            pipeline_config,
        })
    }

    #[tokio::test]
    async fn registry_insert_and_get() {
        let registry = ProjectRuntimeRegistry::new();
        let rt = make_runtime("proj-1");

        registry.insert("proj-1".into(), Arc::clone(&rt)).await;

        let found = registry.get("proj-1").await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().project_id, "proj-1");
    }

    #[tokio::test]
    async fn registry_get_missing_returns_none() {
        let registry = ProjectRuntimeRegistry::new();
        assert!(registry.get("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn registry_remove() {
        let registry = ProjectRuntimeRegistry::new();
        registry.insert("p".into(), make_runtime("p")).await;
        assert_eq!(registry.len(), 1);

        let removed = registry.remove("p").await;
        assert!(removed.is_some());
        assert_eq!(registry.len(), 0);
    }

    #[tokio::test]
    async fn registry_is_empty() {
        let registry = ProjectRuntimeRegistry::new();
        assert!(registry.is_empty());
        registry.insert("x".into(), make_runtime("x")).await;
        assert!(!registry.is_empty());
    }
}
