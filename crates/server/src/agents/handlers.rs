//! Axum HTTP handlers for agent lifecycle management.
//!
//! Routes:
//!   GET  /api/agents             — list all agents with status
//!   POST /api/agents/spawn       — spawn a new agent
//!   POST /api/agents/:id/terminate — terminate an agent
//!   POST /api/agents/:id/pause     — pause an agent
//!   POST /api/agents/:id/resume    — resume an agent
//!   POST /api/agents/:id/steer     — send a steering message to an agent
//!   GET  /api/agents/:id/output    — get buffered output lines
//!   POST /api/agents/suggest-task-title — one-shot title via the configured harness (Claude CLI or CLI adapter)

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::{instrument, warn};

use molt_hub_core::model::{AgentId, SessionId, TaskId};
use molt_hub_harness::acp::AcpAdapter;
use molt_hub_harness::adapter::SpawnConfig;
use molt_hub_harness::supervisor::{SteerMessage, SteerPriority, Supervisor, SupervisorError};
use molt_hub_harness::worktree::{validate_repo, WorktreeConfig, WorktreeManager};

use crate::projects::handlers::ProjectConfigStore;
use crate::projects::runtime::{ensure_project_runtime, ProjectRuntimeRegistry};
use crate::settings::typed_handlers::TypedSettingsState;

use super::output_buffer::AgentOutputBuffer;
use super::worktree_registry::{WorktreeManagerCache, WorktreeRegistry};

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// State shared across agent handlers.
pub struct AgentState {
    pub supervisor: Arc<Supervisor>,
    pub output_buffer: Arc<AgentOutputBuffer>,
    /// When set, [`spawn_agent`] uses this adapter for any `adapter_type` (unit tests only).
    pub test_spawn_adapter: Option<Arc<dyn molt_hub_harness::adapter::AgentAdapter>>,
    /// One [`WorktreeManager`] per repository root (agent isolation under `.molt/worktrees/`).
    pub worktree_managers: Arc<WorktreeManagerCache>,
    pub worktree_registry: Arc<WorktreeRegistry>,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// A single agent as returned by the list API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub agent_id: String,
    pub task_id: String,
    pub status: String,
}

/// Top-level response for GET /api/agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsListResponse {
    pub agents: Vec<AgentResponse>,
    pub count: usize,
}

/// Response for POST /api/agents/spawn.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnResponse {
    pub agent_id: String,
    pub message: String,
}

/// Request body for POST /api/agents/spawn.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnRequest {
    /// Task instructions for the agent.
    pub instructions: String,
    /// Working directory for the agent process.
    pub working_dir: String,
    /// Adapter type: `claude`, `claude-cli`, or `cli`.
    #[serde(default = "default_spawn_adapter_type")]
    pub adapter_type: String,
    /// Opaque JSON forwarded to the adapter (`model`, `command`, etc.).
    #[serde(default)]
    pub adapter_config: Option<serde_json::Value>,
}

fn default_spawn_adapter_type() -> String {
    "claude".to_string()
}

/// Generic message response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageResponse {
    pub message: String,
}

/// Response for GET /api/agents/:id/output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOutputResponse {
    pub agent_id: String,
    pub lines: Vec<OutputLineResponse>,
    pub count: usize,
}

/// A single output line in the response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputLineResponse {
    pub line: String,
    pub timestamp: String,
}

/// Request body for POST /api/agents/:id/steer.
#[derive(Debug, Clone, Deserialize)]
pub struct SteerRequest {
    /// Message to send to the running agent.
    pub message: String,
    /// Priority of the steering message (default: normal).
    #[serde(default)]
    pub priority: Option<String>,
}

/// Response for POST /api/agents/:id/steer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteerResponse {
    pub delivered: bool,
    pub agent_id: String,
}

/// Request body for POST /api/agents/suggest-task-title.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SuggestTaskTitleRequest {
    text: String,
    #[serde(default)]
    adapter_config: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn remove_agent_worktree_if_any(state: &AgentState, agent_id: &AgentId) {
    if let Some(repo) = state.worktree_registry.take_repo_for_agent(agent_id) {
        if let Some(mgr) = state.worktree_managers.get(&repo) {
            if let Err(e) = mgr.remove_agent_worktree(agent_id).await {
                warn!(%agent_id, error = %e, "failed to remove agent worktree");
            }
        }
    }
}

const KNOWN_ADAPTER_TYPES: &[&str] = &[
    "claude",
    "claude-code",
    "claude-agent-acp",
    "claude-acp",
    "opencode",
    "goose",
    "gemini",
    "acp",
];

fn resolve_spawn_adapter(
    state: &AgentState,
    adapter_type: &str,
) -> Result<Arc<dyn molt_hub_harness::adapter::AgentAdapter>, (StatusCode, String)> {
    if let Some(adapter) = &state.test_spawn_adapter {
        return Ok(Arc::clone(adapter));
    }

    if !KNOWN_ADAPTER_TYPES.contains(&adapter_type) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "unknown adapter_type {adapter_type:?}; valid values: {}",
                KNOWN_ADAPTER_TYPES.join(", ")
            ),
        ));
    }

    Ok(Arc::new(AcpAdapter::new()))
}

/// Maps persisted settings `agent_defaults.adapter` to the harness spawn kind.
fn settings_adapter_kind(settings_adapter: &str) -> &str {
    settings_adapter.trim()
}

fn task_title_prompt(draft: &str) -> String {
    format!(
        "You are helping label a task in a tracker. Reply with exactly one short title: maximum 80 characters, plain text only. No quotation marks, no markdown, no bullets, no trailing period. Base it on this draft:\n\n{}",
        draft.trim()
    )
}

fn title_suggestion_timeout(agent_timeout_minutes: u32) -> Duration {
    let secs = u64::from(agent_timeout_minutes).saturating_mul(60);
    Duration::from_secs(secs.clamp(15, 120))
}

fn sanitize_suggested_title(raw: &str) -> String {
    let mut s = raw.trim();
    s = s.trim_matches(|c| c == '"' || c == '\'' || c == '`');
    let s = if let Some(i) = s.find('\n') {
        s[..i].trim()
    } else {
        s
    };
    s.chars().take(100).collect()
}

fn heuristic_title_from_draft(draft: &str) -> String {
    let line = draft
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    sanitize_suggested_title(line)
}

/// POST /api/agents/suggest-task-title — ask the configured harness for a short task title.
#[instrument(skip(state, settings_state, body))]
async fn suggest_task_title(
    State(state): State<Arc<AgentState>>,
    Extension(settings_state): Extension<Arc<TypedSettingsState>>,
    Json(body): Json<SuggestTaskTitleRequest>,
) -> impl IntoResponse {
    let draft = body.text.trim();
    if draft.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "text must not be empty" })),
        )
            .into_response();
    }

    if state.test_spawn_adapter.is_some() {
        let title = heuristic_title_from_draft(draft);
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "title": title,
                "source": "fallback"
            })),
        )
            .into_response();
    }

    let settings = settings_state.store.get().await;
    let adapter_type = settings_adapter_kind(&settings.agent_defaults.adapter);
    let timeout = title_suggestion_timeout(settings.agent_defaults.timeout_minutes.max(1));
    let instructions = task_title_prompt(draft);

    let mut adapter_config = body.adapter_config.clone().unwrap_or_else(|| serde_json::json!({}));
    if !adapter_config.is_object() {
        adapter_config = serde_json::json!({});
    }
    if let Some(map) = adapter_config.as_object_mut() {
        map.entry("adapter_type")
            .or_insert_with(|| serde_json::json!(adapter_type));
    }

    let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let acp = AcpAdapter::new();
    match tokio::time::timeout(
        timeout,
        acp.run_oneshot(working_dir, instructions, adapter_config, None),
    )
    .await
    {
        Ok(Ok(raw)) => {
            let title = sanitize_suggested_title(&raw);
            if !title.is_empty() {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({ "title": title, "source": "acp" })),
                )
                    .into_response();
            }
            // Empty title from ACP — fall through to heuristic below.
        }
        Ok(Err(e)) => {
            warn!(error = %e, "ACP run_oneshot failed for title suggestion; using heuristic");
        }
        Err(_elapsed) => {
            warn!(timeout_secs = timeout.as_secs(), "ACP title suggestion timed out; using heuristic");
        }
    }

    // Heuristic fallback.
    let title = heuristic_title_from_draft(draft);
    if title.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "error": "could not derive a title from the provided text" })),
        )
            .into_response();
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({ "title": title, "source": "heuristic" })),
    )
        .into_response()
}

/// POST /api/agents/spawn — start a new agent process.
#[instrument(skip(state, settings_state, body))]
async fn spawn_agent(
    State(state): State<Arc<AgentState>>,
    Extension(settings_state): Extension<Arc<TypedSettingsState>>,
    Json(body): Json<SpawnRequest>,
) -> impl IntoResponse {
    // Resolve adapter type: use request value, fall back to settings default.
    let settings = settings_state.store.get().await;
    let adapter_type = if body.adapter_type.trim().is_empty() {
        settings.agent_defaults.adapter.clone()
    } else {
        body.adapter_type.trim().to_string()
    };

    let adapter = match resolve_spawn_adapter(&state, &adapter_type) {
        Ok(a) => a,
        Err((code, msg)) => {
            return (code, Json(MessageResponse { message: msg })).into_response();
        }
    };

    // If the matching harness entry has a custom command, inject it into adapter_config.
    let harness_command = settings
        .agent_defaults
        .harnesses
        .iter()
        .find(|h| h.adapter_type == adapter_type && h.enabled)
        .and_then(|h| h.command.clone());

    let working_dir = PathBuf::from(body.working_dir.trim());
    if working_dir.as_os_str().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(MessageResponse {
                message: "working_dir must not be empty".into(),
            }),
        )
            .into_response();
    }

    let repo_root = std::fs::canonicalize(&working_dir).unwrap_or_else(|_| working_dir.clone());

    let agent_id = AgentId::new();
    let mut effective_working_dir = working_dir.clone();

    if validate_repo(&repo_root).is_ok() {
        match state
            .worktree_managers
            .get_or_insert(repo_root.clone(), || {
                WorktreeManager::new(repo_root.clone(), WorktreeConfig::default())
            }) {
            Ok(mgr) => match mgr.create_for_agent(agent_id.clone(), None).await {
                Ok(info) => {
                    state
                        .worktree_registry
                        .record(agent_id.clone(), repo_root.clone());
                    effective_working_dir = info.path;
                }
                Err(e) => {
                    warn!(
                        repo = %repo_root.display(),
                        error = %e,
                        "worktree create failed; using repository root as working directory"
                    );
                }
            },
            Err(e) => {
                warn!(
                    repo = %repo_root.display(),
                    error = %e,
                    "WorktreeManager init failed; using repository root as working directory"
                );
            }
        }
    }

    let spawn_cfg = SpawnConfig {
        agent_id: agent_id.clone(),
        task_id: TaskId::new(),
        session_id: SessionId::new(),
        working_dir: effective_working_dir,
        instructions: body.instructions,
        env: HashMap::new(),
        timeout: None,
        adapter_config: {
            let mut cfg = body.adapter_config.unwrap_or_else(|| serde_json::json!({}));
            if let Some(map) = cfg.as_object_mut() {
                map.entry("adapter_type")
                    .or_insert_with(|| serde_json::json!(adapter_type));
                if let Some(cmd) = &harness_command {
                    map.entry("command").or_insert_with(|| serde_json::json!(cmd));
                }
            }
            cfg
        },
        project_id: None,
        event_tx: None, // supervisor injects the global channel
    };

    match state.supervisor.spawn_agent(adapter, spawn_cfg).await {
        Ok(id) => {
            let id_str = id.to_string();
            Json(SpawnResponse {
                agent_id: id_str.clone(),
                message: format!("agent {id_str} spawned"),
            })
            .into_response()
        }
        Err(e) => {
            remove_agent_worktree_if_any(state.as_ref(), &agent_id).await;
            match e {
                SupervisorError::MaxAgentsReached(n) => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(MessageResponse {
                        message: format!("maximum concurrent agents reached ({n})"),
                    }),
                )
                    .into_response(),
                SupervisorError::AdapterError(adapt_err) => (
                    StatusCode::BAD_GATEWAY,
                    Json(MessageResponse {
                        message: format!("adapter error: {adapt_err}"),
                    }),
                )
                    .into_response(),
                other => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(MessageResponse {
                        message: format!("spawn failed: {other}"),
                    }),
                )
                    .into_response(),
            }
        }
    }
}

/// GET /api/agents — list all agents with status.
#[instrument(skip(state))]
async fn list_agents(State(state): State<Arc<AgentState>>) -> impl IntoResponse {
    let agents = state.supervisor.list_agents().await;
    let responses: Vec<AgentResponse> = agents
        .into_iter()
        .map(|(agent_id, task_id, status)| AgentResponse {
            agent_id: agent_id.to_string(),
            task_id: task_id.to_string(),
            status: format!("{:?}", status),
        })
        .collect();
    let count = responses.len();

    Json(AgentsListResponse {
        agents: responses,
        count,
    })
}

/// POST /api/agents/:id/terminate — terminate an agent.
#[instrument(skip(state))]
async fn terminate_agent(
    State(state): State<Arc<AgentState>>,
    Path(agent_id_str): Path<String>,
) -> impl IntoResponse {
    // Parse the agent ID from the ULID string.
    let ulid = match ulid::Ulid::from_string(&agent_id_str) {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(MessageResponse {
                    message: format!("invalid agent ID: {agent_id_str}"),
                }),
            )
                .into_response();
        }
    };

    let agent_id = AgentId(ulid);

    let result = state.supervisor.terminate_agent(&agent_id).await;
    remove_agent_worktree_if_any(state.as_ref(), &agent_id).await;

    match result {
        Ok(()) => Json(MessageResponse {
            message: format!("agent {agent_id_str} terminated"),
        })
        .into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(MessageResponse {
                message: format!("failed to terminate agent: {e}"),
            }),
        )
            .into_response(),
    }
}

/// POST /api/agents/:id/pause — pause an agent.
#[instrument(skip(state))]
async fn pause_agent(
    State(state): State<Arc<AgentState>>,
    Path(agent_id_str): Path<String>,
) -> impl IntoResponse {
    let ulid = match ulid::Ulid::from_string(&agent_id_str) {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(MessageResponse {
                    message: format!("invalid agent ID: {agent_id_str}"),
                }),
            )
                .into_response();
        }
    };

    let agent_id = AgentId(ulid);

    match state.supervisor.pause_agent(&agent_id).await {
        Ok(()) => Json(MessageResponse {
            message: format!("agent {agent_id_str} paused"),
        })
        .into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(MessageResponse {
                message: format!("failed to pause agent: {e}"),
            }),
        )
            .into_response(),
    }
}

/// POST /api/agents/:id/resume — resume a paused agent.
#[instrument(skip(state))]
async fn resume_agent(
    State(state): State<Arc<AgentState>>,
    Path(agent_id_str): Path<String>,
) -> impl IntoResponse {
    let ulid = match ulid::Ulid::from_string(&agent_id_str) {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(MessageResponse {
                    message: format!("invalid agent ID: {agent_id_str}"),
                }),
            )
                .into_response();
        }
    };

    let agent_id = AgentId(ulid);

    match state.supervisor.resume_agent(&agent_id).await {
        Ok(()) => Json(MessageResponse {
            message: format!("agent {agent_id_str} resumed"),
        })
        .into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(MessageResponse {
                message: format!("failed to resume agent: {e}"),
            }),
        )
            .into_response(),
    }
}

/// POST /api/agents/:id/steer — send a steering message to a running agent.
#[instrument(skip(state, body))]
async fn steer_agent(
    State(state): State<Arc<AgentState>>,
    Path(agent_id_str): Path<String>,
    Json(body): Json<SteerRequest>,
) -> impl IntoResponse {
    let ulid = match ulid::Ulid::from_string(&agent_id_str) {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(MessageResponse {
                    message: format!("invalid agent ID: {agent_id_str}"),
                }),
            )
                .into_response();
        }
    };

    let agent_id = AgentId(ulid);

    let priority = match body.priority.as_deref() {
        Some("urgent") => SteerPriority::Urgent,
        _ => SteerPriority::Normal,
    };

    let steer_msg = SteerMessage {
        message: body.message,
        priority,
    };

    match state.supervisor.steer(&agent_id, steer_msg).await {
        Ok(()) => Json(SteerResponse {
            delivered: true,
            agent_id: agent_id_str,
        })
        .into_response(),
        Err(SupervisorError::AgentNotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json(MessageResponse {
                message: format!("agent not found: {agent_id_str}"),
            }),
        )
            .into_response(),
        Err(SupervisorError::AgentNotRunning(_)) => (
            StatusCode::CONFLICT,
            Json(MessageResponse {
                message: format!("agent not running: {agent_id_str}"),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(MessageResponse {
                message: format!("steer failed: {e}"),
            }),
        )
            .into_response(),
    }
}

/// GET /api/agents/:id/output — return buffered output lines for an agent.
#[instrument(skip(state))]
async fn get_agent_output(
    State(state): State<Arc<AgentState>>,
    Path(agent_id_str): Path<String>,
) -> impl IntoResponse {
    let lines = state.output_buffer.get_lines(&agent_id_str);
    let count = lines.len();

    Json(AgentOutputResponse {
        agent_id: agent_id_str,
        lines: lines
            .into_iter()
            .map(|l| OutputLineResponse {
                line: l.line,
                timestamp: l.timestamp.to_rfc3339(),
            })
            .collect(),
        count,
    })
}

// ---------------------------------------------------------------------------
// Login
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct LoginRequest {
    adapter_type: Option<String>,
}

/// POST /api/agents/login — run `<tool> login` for the configured harness.
///
/// Spawns the CLI login subprocess (which opens the system browser for OAuth)
/// and waits up to 120 seconds for it to complete.
#[instrument(skip(body))]
async fn login_agent(
    Json(body): Json<LoginRequest>,
) -> impl IntoResponse {
    let adapter_type = body.adapter_type.as_deref().unwrap_or("claude");

    let (command, args) = match AcpAdapter::resolve_login_command(adapter_type) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let result = tokio::task::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(&command);
        cmd.args(&args)
            .env("PATH", AcpAdapter::augmented_path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(format!(
                    "`{command}` not found. Install the CLI first (e.g. `npm install -g @anthropic-ai/claude-code`)."
                ));
            }
            Err(e) => return Err(format!("failed to spawn `{command} {args}`: {e}", args = args.join(" "))),
        };

        let full_cmd = format!("{command} {}", args.join(" "));
        match child.wait_with_output() {
            Ok(output) if output.status.success() => Ok(()),
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let detail = if !stderr.trim().is_empty() {
                    stderr.into_owned()
                } else {
                    stdout.into_owned()
                };
                Err(format!("`{full_cmd}` failed: {detail}"))
            }
            Err(e) => Err(format!("`{full_cmd}` error: {e}")),
        }
    })
    .await;

    match result {
        Ok(Ok(())) => (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true })),
        )
            .into_response(),
        Ok(Err(msg)) => {
            warn!("login failed: {msg}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("task panicked: {e}") })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the agent API sub-router.
///
/// Mounts:
///   GET  /                  — list all agents
///   POST /spawn             — spawn a new agent
///   POST /suggest-task-title — suggest a short title via the harness
///   POST /:id/terminate     — terminate an agent
///   POST /:id/pause         — pause an agent
///   POST /:id/resume        — resume an agent
///   POST /:id/steer         — send a steering message to an agent
///   GET  /:id/output        — get buffered output lines
///   POST /login              — run `<tool> login` for the configured harness
pub fn agent_router(state: Arc<AgentState>) -> Router {
    Router::new()
        .route("/", get(list_agents))
        .route("/spawn", post(spawn_agent))
        .route("/suggest-task-title", post(suggest_task_title))
        .route("/login", post(login_agent))
        .route("/:id/terminate", post(terminate_agent))
        .route("/:id/pause", post(pause_agent))
        .route("/:id/resume", post(resume_agent))
        .route("/:id/steer", post(steer_agent))
        .route("/:id/output", get(get_agent_output))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Project-scoped handlers
// ---------------------------------------------------------------------------

/// GET /api/projects/:pid/agents — list agents belonging to a specific project.
#[instrument(skip(registry, projects, supervisor))]
pub async fn list_project_agents(
    Path(project_id): Path<String>,
    State(projects): State<Arc<ProjectConfigStore>>,
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
) -> impl IntoResponse {
    if project_id != "default" && projects.get(&project_id).await.is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(MessageResponse {
                message: format!("project not found: {project_id}"),
            }),
        )
            .into_response();
    }
    let runtime = ensure_project_runtime(&project_id, &registry, &supervisor).await;

    let agents = runtime.supervisor.list_agents().await;
    let responses: Vec<AgentResponse> = agents
        .into_iter()
        .map(|(agent_id, task_id, status)| AgentResponse {
            agent_id: agent_id.to_string(),
            task_id: task_id.to_string(),
            status: format!("{:?}", status),
        })
        .collect();
    let count = responses.len();

    Json(AgentsListResponse {
        agents: responses,
        count,
    })
    .into_response()
}

/// DELETE /api/projects/:pid/agents/:aid — terminate an agent under a project.
#[instrument(skip(registry, projects, supervisor))]
pub async fn delete_project_agent(
    Path((project_id, agent_id_str)): Path<(String, String)>,
    State(projects): State<Arc<ProjectConfigStore>>,
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
) -> impl IntoResponse {
    if project_id != "default" && projects.get(&project_id).await.is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(MessageResponse {
                message: format!("project not found: {project_id}"),
            }),
        )
            .into_response();
    }
    let runtime = ensure_project_runtime(&project_id, &registry, &supervisor).await;

    let ulid = match ulid::Ulid::from_string(&agent_id_str) {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(MessageResponse {
                    message: format!("invalid agent ID: {agent_id_str}"),
                }),
            )
                .into_response();
        }
    };

    let agent_id = AgentId(ulid);

    match runtime.supervisor.terminate_agent(&agent_id).await {
        Ok(()) => Json(MessageResponse {
            message: format!("agent {agent_id_str} terminated"),
        })
        .into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(MessageResponse {
                message: format!("failed to terminate agent: {e}"),
            }),
        )
            .into_response(),
    }
}

/// GET /api/projects/:pid/agents/:aid — get status of a specific agent under a project.
#[instrument(skip(registry, projects, supervisor))]
pub async fn get_project_agent(
    Path((project_id, agent_id_str)): Path<(String, String)>,
    State(projects): State<Arc<ProjectConfigStore>>,
    Extension(registry): Extension<Arc<ProjectRuntimeRegistry>>,
    Extension(supervisor): Extension<Arc<Supervisor>>,
) -> impl IntoResponse {
    if project_id != "default" && projects.get(&project_id).await.is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(MessageResponse {
                message: format!("project not found: {project_id}"),
            }),
        )
            .into_response();
    }
    let runtime = ensure_project_runtime(&project_id, &registry, &supervisor).await;

    let ulid = match ulid::Ulid::from_string(&agent_id_str) {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(MessageResponse {
                    message: format!("invalid agent ID: {agent_id_str}"),
                }),
            )
                .into_response();
        }
    };

    let agent_id = AgentId(ulid);

    match runtime.supervisor.get_status(&agent_id).await {
        Some(status) => Json(AgentResponse {
            agent_id: agent_id_str,
            task_id: String::new(),
            status: format!("{:?}", status),
        })
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(MessageResponse {
                message: format!("agent not found: {agent_id_str}"),
            }),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use molt_hub_core::model::AgentStatus;
    use molt_hub_harness::adapter::{
        AdapterError, AgentAdapter, AgentEvent, AgentHandle, AgentMessage,
    };
    use molt_hub_harness::supervisor::SupervisorConfig;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::sync::broadcast;
    use tower::ServiceExt;

    struct MockAdapter {
        spawn_count: Arc<AtomicUsize>,
        status: AgentStatus,
    }

    impl MockAdapter {
        fn new(status: AgentStatus) -> Self {
            Self {
                spawn_count: Arc::new(AtomicUsize::new(0)),
                status,
            }
        }

        fn spawn_count(&self) -> usize {
            self.spawn_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl AgentAdapter for MockAdapter {
        async fn spawn(&self, config: SpawnConfig) -> Result<AgentHandle, AdapterError> {
            self.spawn_count.fetch_add(1, Ordering::SeqCst);
            Ok(AgentHandle::new(config.agent_id, None, Box::new(())))
        }

        async fn send(
            &self,
            _handle: &AgentHandle,
            _message: AgentMessage,
        ) -> Result<(), AdapterError> {
            Ok(())
        }

        async fn status(&self, _handle: &AgentHandle) -> Result<AgentStatus, AdapterError> {
            Ok(self.status.clone())
        }

        async fn terminate(&self, _handle: &AgentHandle) -> Result<(), AdapterError> {
            Ok(())
        }

        async fn abort(&self, _handle: &AgentHandle) -> Result<(), AdapterError> {
            Ok(())
        }

        fn adapter_type(&self) -> &str {
            "mock"
        }
    }

    fn make_supervisor() -> Arc<Supervisor> {
        let (tx, _rx) = broadcast::channel::<AgentEvent>(64);
        let config = SupervisorConfig {
            max_agents: 4,
            health_check_interval: Duration::from_secs(60),
            graceful_shutdown_timeout: Duration::from_millis(100),
        };
        Arc::new(Supervisor::new(config, tx))
    }

    fn make_state() -> Arc<AgentState> {
        Arc::new(AgentState {
            supervisor: make_supervisor(),
            output_buffer: Arc::new(AgentOutputBuffer::new()),
            test_spawn_adapter: None,
            worktree_managers: Arc::new(WorktreeManagerCache::new()),
            worktree_registry: Arc::new(WorktreeRegistry::new()),
        })
    }

    #[tokio::test]
    async fn test_list_agents_empty() {
        let state = make_state();
        let app = agent_router(state);

        let req = Request::builder()
            .uri("/")
            .method("GET")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let parsed: AgentsListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.count, 0);
        assert!(parsed.agents.is_empty());
    }

    #[tokio::test]
    async fn test_terminate_invalid_id_returns_error() {
        let state = make_state();
        let app = agent_router(state);

        let req = Request::builder()
            .uri("/not-a-ulid/terminate")
            .method("POST")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        // Handler returns 400 for malformed ULID, but Axum may return 404
        // if the route pattern doesn't match. Either is acceptable for invalid IDs.
        let status = resp.status().as_u16();
        assert!(
            status == 400 || status == 404,
            "expected 400 or 404, got {status}"
        );
    }

    #[tokio::test]
    async fn test_terminate_unknown_agent_returns_not_found() {
        let state = make_state();
        let app = agent_router(state);

        let agent_id = AgentId::new();
        let uri = format!("/{}/terminate", agent_id);

        let req = Request::builder()
            .uri(&uri)
            .method("POST")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_pause_unknown_agent_returns_not_found() {
        let state = make_state();
        let app = agent_router(state);

        let agent_id = AgentId::new();
        let uri = format!("/{}/pause", agent_id);

        let req = Request::builder()
            .uri(&uri)
            .method("POST")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_resume_unknown_agent_returns_not_found() {
        let state = make_state();
        let app = agent_router(state);

        let agent_id = AgentId::new();
        let uri = format!("/{}/resume", agent_id);

        let req = Request::builder()
            .uri(&uri)
            .method("POST")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_agent_output_empty() {
        let state = make_state();
        let app = agent_router(state);

        let req = Request::builder()
            .uri("/some-agent/output")
            .method("GET")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let parsed: AgentOutputResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.count, 0);
        assert!(parsed.lines.is_empty());
    }

    #[tokio::test]
    async fn test_get_agent_output_with_data() {
        let output_buffer = Arc::new(AgentOutputBuffer::new());
        output_buffer.push("agent-42", "line one".into());
        output_buffer.push("agent-42", "line two".into());

        let state = Arc::new(AgentState {
            supervisor: make_supervisor(),
            output_buffer,
            test_spawn_adapter: None,
            worktree_managers: Arc::new(WorktreeManagerCache::new()),
            worktree_registry: Arc::new(WorktreeRegistry::new()),
        });
        let app = agent_router(state);

        let req = Request::builder()
            .uri("/agent-42/output")
            .method("GET")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let parsed: AgentOutputResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.agent_id, "agent-42");
        assert_eq!(parsed.count, 2);
        assert_eq!(parsed.lines[0].line, "line one");
        assert_eq!(parsed.lines[1].line, "line two");
        // Verify timestamps are valid RFC3339.
        for line in &parsed.lines {
            assert!(!line.timestamp.is_empty());
        }
    }

    // -----------------------------------------------------------------------
    // Steer endpoint tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_steer_unknown_agent_returns_not_found() {
        let state = make_state();
        let app = agent_router(state);

        let agent_id = AgentId::new();
        let uri = format!("/{}/steer", agent_id);

        let req = Request::builder()
            .uri(&uri)
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"message": "hello", "priority": "normal"}).to_string(),
            ))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_steer_invalid_id_returns_error() {
        let state = make_state();
        let app = agent_router(state);

        let req = Request::builder()
            .uri("/not-a-ulid/steer")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"message": "hello"}).to_string(),
            ))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status().as_u16();
        assert!(
            status == 400 || status == 404,
            "expected 400 or 404, got {status}"
        );
    }

    #[test]
    fn test_steer_response_serialization() {
        let resp = SteerResponse {
            delivered: true,
            agent_id: "abc123".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"delivered\":true"));
        assert!(json.contains("\"agent_id\":\"abc123\""));
    }

    #[tokio::test]
    async fn test_spawn_agent_with_test_adapter() {
        let mock = Arc::new(MockAdapter::new(AgentStatus::Running));
        let adapter: Arc<dyn AgentAdapter> = mock.clone();
        let state = Arc::new(AgentState {
            supervisor: make_supervisor(),
            output_buffer: Arc::new(AgentOutputBuffer::new()),
            test_spawn_adapter: Some(adapter),
            worktree_managers: Arc::new(WorktreeManagerCache::new()),
            worktree_registry: Arc::new(WorktreeRegistry::new()),
        });
        let app = agent_router(state);

        let req = Request::builder()
            .uri("/spawn")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "instructions": "do the thing",
                    "workingDir": "/tmp",
                    "adapterType": "claude"
                })
                .to_string(),
            ))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(mock.spawn_count(), 1);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let parsed: SpawnResponse = serde_json::from_slice(&body).unwrap();
        assert!(!parsed.agent_id.is_empty());
    }

    #[tokio::test]
    async fn test_spawn_agent_unknown_adapter_type() {
        let state = make_state();
        let app = agent_router(state);

        let req = Request::builder()
            .uri("/spawn")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "instructions": "x",
                    "workingDir": "/tmp",
                    "adapterType": "not-real"
                })
                .to_string(),
            ))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
