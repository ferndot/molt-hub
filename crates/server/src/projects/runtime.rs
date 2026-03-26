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
use tokio::sync::RwLock;
use tracing::warn;
use ulid::Ulid;

use crate::pipeline::handlers::PipelineConfigStore;
use crate::projects::boards_store::BoardsStore;

// ---------------------------------------------------------------------------
// Persist directory helper
// ---------------------------------------------------------------------------

fn board_persist_dir(project_id: &str) -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("molt-hub").join("boards").join(project_id))
}

// ---------------------------------------------------------------------------
// Load board YAML configs from disk using the board IDs supplied by the caller
// ---------------------------------------------------------------------------

/// Load per-board `{id}.yaml` files from `root` for the given `board_ids`.
///
/// Any board whose YAML file is missing is silently dropped from the result.
/// Returns (kept_ids, boards_map) so callers can reconcile stale SQLite rows.
fn load_board_yamls(
    root: &Path,
    board_ids: &[String],
    template: &PipelineConfig,
) -> (Vec<String>, HashMap<String, Arc<PipelineConfigStore>>) {
    let mut map = HashMap::new();
    let mut kept: Vec<String> = Vec::new();
    for id in board_ids {
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
    (kept, map)
}

/// Named kanban boards, each backed by a [`PipelineConfigStore`].
///
/// When a config directory is available (`~/.config/molt-hub/boards/<project_id>/` on Unix),
/// boards are persisted as `{ulid}.yaml` with the index tracked in SQLite, and survive
/// server restarts.
///
/// Use [`Self::empty_with_template`] for tests (no disk I/O).
pub struct MultiBoardPipelineStore {
    /// When set, board YAML files live under this directory.
    persist_dir: Option<std::path::PathBuf>,
    /// SQLite-backed index tracking which boards belong to this project.
    boards_store: Option<Arc<BoardsStore>>,
    /// `project_id` — needed for SQLite writes.
    project_id: String,
    boards: RwLock<HashMap<String, Arc<PipelineConfigStore>>>,
    /// Pipeline config copied for each new board file (columns, stages, hooks).
    new_board_template: PipelineConfig,
}

impl MultiBoardPipelineStore {
    /// Load boards for `project_id` using the SQLite `boards_store`, falling back
    /// to an empty list if the store is not available or fails.
    ///
    /// Per-board YAML config files are loaded from the platform config directory.
    pub async fn load_or_empty(
        project_id: &str,
        new_board_template: PipelineConfig,
        boards_store: Option<Arc<BoardsStore>>,
    ) -> Self {
        let persist_dir = board_persist_dir(project_id);
        let mut boards_map = HashMap::new();

        if let (Some(ref dir), Some(ref store)) = (&persist_dir, &boards_store) {
            match store.list_boards(project_id).await {
                Ok(records) => {
                    let board_ids: Vec<String> =
                        records.iter().map(|r| r.board_id.clone()).collect();
                    let (kept, map) = load_board_yamls(dir, &board_ids, &new_board_template);
                    boards_map = map;

                    // Remove SQLite rows whose YAML files have been deleted.
                    let stale: Vec<_> = board_ids
                        .iter()
                        .filter(|id| !kept.contains(id))
                        .collect();
                    for id in stale {
                        if let Err(e) = store.remove_board(project_id, id).await {
                            warn!(
                                project_id = %project_id,
                                board_id = %id,
                                error = %e,
                                "failed to remove stale board from SQLite"
                            );
                        }
                    }
                }
                Err(e) => warn!(
                    project_id = %project_id,
                    error = %e,
                    "failed to load boards from SQLite; starting with an empty board list",
                ),
            }
        } else if let Some(ref dir) = persist_dir {
            // No SQLite store available — start empty (boards will be re-created if needed).
            warn!(
                project_id = %project_id,
                path = %dir.display(),
                "no SQLite boards store available; starting with empty board list",
            );
        }

        Self {
            persist_dir,
            boards_store,
            project_id: project_id.to_string(),
            boards: RwLock::new(boards_map),
            new_board_template,
        }
    }

    /// Empty project: no boards until the user creates one (each new board uses `template`).
    ///
    /// Does not read or write the filesystem or SQLite (for unit tests).
    pub fn empty_with_template(new_board_template: PipelineConfig) -> Self {
        Self {
            persist_dir: None,
            boards_store: None,
            project_id: String::new(),
            boards: RwLock::new(HashMap::new()),
            new_board_template,
        }
    }

    /// Tests: load or create boards under `persist_dir` reading from on-disk YAML only.
    ///
    /// Does NOT use SQLite (for tests that only care about YAML file persistence).
    #[cfg(test)]
    fn with_persist_dir(persist_dir: std::path::PathBuf, new_board_template: PipelineConfig) -> Self {
        // Scan the directory for any existing {id}.yaml files directly (no index).
        let board_ids: Vec<String> = std::fs::read_dir(&persist_dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        if name.ends_with(".yaml") && name != "boards-index.yaml" {
                            Some(name.trim_end_matches(".yaml").to_string())
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let (_, boards_map) = load_board_yamls(&persist_dir, &board_ids, &new_board_template);

        Self {
            persist_dir: Some(persist_dir),
            boards_store: None,
            project_id: String::new(),
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
            let config_path = path.display().to_string();
            let store = PipelineConfigStore::from_file_with_template(
                path,
                self.new_board_template.clone(),
            );
            store.set_display_name(title.to_string()).await;

            // Register in SQLite boards index.
            if let Some(ref bs) = self.boards_store {
                if let Err(e) = bs.add_board(&self.project_id, &id, &config_path).await {
                    warn!(
                        project_id = %self.project_id,
                        board_id = %id,
                        error = %e,
                        "failed to add board to SQLite index"
                    );
                }
            }

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
        }

        // Remove from SQLite boards index.
        if let Some(ref bs) = self.boards_store {
            if let Err(e) = bs.remove_board(&self.project_id, &id).await {
                warn!(
                    project_id = %self.project_id,
                    board_id = %id,
                    error = %e,
                    "failed to remove board from SQLite index"
                );
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
    /// SQLite-backed boards index shared across all project runtimes.
    boards_store: Option<Arc<BoardsStore>>,
}

impl Default for ProjectRuntimeRegistry {
    fn default() -> Self {
        Self::new(PipelineConfig::board_defaults(), None)
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
        boards: Arc::new(
            MultiBoardPipelineStore::load_or_empty(
                project_id,
                registry.new_board_template(),
                registry.boards_store(),
            )
            .await,
        ),
    });
    registry
        .insert(project_id.to_string(), Arc::clone(&runtime))
        .await;
    runtime
}

impl ProjectRuntimeRegistry {
    /// Create a registry with the default capacity ([`DEFAULT_MAX_PROJECT_RUNTIMES`]).
    pub fn new(new_board_template: PipelineConfig, boards_store: Option<Arc<BoardsStore>>) -> Self {
        Self::with_max_projects(DEFAULT_MAX_PROJECT_RUNTIMES, new_board_template, boards_store)
    }

    /// Create a registry that retains at most `max_projects` entries before LRU eviction.
    pub fn with_max_projects(
        max_projects: usize,
        new_board_template: PipelineConfig,
        boards_store: Option<Arc<BoardsStore>>,
    ) -> Self {
        Self {
            map: DashMap::new(),
            max_projects: max_projects.max(2),
            new_board_template,
            boards_store,
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

    /// Return the shared SQLite boards store, if one is configured.
    pub fn boards_store(&self) -> Option<Arc<BoardsStore>> {
        self.boards_store.clone()
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
        let registry = ProjectRuntimeRegistry::new(PipelineConfig::board_defaults(), None);
        let rt = make_runtime("proj-1");

        registry.insert("proj-1".into(), Arc::clone(&rt)).await;

        let found = registry.get("proj-1").await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().project_id, "proj-1");
    }

    #[tokio::test]
    async fn registry_get_missing_returns_none() {
        let registry = ProjectRuntimeRegistry::new(PipelineConfig::board_defaults(), None);
        assert!(registry.get("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn registry_remove() {
        let registry = ProjectRuntimeRegistry::new(PipelineConfig::board_defaults(), None);
        registry.insert("p".into(), make_runtime("p")).await;
        assert_eq!(registry.len(), 1);

        let removed = registry.remove("p").await;
        assert!(removed.is_some());
        assert_eq!(registry.len(), 0);
    }

    #[tokio::test]
    async fn registry_is_empty() {
        let registry = ProjectRuntimeRegistry::new(PipelineConfig::board_defaults(), None);
        assert!(registry.is_empty());
        registry.insert("x".into(), make_runtime("x")).await;
        assert!(!registry.is_empty());
    }

    #[tokio::test]
    async fn registry_evicts_lru_when_over_capacity() {
        let registry =
            ProjectRuntimeRegistry::with_max_projects(2, PipelineConfig::board_defaults(), None);

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
