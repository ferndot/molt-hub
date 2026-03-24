//! Per-project runtime state — supervisor, multi-board pipeline config, and event store.
//!
//! [`ProjectRuntime`] holds the live state for a single project.
//! [`ProjectRuntimeRegistry`] is an in-memory map of `project_id → runtime`
//! and is injected into Axum handlers via `axum::Extension`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use molt_hub_harness::supervisor::Supervisor;
use tokio::sync::RwLock;

use crate::pipeline::handlers::PipelineConfigStore;

// ---------------------------------------------------------------------------
// Multi-board store (per project)
// ---------------------------------------------------------------------------

/// Named kanban boards, each backed by an in-memory [`PipelineConfigStore`].
pub struct MultiBoardPipelineStore {
    boards: RwLock<HashMap<String, Arc<PipelineConfigStore>>>,
}

impl MultiBoardPipelineStore {
    /// One board with id `default` using built-in stage defaults.
    pub fn new() -> Self {
        let mut m = HashMap::new();
        m.insert(
            "default".to_string(),
            Arc::new(PipelineConfigStore::in_memory()),
        );
        Self {
            boards: RwLock::new(m),
        }
    }

    /// Start with `default` seeded from an existing config snapshot (e.g. global pipeline file).
    pub fn with_default_from_config(cfg: molt_hub_core::config::PipelineConfig) -> Self {
        let mut m = HashMap::new();
        m.insert(
            "default".to_string(),
            Arc::new(PipelineConfigStore::from_pipeline_config(cfg)),
        );
        Self {
            boards: RwLock::new(m),
        }
    }

    fn normalize_id(id: &str) -> Result<String, String> {
        let t = id.trim();
        if t.is_empty() {
            return Err("board id must not be empty".into());
        }
        if !t
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err("board id must be alphanumeric, underscore, or hyphen".into());
        }
        Ok(t.to_string())
    }

    pub async fn list_summaries(&self) -> Vec<BoardSummary> {
        let g = self.boards.read().await;
        let mut out: Vec<BoardSummary> = Vec::new();
        for (id, store) in g.iter() {
            out.push(BoardSummary {
                id: id.clone(),
                name: store.pipeline_display_name().await,
            });
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    pub async fn get_store(&self, board_id: &str) -> Option<Arc<PipelineConfigStore>> {
        self.boards.read().await.get(board_id).cloned()
    }

    pub async fn create_board(&self, id: &str, name: Option<String>) -> Result<(), String> {
        let id = Self::normalize_id(id)?;
        let mut g = self.boards.write().await;
        if g.contains_key(&id) {
            return Err(format!("board '{id}' already exists"));
        }
        let store = PipelineConfigStore::in_memory();
        if let Some(n) = name {
            let trimmed = n.trim();
            if !trimmed.is_empty() {
                store.set_display_name(trimmed.to_string()).await;
            }
        }
        g.insert(id, Arc::new(store));
        Ok(())
    }

    pub async fn delete_board(&self, board_id: &str) -> Result<(), String> {
        let id = Self::normalize_id(board_id)?;
        if id == "default" {
            return Err("cannot delete the default board".into());
        }
        let mut g = self.boards.write().await;
        if g.len() <= 1 {
            return Err("cannot delete the last board".into());
        }
        g.remove(&id)
            .ok_or_else(|| format!("board '{id}' not found"))?;
        Ok(())
    }
}

/// Wire shape for `GET /api/projects/:pid/boards`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BoardSummary {
    pub id: String,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Default cap on distinct project runtimes kept in memory before LRU eviction.
const DEFAULT_MAX_PROJECT_RUNTIMES: usize = 64;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Runtime entry (LRU)
// ---------------------------------------------------------------------------

struct RuntimeEntry {
    runtime: Arc<ProjectRuntime>,
    last_access_ms: AtomicU64,
}

impl RuntimeEntry {
    fn new(runtime: Arc<ProjectRuntime>) -> Self {
        Self {
            runtime,
            last_access_ms: AtomicU64::new(now_ms()),
        }
    }

    fn touch(&self) {
        self.last_access_ms.store(now_ms(), Ordering::Relaxed);
    }

    fn last_access_ms(&self) -> u64 {
        self.last_access_ms.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// ProjectRuntime
// ---------------------------------------------------------------------------

/// Live runtime state for a single project.
pub struct ProjectRuntime {
    /// The project's string identifier (matches the ULID stored in the config).
    pub project_id: String,
    /// Supervisor managing agents spawned under this project.
    pub supervisor: Arc<Supervisor>,
    /// Named pipeline / kanban boards for this project.
    pub boards: Arc<MultiBoardPipelineStore>,
}

// ---------------------------------------------------------------------------
// ProjectRuntimeRegistry
// ---------------------------------------------------------------------------

/// Registry of all active project runtimes, keyed by `project_id`.
///
/// Inject this into Axum via `axum::Extension<Arc<ProjectRuntimeRegistry>>`.
///
/// When the number of projects exceeds [`DEFAULT_MAX_PROJECT_RUNTIMES`], the
/// least-recently-used project (by [`get`] / [`insert`] touch) is evicted.
/// The synthetic `"default"` project is never evicted.
pub struct ProjectRuntimeRegistry {
    map: DashMap<String, RuntimeEntry>,
    max_projects: usize,
}

impl Default for ProjectRuntimeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Return an existing [`ProjectRuntime`] or register a new one.
///
/// **Call only after** verifying `project_id == "default"` or the project exists in
/// [`ProjectConfigStore`], otherwise bogus runtimes may be created.
pub async fn ensure_project_runtime(
    project_id: &str,
    registry: &ProjectRuntimeRegistry,
    supervisor: &Arc<Supervisor>,
) -> Arc<ProjectRuntime> {
    if let Some(r) = registry.get(project_id).await {
        return r;
    }
    let runtime = Arc::new(ProjectRuntime {
        project_id: project_id.to_string(),
        supervisor: Arc::clone(supervisor),
        boards: Arc::new(MultiBoardPipelineStore::new()),
    });
    registry
        .insert(project_id.to_string(), Arc::clone(&runtime))
        .await;
    runtime
}

impl ProjectRuntimeRegistry {
    /// Create a registry with the default capacity ([`DEFAULT_MAX_PROJECT_RUNTIMES`]).
    pub fn new() -> Self {
        Self::with_max_projects(DEFAULT_MAX_PROJECT_RUNTIMES)
    }

    /// Create a registry that retains at most `max_projects` entries before LRU eviction.
    pub fn with_max_projects(max_projects: usize) -> Self {
        Self {
            map: DashMap::new(),
            max_projects: max_projects.max(2),
        }
    }

    fn evict_lru_exclude_default(&self) {
        let mut candidate: Option<(String, u64)> = None;
        for r in self.map.iter() {
            let key = r.key().clone();
            if key == "default" {
                continue;
            }
            let t = r.value().last_access_ms();
            candidate = Some(match candidate {
                None => (key, t),
                Some((_, t_old)) if t < t_old => (key, t),
                Some(other) => other,
            });
        }
        if let Some((k, _)) = candidate {
            self.map.remove(&k);
        }
    }

    /// Register a runtime for `project_id`, replacing any existing entry.
    pub async fn insert(&self, project_id: String, runtime: Arc<ProjectRuntime>) {
        if project_id != "default"
            && self.map.len() >= self.max_projects
            && !self.map.contains_key(&project_id)
        {
            self.evict_lru_exclude_default();
        }

        let entry = RuntimeEntry::new(runtime);
        self.map.insert(project_id, entry);
    }

    /// Look up the runtime for `project_id`.
    ///
    /// Returns `None` when no runtime has been registered for that project.
    pub async fn get(&self, project_id: &str) -> Option<Arc<ProjectRuntime>> {
        self.map.get(project_id).map(|r| {
            r.touch();
            Arc::clone(&r.runtime)
        })
    }

    /// Remove the runtime for `project_id` and return it, if present.
    pub async fn remove(&self, project_id: &str) -> Option<Arc<ProjectRuntime>> {
        self.map
            .remove(project_id)
            .map(|(_, e)| Arc::clone(&e.runtime))
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
        let boards = Arc::new(MultiBoardPipelineStore::new());
        Arc::new(ProjectRuntime {
            project_id: id.to_string(),
            supervisor,
            boards,
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

    #[tokio::test]
    async fn registry_evicts_lru_when_over_capacity() {
        let registry = ProjectRuntimeRegistry::with_max_projects(2);

        registry
            .insert("default".into(), make_runtime("default"))
            .await;
        registry.insert("a".into(), make_runtime("a")).await;
        assert_eq!(registry.len(), 2);

        registry.insert("b".into(), make_runtime("b")).await;
        assert_eq!(registry.len(), 2);

        assert!(registry.get("default").await.is_some());
        assert!(registry.get("b").await.is_some());
        assert!(
            registry.get("a").await.is_none(),
            "project a should be evicted to make room for b"
        );
    }
}
