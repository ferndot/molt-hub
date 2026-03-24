//! Shared router builder for the Molt Hub Axum server.
//!
//! Used by both the standalone CLI binary (`molt-hub serve`) and the Tauri desktop shell.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::routing::{get, post};
use axum::{Json, Router};
use sqlx::sqlite::SqlitePoolOptions;
use tokio::task::JoinHandle;
use tower_http::services::{ServeDir, ServeFile};
use tracing::{debug, info, warn};

use molt_hub_core::events::SqliteEventStore;
use molt_hub_harness::adapter::AgentEvent;
use molt_hub_harness::claude::ClaudeAdapter;
use molt_hub_harness::supervisor::{Supervisor, SupervisorConfig};

use crate::agents::handlers::{agent_router, AgentState};
use crate::agents::output_buffer::{shared_output_buffer, spawn_agent_output_buffer_task};
use crate::agents::worktree_registry::{WorktreeManagerCache, WorktreeRegistry};
use crate::audit::{audit_router, start_audit_writer, AuditHandle, AuditState};
use crate::credentials::KeyringStore;
use crate::events::handlers::{events_router, tasks_router, EventStoreState};
use crate::integrations::github_app::GithubAppCredentials;
use crate::integrations::github_handlers::{github_integrations_router, GithubAppState};
use crate::integrations::github_oauth::GithubOAuthService;
use crate::integrations::github_oauth_handlers::GithubOAuthState;
use crate::integrations::handlers::{jira_integrations_router, JiraAppState};
use crate::integrations::jira_oauth_handlers::{jira_oauth_router, JiraOAuthState};
use crate::integrations::oauth::JiraOAuthService;
use crate::integrations::oauth_redirect::{github_redirect_uri, jira_redirect_uri};
use crate::pipeline::handlers::PipelineState;
use crate::projects::handlers::{project_router, ProjectConfigStore};
use crate::projects::runtime::{MultiBoardPipelineStore, ProjectRuntime, ProjectRuntimeRegistry};
use crate::settings::{typed_settings_router, SettingsFileStore, TypedSettingsState};
use crate::system::pick_repo_folder;
use crate::ws::{ws_handler, ConnectionManager};
use crate::ws_broadcast::{broadcast_metrics, MetricsPayload};

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// `GET /api/health` — confirms Axum is serving (not a foreign process or SPA fallback).
async fn api_health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

/// Resolve the default event store database path: `~/.config/molt-hub/events.db`.
fn default_events_db_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("molt-hub")
        .join("events.db")
}

/// Build the Molt Hub Axum router with WebSocket and static file serving.
///
/// Returns both the router and a shared `ConnectionManager` that callers can
/// use to broadcast events to connected clients.
///
/// The returned router provides:
/// - `GET /ws` — WebSocket upgrade for real-time UI updates
/// - `GET /api/health` — JSON `{ "ok": true }` for startup / port checks
/// - `POST /api/system/pick-repo-folder` — native folder picker on the server host (browser UI)
/// - `GET /api/events` — query events (by task_id or since timestamp)
/// - `GET /api/events/:id` — get a single event
/// - `POST /api/events` — append an event
/// - `GET /api/tasks` — list tasks derived from events
/// - `/api/projects/...` — projects, agents, and per-board pipeline stages (`…/boards/:bid/stages`)
/// - `/*`      — Static files from `dist_dir` with `index.html` fallback (SPA routing)
pub async fn build_router(
    dist_dir: PathBuf,
) -> (Router, Arc<ConnectionManager>, Arc<Supervisor>, AuditHandle) {
    let manager = Arc::new(ConnectionManager::new());
    let index_html = dist_dir.join("index.html");

    // Pipeline stages API state
    let pipeline_state = Arc::new(PipelineState::default_stages());

    // Process supervisor
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel::<AgentEvent>(256);

    // Agent output buffer (shared with broadcast layer)
    let output_buffer = shared_output_buffer();
    let _agent_output_fanout =
        spawn_agent_output_buffer_task(event_tx.subscribe(), Arc::clone(&output_buffer));

    let supervisor = Arc::new(Supervisor::new(SupervisorConfig::default(), event_tx));

    // Agent API state
    let agent_state = Arc::new(AgentState {
        supervisor: Arc::clone(&supervisor),
        output_buffer,
        claude_adapter: Arc::new(ClaudeAdapter::new()),
        test_spawn_adapter: None,
        worktree_managers: Arc::new(WorktreeManagerCache::new()),
        worktree_registry: Arc::new(WorktreeRegistry::new()),
    });

    // Audit log writer
    let audit_handle = start_audit_writer();
    let audit_state = Arc::new(AuditState {
        handle: audit_handle.clone(),
    });

    // Typed settings (JSON-file-backed)
    let settings_store = Arc::new(SettingsFileStore::open_default());
    let typed_settings_state = Arc::new(TypedSettingsState {
        store: settings_store,
    });

    // ---- SQLite event store ------------------------------------------------
    let event_store_state = init_event_store().await;

    // ---- Project runtime registry ------------------------------------------
    let registry = Arc::new(ProjectRuntimeRegistry::new());
    {
        let default_cfg = pipeline_state.snapshot_config().await;
        let boards = Arc::new(MultiBoardPipelineStore::with_default_from_config(
            default_cfg,
        ));
        let default_runtime = Arc::new(ProjectRuntime {
            project_id: "default".to_owned(),
            supervisor: Arc::clone(&supervisor),
            boards,
        });
        registry.insert("default".to_owned(), default_runtime).await;
    }

    // Project store (YAML-backed)
    let project_state = Arc::new(ProjectConfigStore::load_default());

    let agents = agent_router(agent_state);
    let audit = audit_router(audit_state);
    let typed_settings = typed_settings_router(Arc::clone(&typed_settings_state));
    let projects = project_router(project_state);

    // Shared credential store backed by the OS keychain.
    let credential_store: Arc<dyn crate::credentials::CredentialStore> =
        Arc::new(KeyringStore::new());

    // GitHub OAuth — client id/secret from env or optional compile-time defaults (see `github_oauth`).
    let github_callback = github_redirect_uri();
    let github_oauth_svc = GithubOAuthService::from_redirect_uri(&github_callback);
    let github_app_creds = match GithubAppCredentials::try_from_env() {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "GitHub App env present but invalid; continuing with OAuth-only");
            None
        }
    };
    let github_oauth_state = Arc::new(GithubOAuthState::with_github_app(
        github_oauth_svc,
        Arc::clone(&credential_store),
        github_app_creds,
    ));
    let github_store = event_store_state.as_ref().map(|es| Arc::clone(&es.store));
    let github_stack = github_integrations_router(GithubAppState {
        oauth: Arc::clone(&github_oauth_state),
        store: github_store,
    });

    // Jira (Atlassian 3LO) — PKCE + confidential client secret for code exchange (see `oauth` module).
    let jira_oauth_svc = JiraOAuthService::from_redirect_uri(&jira_redirect_uri());
    let jira_oauth_state = Arc::new(JiraOAuthState::new(
        jira_oauth_svc,
        Arc::clone(&credential_store),
    ));

    // Jira REST (search/import/projects) requires the event store; OAuth-only if SQLite failed.
    let jira_stack = match event_store_state.as_ref() {
        Some(es) => jira_integrations_router(JiraAppState {
            oauth: Arc::clone(&jira_oauth_state),
            store: Arc::clone(&es.store),
        }),
        None => jira_oauth_router(Arc::clone(&jira_oauth_state)),
    };

    let mut router = Router::new()
        .route("/ws", get(ws_handler))
        .route("/api/health", get(api_health))
        .route("/api/system/pick-repo-folder", post(pick_repo_folder))
        .nest_service("/api/agents", agents)
        .nest_service("/api/audit", audit)
        .nest_service("/api/settings", typed_settings)
        .nest_service("/api/integrations/github", github_stack)
        .nest_service("/api/integrations/jira", jira_stack)
        // Projects CRUD + project-scoped agent/pipeline routes.
        // The project-scoped routes are part of project_router (same nest_service)
        // so there is no wildcard conflict.
        .nest_service("/api/projects", projects);

    // Wire event/task routes if the store initialised successfully.
    if let Some(es_state) = event_store_state {
        let es = Arc::new(es_state);
        let events = events_router(Arc::clone(&es));
        let tasks = tasks_router(es);
        router = router
            .nest_service("/api/events", events)
            .nest_service("/api/tasks", tasks);
    }

    // Single `ProjectRuntimeRegistry` Extension (populated above, e.g. `"default"`).
    let router = router
        .fallback_service(ServeDir::new(dist_dir).fallback(ServeFile::new(index_html)))
        .layer(axum::Extension(Arc::clone(&registry)))
        .layer(axum::Extension(Arc::clone(&supervisor)))
        .layer(axum::Extension(Arc::clone(&manager)))
        .layer(axum::Extension(Arc::clone(&typed_settings_state)))
        .with_state(Arc::clone(&manager));

    (router, manager, supervisor, audit_handle)
}

/// Initialise the SQLite event store, returning `None` if it fails so the
/// server can still start without event persistence.
async fn init_event_store() -> Option<EventStoreState> {
    let db_path = default_events_db_path();

    // Ensure parent directory exists.
    if let Some(parent) = db_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            warn!(path = %parent.display(), error = %e, "failed to create event store directory");
            return None;
        }
    }

    let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
    info!(path = %db_path.display(), "opening event store");

    let pool = match SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "failed to open SQLite event store — events API disabled");
            return None;
        }
    };

    match SqliteEventStore::new(pool).await {
        Ok(store) => {
            info!("event store initialised");
            Some(EventStoreState {
                store: Arc::new(store),
            })
        }
        Err(e) => {
            warn!(error = %e, "failed to initialise event store schema — events API disabled");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Periodic health metrics broadcast
// ---------------------------------------------------------------------------

/// Spawn a background task that periodically broadcasts system health metrics
/// to all connected WebSocket clients every `interval`.
///
/// Metrics include:
/// - CPU usage (approximated from system load average)
/// - Memory usage (from process RSS via libc)
/// - Active agent count (from the Supervisor's real agent registry)
///
/// Returns a `JoinHandle` that can be used to abort the task on shutdown.
pub fn spawn_health_metrics_task(
    manager: Arc<ConnectionManager>,
    supervisor: Arc<Supervisor>,
    interval: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;

            let (cpu_usage, memory_bytes) = collect_system_metrics();
            let active_agents = supervisor.agent_count() as u32;

            let payload = MetricsPayload {
                active_agent_count: Some(active_agents),
                cpu_usage: Some(cpu_usage),
                memory_bytes: Some(memory_bytes),
            };

            debug!(
                cpu = cpu_usage,
                mem_bytes = memory_bytes,
                active_agents = active_agents,
                "broadcasting health metrics"
            );

            broadcast_metrics(&manager, &payload);
        }
    })
}

/// Collect basic system metrics without external crate dependencies.
///
/// - **CPU**: Uses libc `getloadavg` on Unix; returns a normalised 0-100 value
///   based on 1-minute load average divided by available CPUs.
/// - **Memory**: Reads process RSS from `/proc/self/statm` on Linux; falls
///   back to a reasonable estimate on other platforms.
fn collect_system_metrics() -> (f64, u64) {
    let cpu_usage = collect_cpu_usage();
    let memory_bytes = collect_memory_bytes();
    (cpu_usage, memory_bytes)
}

#[cfg(unix)]
fn collect_cpu_usage() -> f64 {
    let mut loadavg: [f64; 3] = [0.0; 3];
    // SAFETY: getloadavg writes up to `nelem` doubles into the provided buffer.
    let ret = unsafe { libc::getloadavg(loadavg.as_mut_ptr(), 1) };
    if ret < 1 {
        return 0.0;
    }
    let ncpus = std::thread::available_parallelism()
        .map(|n| n.get() as f64)
        .unwrap_or(1.0);
    // Normalise to 0-100 range (capped).
    (loadavg[0] / ncpus * 100.0).min(100.0).max(0.0)
}

#[cfg(not(unix))]
fn collect_cpu_usage() -> f64 {
    // Fallback: return a modest constant on non-Unix.
    15.0
}

fn collect_memory_bytes() -> u64 {
    // Try reading from /proc/self/statm (Linux).
    if let Ok(contents) = std::fs::read_to_string("/proc/self/statm") {
        // Second field is RSS in pages.
        if let Some(rss_pages_str) = contents.split_whitespace().nth(1) {
            if let Ok(rss_pages) = rss_pages_str.parse::<u64>() {
                let page_size = 4096u64; // typical page size
                return rss_pages * page_size;
            }
        }
    }

    // macOS: use mach task_info to get RSS.
    #[cfg(target_os = "macos")]
    {
        if let Some(rss) = macos_rss() {
            return rss;
        }
    }

    // Fallback: estimate ~50 MB.
    50 * 1024 * 1024
}

#[cfg(target_os = "macos")]
fn macos_rss() -> Option<u64> {
    use std::mem;
    // Use rusage to get RSS on macOS — avoids deprecated mach_task_self.
    // SAFETY: getrusage is a standard POSIX call reading our own process stats.
    unsafe {
        let mut usage: libc::rusage = mem::zeroed();
        let ret = libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        if ret == 0 {
            // On macOS, ru_maxrss is in bytes (unlike Linux where it's in KB).
            Some(usage.ru_maxrss as u64)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn build_router_does_not_panic() {
        let (_router, _manager, _supervisor, _audit) =
            build_router(PathBuf::from("/tmp/nonexistent-dist")).await;
    }

    #[tokio::test]
    async fn build_router_returns_shared_manager() {
        let (_router, manager, _supervisor, _audit) =
            build_router(PathBuf::from("/tmp/nonexistent-dist")).await;
        assert_eq!(manager.connection_count(), 0);
    }

    #[tokio::test]
    async fn build_router_returns_supervisor() {
        let (_router, _manager, supervisor, _audit) =
            build_router(PathBuf::from("/tmp/nonexistent-dist")).await;
        assert_eq!(supervisor.agent_count(), 0);
    }

    #[test]
    fn collect_system_metrics_returns_sane_values() {
        let (cpu, mem) = collect_system_metrics();
        assert!(cpu >= 0.0 && cpu <= 100.0, "cpu out of range: {cpu}");
        assert!(mem > 0, "memory should be > 0, got {mem}");
    }
}
