//! Per-project runtime state — supervisor, multi-board pipeline config, and event store.
//!
//! Kanban boards for each project are persisted under the platform config directory
//! (see [`MultiBoardPipelineStore::load_or_empty`]).
//!
//! [`ProjectRuntime`] holds the live state for a single project.
//! [`ProjectRuntimeRegistry`] is an in-memory map of `project_id → runtime`
//! and is injected into Axum handlers via `axum::Extension`.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use molt_hub_core::config::PipelineConfig;
use molt_hub_harness::supervisor::Supervisor;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::warn;
use ulid::Ulid;

use crate::pipeline::handlers::PipelineConfigStore;

// ---------------------------------------------------------------------------
// Multi-board store (per project)
// ---------------------------------------------------------------------------

const BOARDS_INDEX_FILE: &str = "boards-index.yaml";

#[derive(Debug, Default, Serialize, Deserialize)]
struct BoardsIndex {
    #[serde(default)]
    board_ids: Vec<String>,
}

fn board_persist_dir(project_id: &str) -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("molt-hub").join("boards").join(project_id))
}

fn boards_index_path(root: &Path) -> std::path::PathBuf {
    root.join(BOARDS_INDEX_FILE)
}

fn read_boards_index(path: &Path) -> Result<BoardsIndex, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_yaml::from_str(&s).map_err(|e| format!("boards index: {e}")),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(BoardsIndex::default()),
        Err(e) => Err(format!("read boards index: {e}")),
    }
}

fn write_boards_index(path: &Path, index: &BoardsIndex) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let s = serde_yaml::to_string(index).map_err(|e| e.to_string())?;
    std::fs::write(path, s).map_err(|e| e.to_string())
}

fn load_board_stores_from_disk(
    root: &Path,
    template: &PipelineConfig,
) -> Result<HashMap<String, Arc<PipelineConfigStore>>, String> {
    let index_path = boards_index_path(root);
    let mut index = read_boards_index(&index_path)?;
    let mut map = HashMap::new();
    let mut kept: Vec<String> = Vec::new();
    for id in &index.board_ids {
        let p = root.join(format!("{id}.yaml"));
        if p.is_file() {
            kept.push(id.clone());
            map.insert(
                id.clone(),
                Arc::new(PipelineConfigStore::from_file_with_template(
                    p,
                    template.clone(),
                )),
            );
        }
    }
    if kept != index.board_ids {
        index.board_ids = kept;
        write_boards_index(&index_path, &index)?;
    }
    Ok(map)
}

fn append_board_id(root: &Path, id: &str) -> Result<(), String> {
    let path = boards_index_path(root);
    let mut index = read_boards_index(&path)?;
    if !index.board_ids.iter().any(|x| x == id) {
        index.board_ids.push(id.to_string());
    }
    write_boards_index(&path, &index)
}

fn remove_board_id(root: &Path, id: &str) -> Result<(), String> {
    let path = boards_index_path(root);
    let mut index = read_boards_index(&path)?;
    index.board_ids.retain(|x| x != id);
    write_boards_index(&path, &index)
}

/// Named kanban boards, each backed by a [`PipelineConfigStore`].
///
/// When a config directory is available (`~/.config/molt-hub/boards/<project_id>/` on Unix),
/// boards are persisted as `{ulid}.yaml` plus `boards-index.yaml` and survive server restarts.
///
/// Use [`Self::empty_with_template`] for tests (no disk I/O).
pub struct MultiBoardPipelineStore {
    /// When set, board YAML files and the index live under this directory.
    persist_dir: Option<std::path::PathBuf>,
    boards: RwLock<HashMap<String, Arc<PipelineConfigStore>>>,
    /// Pipeline config copied for each new board file (columns, stages, hooks).
    new_board_template: PipelineConfig,
}

impl MultiBoardPipelineStore {
    /// Load boards from disk for `project_id`, or start empty if none exist.
    pub fn load_or_empty(project_id: &str, new_board_template: PipelineConfig) -> Self {
        let persist_dir = board_persist_dir(project_id);
        let mut boards_map = HashMap::new();
        if let Some(ref dir) = persist_dir {
            match load_board_stores_from_disk(dir, &new_board_template) {
                Ok(m) => boards_map = m,
                Err(e) => warn!(
                    project_id = %project_id,
                    path = %dir.display(),
                    error = %e,
                    "failed to load boards from disk; starting with an empty board list",
                ),
            }
        }
        Self {
            persist_dir,
            boards: RwLock::new(boards_map),
            new_board_template,
        }
    }

    /// Empty project: no boards until the user creates one (each new board uses `template`).
    ///
    /// Does not read or write the filesystem (for unit tests).
    pub fn empty_with_template(new_board_template: PipelineConfig) -> Self {
        Self {
            persist_dir: None,
            boards: RwLock::new(HashMap::new()),
            new_board_template,
        }
    }

    /// Tests: load or create boards under `persist_dir` with stock [`PipelineConfig::board_defaults`].
    #[cfg(test)]
    fn with_persist_dir(persist_dir: std::path::PathBuf, new_board_template: PipelineConfig) -> Self {
        let mut boards_map = HashMap::new();
        if let Ok(m) = load_board_stores_from_disk(&persist_dir, &new_board_template) {
            boards_map = m;
        }
        Self {
            persist_dir: Some(persist_dir),
            boards: RwLock::new(boards_map),
            new_board_template,
        }
    }

    /// Tests: empty store with stock [`PipelineConfig::board_defaults`] as the create template.
    pub fn new() -> Self {
        Self::empty_with_template(PipelineConfig::board_defaults())
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

    /// Create a board with a new ULID key and the given display name (`PipelineConfig::name`).
    pub async fn create_board(&self, display_name: &str) -> Result<String, String> {
        let title = display_name.trim();
        if title.is_empty() {
            return Err("board name must not be empty".into());
        }
        let id = Ulid::new().to_string();

        let arc_store = if let Some(ref root) = self.persist_dir {
            std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
            let path = root.join(format!("{id}.yaml"));
            let store = PipelineConfigStore::from_file_with_template(
                path,
                self.new_board_template.clone(),
            );
            store.set_display_name(title.to_string()).await;
            append_board_id(root, &id)?;
            Arc::new(store)
        } else {
            let store = PipelineConfigStore::from_pipeline_config(self.new_board_template.clone());
            store.set_display_name(title.to_string()).await;
            Arc::new(store)
        };

        let mut g = self.boards.write().await;
        g.insert(id.clone(), arc_store);
        Ok(id)
    }

    pub async fn delete_board(&self, board_id: &str) -> Result<(), String> {
        let id = Self::normalize_id(board_id)?;
        let mut g = self.boards.write().await;
        g.remove(&id)
            .ok_or_else(|| format!("board '{id}' not found"))?;
        drop(g);

        if let Some(ref root) = self.persist_dir {
            let yaml = root.join(format!("{id}.yaml"));
            if let Err(e) = std::fs::remove_file(&yaml) {
                if e.kind() != ErrorKind::NotFound {
                    warn!(path = %yaml.display(), error = %e, "failed to remove board yaml");
                }
            }
            if let Err(e) = remove_board_id(root, &id) {
                warn!(error = %e, "failed to update boards index after delete");
            }
        }
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
    /// Copied into each new [`MultiBoardPipelineStore`] when a project runtime is created.
    new_board_template: PipelineConfig,
}

impl Default for ProjectRuntimeRegistry {
    fn default() -> Self {
        Self::new(PipelineConfig::board_defaults())
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
        boards: Arc::new(MultiBoardPipelineStore::load_or_empty(
            project_id,
            registry.new_board_template(),
        )),
    });
    registry
        .insert(project_id.to_string(), Arc::clone(&runtime))
        .await;
    runtime
}

impl ProjectRuntimeRegistry {
    /// Create a registry with the default capacity ([`DEFAULT_MAX_PROJECT_RUNTIMES`]).
    pub fn new(new_board_template: PipelineConfig) -> Self {
        Self::with_max_projects(DEFAULT_MAX_PROJECT_RUNTIMES, new_board_template)
    }

    /// Create a registry that retains at most `max_projects` entries before LRU eviction.
    pub fn with_max_projects(max_projects: usize, new_board_template: PipelineConfig) -> Self {
        Self {
            map: DashMap::new(),
            max_projects: max_projects.max(2),
            new_board_template,
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

    /// Pipeline snapshot used when creating new boards (and new project runtimes).
    pub fn new_board_template(&self) -> PipelineConfig {
        self.new_board_template.clone()
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
        let registry = ProjectRuntimeRegistry::new(PipelineConfig::board_defaults());
        let rt = make_runtime("proj-1");

        registry.insert("proj-1".into(), Arc::clone(&rt)).await;

        let found = registry.get("proj-1").await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().project_id, "proj-1");
    }

    #[tokio::test]
    async fn registry_get_missing_returns_none() {
        let registry = ProjectRuntimeRegistry::new(PipelineConfig::board_defaults());
        assert!(registry.get("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn registry_remove() {
        let registry = ProjectRuntimeRegistry::new(PipelineConfig::board_defaults());
        registry.insert("p".into(), make_runtime("p")).await;
        assert_eq!(registry.len(), 1);

        let removed = registry.remove("p").await;
        assert!(removed.is_some());
        assert_eq!(registry.len(), 0);
    }

    #[tokio::test]
    async fn registry_is_empty() {
        let registry = ProjectRuntimeRegistry::new(PipelineConfig::board_defaults());
        assert!(registry.is_empty());
        registry.insert("x".into(), make_runtime("x")).await;
        assert!(!registry.is_empty());
    }

    #[tokio::test]
    async fn registry_evicts_lru_when_over_capacity() {
        let registry =
            ProjectRuntimeRegistry::with_max_projects(2, PipelineConfig::board_defaults());

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

    #[tokio::test]
    async fn persisted_boards_survive_store_reload() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let tpl = PipelineConfig::board_defaults();
        let board_id = {
            let s = MultiBoardPipelineStore::with_persist_dir(root.clone(), tpl.clone());
            s.create_board("Sprint 1").await.unwrap()
        };
        let s2 = MultiBoardPipelineStore::with_persist_dir(root, tpl);
        let boards = s2.list_summaries().await;
        assert_eq!(boards.len(), 1);
        assert_eq!(boards[0].id, board_id);
        assert_eq!(boards[0].name, "Sprint 1");
    }
}
