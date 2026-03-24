//! Per-project runtime resources.
//!
//! Each monitored project gets its own [`ProjectRuntime`] which bundles together
//! the supervisor, pipeline configuration, and event store scoped to that project.
//! The [`ProjectRuntimeRegistry`] holds all active runtimes keyed by project ID.

use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;

use crate::pipeline::handlers::PipelineState;
use molt_hub_core::events::SqliteEventStore;
use molt_hub_harness::supervisor::Supervisor;

/// All runtime resources scoped to a single project.
pub struct ProjectRuntime {
    pub project_id: String,
    pub supervisor: Arc<Supervisor>,
    pub pipeline_config: Arc<PipelineState>,
    pub event_store: Arc<SqliteEventStore>,
}

impl ProjectRuntime {
    pub fn new(
        project_id: impl Into<String>,
        supervisor: Arc<Supervisor>,
        pipeline_config: Arc<PipelineState>,
        event_store: Arc<SqliteEventStore>,
    ) -> Self {
        Self {
            project_id: project_id.into(),
            supervisor,
            pipeline_config,
            event_store,
        }
    }
}

/// Registry of per-project runtimes.  The `"default"` project is always present.
pub struct ProjectRuntimeRegistry {
    runtimes: RwLock<HashMap<String, Arc<ProjectRuntime>>>,
}

impl ProjectRuntimeRegistry {
    pub fn new() -> Self {
        Self {
            runtimes: RwLock::new(HashMap::new()),
        }
    }

    pub async fn insert(&self, runtime: Arc<ProjectRuntime>) {
        self.runtimes
            .write()
            .await
            .insert(runtime.project_id.clone(), runtime);
    }

    pub async fn get(&self, project_id: &str) -> Option<Arc<ProjectRuntime>> {
        self.runtimes.read().await.get(project_id).cloned()
    }

    pub async fn get_default(&self) -> Option<Arc<ProjectRuntime>> {
        self.get("default").await
    }

    pub async fn list(&self) -> Vec<Arc<ProjectRuntime>> {
        self.runtimes.read().await.values().cloned().collect()
    }
}

impl Default for ProjectRuntimeRegistry {
    fn default() -> Self {
        Self::new()
    }
}
