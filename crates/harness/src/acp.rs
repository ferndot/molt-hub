//! ACP (Agent Client Protocol) adapter — drives any ACP-compatible agent over stdio.
//!
//! Spawns the target agent as a subprocess and communicates via JSON-RPC 2.0 over
//! stdin/stdout using the `agent-client-protocol` crate. Because the ACP `Client`
//! trait requires `?Send` futures, all ACP code runs inside a dedicated OS thread
//! that owns a single-threaded Tokio runtime + `LocalSet`.
//!
//! Supported `adapter_type` values:
//!   - `"claude"` / `"claude-code"` (default) → `claude-agent-acp`
//!   - `"opencode"`          → `opencode acp`
//!   - `"goose"`             → `goose acp`
//!   - `"gemini"`            → `gemini --acp`
//!   - `"claude-agent-acp"` / `"claude-acp"` → `claude-agent-acp`
//!   - `"acp"` (generic)    → requires `adapter_config["command"]`
//!
//! Any profile can be overridden by setting `adapter_config["command"]` and
//! optionally `adapter_config["args"]`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use agent_client_protocol::Agent as _;
use tokio::sync::{broadcast, mpsc};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, error, info, warn};

use molt_hub_core::model::{AgentId, AgentStatus};

use crate::adapter::{
    AdapterError, AgentAdapter, AgentEvent, AgentHandle, AgentMessage, SpawnConfig,
};

// ---------------------------------------------------------------------------
// AcpInternal — stored inside AgentHandle::internal
// ---------------------------------------------------------------------------

/// State shared between the async adapter methods and the ACP background thread.
///
/// This type must be `Send + Sync` so it can live in `AgentHandle::internal`
/// (which is `Box<dyn Any + Send + Sync>`). All ACP-specific (`!Send`) state
/// lives exclusively inside the dedicated thread; only `Send` primitives
/// (channels, atomics, the thread handle) are stored here.
pub struct AcpInternal {
    /// Send follow-up text messages to the ACP thread.
    msg_tx: mpsc::Sender<String>,
    /// Marks the agent as terminated (channel dropped or explicit kill).
    terminated: Arc<AtomicBool>,
    /// Tracks logical agent status.
    status: Arc<std::sync::RwLock<AgentStatus>>,
    /// Used by callers to subscribe to agent events.
    pub event_tx: broadcast::Sender<AgentEvent>,
    /// Send `true` (approve) or `false` (reject) to unblock a pending `request_permission` call.
    /// The handler layer can use this when it receives an `"approve:<id>"` / `"reject:<id>"`
    /// steer message.
    pub approve_tx: broadcast::Sender<bool>,
}

// Safety: `mpsc::Sender<String>` and `broadcast::Sender<AgentEvent>` are `Send`;
// `AtomicBool` and `std::sync::RwLock` are `Send + Sync`. The thread handle is not
// stored here to keep things simple — the thread self-terminates when `msg_tx` drops.

// ---------------------------------------------------------------------------
// AcpClientImpl — implements acp::Client, lives inside the LocalSet thread
// ---------------------------------------------------------------------------

struct AcpClientImpl {
    agent_id: AgentId,
    /// When `Some`, text chunks are accumulated here (oneshot mode).
    /// When `None`, chunks are broadcast via `event_tx` (spawn mode).
    output: Option<Rc<RefCell<String>>>,
    event_tx: broadcast::Sender<AgentEvent>,
    /// Receives approval decisions for `request_permission` calls.
    /// `None` in oneshot mode (permissions are auto-approved there).
    permission_rx: Option<Rc<RefCell<broadcast::Receiver<bool>>>>,
}

#[async_trait::async_trait(?Send)]
impl agent_client_protocol::Client for AcpClientImpl {
    async fn request_permission(
        &self,
        args: agent_client_protocol::RequestPermissionRequest,
    ) -> agent_client_protocol::Result<agent_client_protocol::RequestPermissionResponse> {
        use agent_client_protocol::{PermissionOptionKind, RequestPermissionOutcome, SelectedPermissionOutcome};

        // Stable request_id from the ACP tool call id for UI correlation.
        let request_id = args.tool_call.tool_call_id.to_string();

        // Extract a human-readable tool name from the ACP tool call title if present.
        let tool_name = args.tool_call.fields.title.clone().unwrap_or_default();

        // Human-readable option labels for the UI.
        let options: Vec<String> = args
            .options
            .iter()
            .map(|o| format!("{:?}: {}", o.kind, o.name))
            .collect();

        // Emit the approval-required event so the UI can surface it to the user.
        let _ = self.event_tx.send(AgentEvent::ToolApprovalRequired {
            agent_id: self.agent_id.clone(),
            request_id: request_id.clone(),
            tool_name: tool_name.clone(),
            options,
            timestamp: Utc::now(),
        });

        // If we have a permission channel, wait for the user's decision (5-minute timeout).
        // On timeout or channel error we fall through to auto-approve so the agent isn't stuck.
        // TODO mh-rzm: wire the UI decision through AcpInternal::approve_tx → permission_rx
        // to enable real blocking approval instead of the auto-approve timeout fallback.
        let approved = if let Some(rx_cell) = &self.permission_rx {
            info!(
                agent_id = %self.agent_id,
                request_id = %request_id,
                tool_name = %tool_name,
                "ACP: waiting for user permission decision",
            );
            let decision = tokio::time::timeout(
                std::time::Duration::from_secs(300),
                async {
                    loop {
                        let result = rx_cell.borrow_mut().recv().await;
                        match result {
                            Ok(v) => break v,
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => break true,
                        }
                    }
                },
            )
            .await;

            match decision {
                Ok(v) => v,
                Err(_timeout) => {
                    warn!(
                        agent_id = %self.agent_id,
                        request_id = %request_id,
                        "ACP: permission decision timed out after 5 minutes, auto-approving",
                    );
                    true
                }
            }
        } else {
            // Oneshot mode — no interactive permission channel; auto-approve.
            debug!(
                agent_id = %self.agent_id,
                request_id = %request_id,
                "ACP: auto-approving permission request (oneshot mode)",
            );
            true
        };

        let outcome = if approved {
            let chosen = args
                .options
                .iter()
                .find(|o| o.kind == PermissionOptionKind::AllowOnce)
                .or_else(|| args.options.first());

            if let Some(opt) = chosen {
                debug!(
                    agent_id = %self.agent_id,
                    option_id = %opt.option_id,
                    approved = true,
                    "ACP: permission approved",
                );
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                    opt.option_id.clone(),
                ))
            } else {
                warn!(
                    agent_id = %self.agent_id,
                    "ACP: approved but no permission options provided, returning Cancelled",
                );
                RequestPermissionOutcome::Cancelled
            }
        } else {
            info!(
                agent_id = %self.agent_id,
                request_id = %request_id,
                "ACP: permission rejected by user",
            );
            RequestPermissionOutcome::Cancelled
        };

        Ok(agent_client_protocol::RequestPermissionResponse::new(outcome))
    }

    async fn session_notification(
        &self,
        args: agent_client_protocol::SessionNotification,
    ) -> agent_client_protocol::Result<()> {
        use agent_client_protocol::{ContentBlock, SessionUpdate, ToolCallStatus};

        match args.update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                if let ContentBlock::Text(t) = chunk.content {
                    if !t.text.is_empty() {
                        if let Some(ref buf) = self.output {
                            // Oneshot mode: accumulate locally.
                            buf.borrow_mut().push_str(&t.text);
                        } else {
                            // Spawn mode: broadcast to subscribers.
                            let _ = self.event_tx.send(AgentEvent::Output {
                                agent_id: self.agent_id.clone(),
                                content: t.text,
                                timestamp: Utc::now(),
                            });
                        }
                    }
                }
            }
            SessionUpdate::ToolCall(tc) => {
                let _ = self.event_tx.send(AgentEvent::ToolCall {
                    agent_id: self.agent_id.clone(),
                    call_id: tc.tool_call_id.0.to_string(),
                    tool_name: tc.title.clone(),
                    input: tc.raw_input.unwrap_or(serde_json::Value::Null),
                    timestamp: Utc::now(),
                });
            }
            SessionUpdate::ToolCallUpdate(tcu) => {
                let status = tcu.fields.status.unwrap_or(ToolCallStatus::Pending);
                if matches!(status, ToolCallStatus::Completed | ToolCallStatus::Failed) {
                    let is_error = matches!(status, ToolCallStatus::Failed);
                    let _ = self.event_tx.send(AgentEvent::ToolResult {
                        agent_id: self.agent_id.clone(),
                        call_id: tcu.tool_call_id.0.to_string(),
                        output: tcu.fields.raw_output.unwrap_or(serde_json::Value::Null),
                        is_error,
                        timestamp: Utc::now(),
                    });
                }
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                if let ContentBlock::Text(t) = chunk.content {
                    if !t.text.is_empty() {
                        let _ = self.event_tx.send(AgentEvent::ThinkingChunk {
                            agent_id: self.agent_id.clone(),
                            content: t.text,
                            timestamp: Utc::now(),
                        });
                    }
                }
            }
            _other => {}
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// AcpAdapter
// ---------------------------------------------------------------------------

/// `AgentAdapter` that connects to any ACP-compatible agent over stdio.
#[derive(Debug, Clone, Default)]
pub struct AcpAdapter;

impl AcpAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Resolve the command + args for a given adapter_type / adapter_config.
    fn resolve_command(
        adapter_type: &str,
        adapter_config: &serde_json::Value,
    ) -> Result<(String, Vec<String>), AdapterError> {
        // Allow explicit override via config["command"] + config["args"].
        if let Some(cmd) = adapter_config.get("command").and_then(|v| v.as_str()) {
            let args: Vec<String> = adapter_config
                .get("args")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|a| a.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            return Ok((cmd.to_string(), args));
        }

        let (cmd, args): (&str, &[&str]) = match adapter_type {
            "claude" | "claude-code" | "claude-agent-acp" | "claude-acp" => {
                ("claude-agent-acp", &[])
            }
            "opencode" => ("opencode", &["acp"]),
            "goose" => ("goose", &["acp"]),
            "gemini" => ("gemini", &["--acp"]),
            "acp" => {
                return Err(AdapterError::SpawnFailed(
                    "adapter_type \"acp\" requires adapter_config[\"command\"]".to_string(),
                ));
            }
            other => {
                return Err(AdapterError::SpawnFailed(format!(
                    "unknown ACP adapter_type: {other}"
                )));
            }
        };

        // Prefer the project-local node_modules/.bin/ binary over the global PATH,
        // so that `npm install` in the project root is sufficient.
        let resolved_cmd = Self::resolve_local_bin(cmd).unwrap_or_else(|| cmd.to_string());
        Ok((resolved_cmd, args.iter().map(|s| s.to_string()).collect()))
    }

    /// Resolve the command + args needed to run `<tool> login` for a given adapter type.
    pub fn resolve_login_command(adapter_type: &str) -> Result<(String, Vec<String>), AdapterError> {
        let (cmd, args): (&str, &[&str]) = match adapter_type {
            "claude" | "claude-code" | "claude-agent-acp" | "claude-acp" => {
                ("claude", &["auth", "login"])
            }
            "opencode" => ("opencode", &["login"]),
            "goose" => ("goose", &["login"]),
            other => {
                return Err(AdapterError::SpawnFailed(format!(
                    "login not supported for adapter type: {other}"
                )));
            }
        };
        Ok((cmd.to_string(), args.iter().map(|s| s.to_string()).collect()))
    }

    /// Check for `<name>` in `node_modules/.bin/` relative to cwd, returning
    /// the full path if found. Falls back to the bare name (PATH lookup) otherwise.
    fn resolve_local_bin(name: &str) -> Option<String> {
        let bin = std::env::current_dir()
            .ok()?
            .join("node_modules")
            .join(".bin")
            .join(name);
        bin.exists().then(|| bin.to_string_lossy().into_owned())
    }

    /// Find the directory containing the `node` binary, checking nvm and common
    /// system paths. Returns `None` if node cannot be located.
    ///
    /// The server process may have a minimal PATH (e.g. launched by a GUI or
    /// preview tool) that doesn't include the user's nvm-managed node. We need
    /// to inject it so that `#!/usr/bin/env node` scripts in node_modules/.bin/
    /// actually execute.
    fn node_bin_dir() -> Option<std::path::PathBuf> {
        // 1. nvm: ~/.nvm/versions/node/<version>/bin — pick the highest version.
        let home = std::env::var("HOME").ok()?;
        let nvm_node_dir = std::path::PathBuf::from(&home)
            .join(".nvm")
            .join("versions")
            .join("node");
        if let Ok(entries) = std::fs::read_dir(&nvm_node_dir) {
            let mut versions: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .collect();
            // Sort by directory name — semver-ish lexicographic order is fine for
            // picking the latest major.
            versions.sort_by_key(|e| e.file_name());
            for entry in versions.into_iter().rev() {
                let bin = entry.path().join("bin");
                if bin.join("node").exists() {
                    return Some(bin);
                }
            }
        }
        // 2. Common system/homebrew locations.
        for prefix in &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
            let dir = std::path::PathBuf::from(prefix);
            if dir.join("node").exists() {
                return Some(dir);
            }
        }
        None
    }

    /// Build a PATH string that prepends the node bin dir (if found) so that
    /// `#!/usr/bin/env node` scripts work even when the server has a minimal PATH.
    pub fn augmented_path() -> String {
        let current = std::env::var("PATH").unwrap_or_default();

        // Common user-local bin dirs that may not be in the server's inherited PATH.
        let mut extra: Vec<String> = Vec::new();
        if let Some(home) = std::env::var_os("HOME") {
            let home = std::path::Path::new(&home);
            for rel in &[".local/bin", ".cargo/bin"] {
                let dir = home.join(rel);
                if dir.exists() {
                    extra.push(dir.to_string_lossy().into_owned());
                }
            }
        }
        if let Some(node_dir) = Self::node_bin_dir() {
            extra.push(node_dir.to_string_lossy().into_owned());
        }

        if extra.is_empty() {
            current
        } else {
            format!("{}:{}", extra.join(":"), current)
        }
    }

    /// Return a user-friendly error message when a tool binary is not found.
    fn not_found_hint(command: &str) -> String {
        // Strip any path prefix to get the bare binary name for matching.
        let name = std::path::Path::new(command)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(command);
        let hint = match name {
            "claude-agent-acp" | "claude" | "claude-code" => {
                "run `npm install` in the project root"
            }
            "opencode" => "npm install -g opencode-ai  (or: npx opencode-ai)",
            "goose" => "pip install goose-ai  (or: brew install block/goose/goose)",
            "gemini" => "npm install -g @google/gemini-cli",
            _ => return format!("{command:?} not found in PATH"),
        };
        format!("{command:?} not found. {hint}")
    }

    /// Spawn an ACP agent, send a single prompt, collect all output text,
    /// wait for EndTurn, and return the collected text.
    ///
    /// This is a synchronous one-shot call: it blocks until the agent finishes
    /// the prompt (or the timeout elapses) and returns the full response text.
    pub async fn run_oneshot(
        &self,
        working_dir: std::path::PathBuf,
        instructions: String,
        adapter_config: serde_json::Value,
        timeout: Option<Duration>,
    ) -> Result<String, AdapterError> {
        let adapter_type = adapter_config
            .get("adapter_type")
            .and_then(|v| v.as_str())
            .unwrap_or("claude");

        let (command, acp_args) = Self::resolve_command(adapter_type, &adapter_config)?;

        let (result_tx, result_rx) =
            tokio::sync::oneshot::channel::<Result<String, String>>();

        let command_clone = command.clone();

        std::thread::Builder::new()
            .name("acp-oneshot".to_string())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build ACP oneshot runtime");

                let local = tokio::task::LocalSet::new();

                local.block_on(&rt, async move {
                    let mut cmd = tokio::process::Command::new(&command_clone);
                    cmd.args(&acp_args)
                        .current_dir(&working_dir)
                        .env("PATH", AcpAdapter::augmented_path())
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::piped())
                        .kill_on_drop(true);

                    let mut child = match cmd.spawn() {
                        Ok(c) => c,
                        Err(e) => {
                            let msg = if e.kind() == std::io::ErrorKind::NotFound {
                                Self::not_found_hint(&command_clone)
                            } else {
                                format!("failed to spawn ACP agent \"{command_clone}\": {e}")
                            };
                            let _ = result_tx.send(Err(msg));
                            return;
                        }
                    };

                    let outgoing = child
                        .stdin
                        .take()
                        .expect("ACP agent stdin should be piped")
                        .compat_write();
                    let incoming = child
                        .stdout
                        .take()
                        .expect("ACP agent stdout should be piped")
                        .compat();

                    let collected = Rc::new(RefCell::new(String::new()));
                    let (event_tx, _) = broadcast::channel::<AgentEvent>(64);

                    let client_impl = AcpClientImpl {
                        agent_id: AgentId::new(),
                        output: Some(Rc::clone(&collected)),
                        event_tx,
                        permission_rx: None,
                    };

                    let (conn, handle_io) = agent_client_protocol::ClientSideConnection::new(
                        client_impl,
                        outgoing,
                        incoming,
                        |fut| {
                            tokio::task::spawn_local(fut);
                        },
                    );

                    tokio::task::spawn_local(async move {
                        if let Err(e) = handle_io.await {
                            debug!("ACP oneshot I/O pump ended: {e}");
                        }
                    });

                    // Initialize.
                    if let Err(e) = conn
                        .initialize(
                            agent_client_protocol::InitializeRequest::new(
                                agent_client_protocol::ProtocolVersion::V1,
                            )
                            .client_info(agent_client_protocol::Implementation::new(
                                "molt-hub",
                                "0.1.0",
                            )),
                        )
                        .await
                    {
                        let _ = result_tx.send(Err(format!("ACP oneshot initialize failed: {e}")));
                        return;
                    }

                    // New session.
                    let session_resp = match conn
                        .new_session(agent_client_protocol::NewSessionRequest::new(
                            working_dir.clone(),
                        ))
                        .await
                    {
                        Ok(r) => r,
                        Err(e) => {
                            let _ = result_tx.send(Err(format!("ACP oneshot new_session failed: {e}")));
                            return;
                        }
                    };

                    // Send the prompt and wait for the response.
                    match conn
                        .prompt(agent_client_protocol::PromptRequest::new(
                            session_resp.session_id,
                            vec![instructions.into()],
                        ))
                        .await
                    {
                        Ok(_resp) => {
                            let result = collected.borrow().clone();
                            let _ = result_tx.send(Ok(result));
                        }
                        Err(e) => {
                            let _ = result_tx.send(Err(format!("ACP oneshot prompt failed: {e}")));
                        }
                    }
                });
            })
            .map_err(|e| AdapterError::SpawnFailed(format!("failed to spawn ACP oneshot thread: {e}")))?;

        let recv_future = async move {
            result_rx
                .await
                .map_err(|_| AdapterError::SpawnFailed("ACP oneshot thread dropped result channel".into()))?
                .map_err(AdapterError::SpawnFailed)
        };

        match timeout {
            Some(dur) => tokio::time::timeout(dur, recv_future)
                .await
                .map_err(|_| AdapterError::Timeout)?,
            None => recv_future.await,
        }
    }
}

#[async_trait]
impl AgentAdapter for AcpAdapter {
    fn adapter_type(&self) -> &str {
        "claude"
    }

    async fn spawn(&self, config: SpawnConfig) -> Result<AgentHandle, AdapterError> {
        let adapter_type = config
            .adapter_config
            .get("adapter_type")
            .and_then(|v| v.as_str())
            .unwrap_or("claude");

        // Resolve command at spawn time so we can report errors before the thread starts.
        let (command, acp_args) = Self::resolve_command(adapter_type, &config.adapter_config)?;

        let agent_id = config.agent_id.clone();
        let working_dir = config.working_dir.clone();
        let instructions = config.instructions.clone();
        let env_vars: Vec<(String, String)> = config.env.clone().into_iter().collect();

        // Use the global event channel from SpawnConfig if provided,
        // otherwise create a local one (for tests / standalone usage).
        let event_tx = config
            .event_tx
            .unwrap_or_else(|| broadcast::channel::<AgentEvent>(256).0);
        let event_tx_thread = event_tx.clone();

        // MPSC channel: adapter → thread (follow-up messages).
        let (msg_tx, mut msg_rx) = mpsc::channel::<String>(64);

        // Broadcast channel for permission decisions: handler → ACP thread.
        let (approve_tx, _) = broadcast::channel::<bool>(4);
        let approve_tx_thread = approve_tx.clone();

        let terminated = Arc::new(AtomicBool::new(false));
        let terminated_thread = Arc::clone(&terminated);

        let status = Arc::new(std::sync::RwLock::new(AgentStatus::Running));
        let status_thread = Arc::clone(&status);

        let agent_id_clone = agent_id.clone();

        // Spawn the dedicated thread.  All !Send ACP code lives here.
        std::thread::Builder::new()
            .name(format!("acp-{agent_id}"))
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build ACP runtime");

                let local = tokio::task::LocalSet::new();

                local.block_on(&rt, async move {
                    // Build the subprocess command.
                    let mut cmd = tokio::process::Command::new(&command);
                    cmd.args(&acp_args)
                        .current_dir(&working_dir)
                        .env("PATH", AcpAdapter::augmented_path())
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::piped())
                        .kill_on_drop(true);

                    for (k, v) in &env_vars {
                        cmd.env(k, v);
                    }

                    let mut child = match cmd.spawn() {
                        Ok(c) => c,
                        Err(e) => {
                            let msg = if e.kind() == std::io::ErrorKind::NotFound {
                                AcpAdapter::not_found_hint(&command)
                            } else {
                                format!("failed to spawn ACP agent \"{command}\": {e}")
                            };
                            error!(agent_id = %agent_id_clone, "{msg}");
                            let _ = event_tx_thread.send(AgentEvent::Error {
                                agent_id: agent_id_clone.clone(),
                                message: msg,
                                timestamp: Utc::now(),
                            });
                            terminated_thread.store(true, Ordering::Relaxed);
                            return;
                        }
                    };

                    let outgoing = child
                        .stdin
                        .take()
                        .expect("ACP agent stdin should be piped")
                        .compat_write();
                    let incoming = child
                        .stdout
                        .take()
                        .expect("ACP agent stdout should be piped")
                        .compat();

                    // Create approval broadcast channel for interactive permission decisions.
                    // The receiver is held by the client impl; the sender is surfaced via
                    // AcpInternal::approve_tx so the handler layer can send decisions.
                    let permission_rx_local = {
                        let rx = approve_tx_thread.subscribe();
                        Some(Rc::new(RefCell::new(rx)))
                    };

                    // Create ACP client-side connection.
                    let client_impl = AcpClientImpl {
                        agent_id: agent_id_clone.clone(),
                        output: None,
                        event_tx: event_tx_thread.clone(),
                        permission_rx: permission_rx_local,
                    };

                    let (conn, handle_io) = agent_client_protocol::ClientSideConnection::new(
                        client_impl,
                        outgoing,
                        incoming,
                        |fut| {
                            tokio::task::spawn_local(fut);
                        },
                    );

                    // Spawn the I/O pump.
                    tokio::task::spawn_local(async move {
                        if let Err(e) = handle_io.await {
                            debug!("ACP I/O pump ended: {e}");
                        }
                    });

                    // 1. Initialize.
                    if let Err(e) = conn
                        .initialize(
                            agent_client_protocol::InitializeRequest::new(
                                agent_client_protocol::ProtocolVersion::V1,
                            )
                            .client_info(agent_client_protocol::Implementation::new(
                                "molt-hub",
                                "0.1.0",
                            )),
                        )
                        .await
                    {
                        let msg = format!("ACP initialize failed: {e}");
                        error!(agent_id = %agent_id_clone, "{msg}");
                        let _ = event_tx_thread.send(AgentEvent::Error {
                            agent_id: agent_id_clone.clone(),
                            message: msg,
                            timestamp: Utc::now(),
                        });
                        terminated_thread.store(true, Ordering::Relaxed);
                        return;
                    }

                    // 2. New session.
                    let session_resp = match conn
                        .new_session(agent_client_protocol::NewSessionRequest::new(
                            working_dir.clone(),
                        ))
                        .await
                    {
                        Ok(r) => r,
                        Err(e) => {
                            let msg = format!("ACP new_session failed: {e}");
                            error!(agent_id = %agent_id_clone, "{msg}");
                            let _ = event_tx_thread.send(AgentEvent::Error {
                                agent_id: agent_id_clone.clone(),
                                message: msg,
                                timestamp: Utc::now(),
                            });
                            terminated_thread.store(true, Ordering::Relaxed);
                            return;
                        }
                    };

                    let session_id = session_resp.session_id;

                    // 3. Send initial prompt (if any).
                    if !instructions.is_empty() {
                        let prompt_result = conn
                            .prompt(agent_client_protocol::PromptRequest::new(
                                session_id.clone(),
                                vec![instructions.into()],
                            ))
                            .await;

                        match prompt_result {
                            Ok(resp) => {
                                info!(
                                    agent_id = %agent_id_clone,
                                    stop_reason = ?resp.stop_reason,
                                    "ACP initial prompt completed",
                                );
                                // Flush partial output buffer at turn end
                                let _ = event_tx_thread.send(AgentEvent::TurnEnd {
                                    agent_id: agent_id_clone.clone(),
                                    stop_reason: Some(format!("{:?}", resp.stop_reason)),
                                    input_tokens: resp.usage.as_ref().map(|u| u.input_tokens as u32),
                                    output_tokens: resp.usage.as_ref().map(|u| u.output_tokens as u32),
                                    cost: None,
                                    timestamp: Utc::now(),
                                });
                                if resp.stop_reason == agent_client_protocol::StopReason::EndTurn {
                                    // Emit Completed only if the message loop also ends.
                                    // We continue to the follow-up loop below so the caller
                                    // can send more messages.
                                }
                            }
                            Err(e) => {
                                warn!(
                                    agent_id = %agent_id_clone,
                                    error = %e,
                                    "ACP initial prompt returned error",
                                );
                                let e_str = e.to_string();
                                let is_auth = e_str.contains("authenticate")
                                    || e_str.contains("token has expired")
                                    || e_str.contains("401");
                                let message = if is_auth {
                                    "auth_required: Claude OAuth token expired. Run `claude login` in your terminal to re-authenticate.".to_string()
                                } else {
                                    format!("ACP error: {e}")
                                };
                                let _ = event_tx_thread.send(AgentEvent::Error {
                                    agent_id: agent_id_clone.clone(),
                                    message: message.clone(),
                                    timestamp: Utc::now(),
                                });
                                let _ = event_tx_thread.send(AgentEvent::Output {
                                    agent_id: agent_id_clone.clone(),
                                    content: message,
                                    timestamp: Utc::now(),
                                });
                            }
                        }
                    }

                    // 4. Follow-up message loop.
                    while let Some(msg) = msg_rx.recv().await {
                        let result = conn
                            .prompt(agent_client_protocol::PromptRequest::new(
                                session_id.clone(),
                                vec![msg.into()],
                            ))
                            .await;

                        match result {
                            Ok(resp) => {
                                debug!(
                                    agent_id = %agent_id_clone,
                                    stop_reason = ?resp.stop_reason,
                                    "ACP prompt completed",
                                );
                                let _ = event_tx_thread.send(AgentEvent::TurnEnd {
                                    agent_id: agent_id_clone.clone(),
                                    stop_reason: Some(format!("{:?}", resp.stop_reason)),
                                    input_tokens: resp.usage.as_ref().map(|u| u.input_tokens as u32),
                                    output_tokens: resp.usage.as_ref().map(|u| u.output_tokens as u32),
                                    cost: None,
                                    timestamp: Utc::now(),
                                });
                            }
                            Err(e) => {
                                warn!(
                                    agent_id = %agent_id_clone,
                                    error = %e,
                                    "ACP prompt returned error",
                                );
                                let e_str = e.to_string();
                                let is_auth = e_str.contains("authenticate")
                                    || e_str.contains("token has expired")
                                    || e_str.contains("401");
                                let message = if is_auth {
                                    "auth_required: Claude OAuth token expired. Run `claude login` in your terminal to re-authenticate.".to_string()
                                } else {
                                    format!("ACP error: {e}")
                                };
                                let _ = event_tx_thread.send(AgentEvent::Error {
                                    agent_id: agent_id_clone.clone(),
                                    message: message.clone(),
                                    timestamp: Utc::now(),
                                });
                                let _ = event_tx_thread.send(AgentEvent::Output {
                                    agent_id: agent_id_clone.clone(),
                                    content: message,
                                    timestamp: Utc::now(),
                                });
                            }
                        }
                    }

                    // msg_rx closed (msg_tx dropped) → agent is done.
                    debug!(agent_id = %agent_id_clone, "ACP message loop ended, emitting Completed");

                    {
                        let mut w = status_thread.write().unwrap();
                        *w = AgentStatus::Completed;
                    }

                    // Run git diff to capture what the agent changed, then emit per-file events.
                    if let Ok(diff_output) = crate::worktree::run_git(&working_dir, &["diff", "HEAD"]).await {
                        for (path, diff) in parse_unified_diff_by_file(&diff_output) {
                            let _ = event_tx_thread.send(AgentEvent::FileDiff {
                                agent_id: agent_id_clone.clone(),
                                path,
                                unified_diff: diff,
                                timestamp: Utc::now(),
                            });
                        }
                    }

                    let _ = event_tx_thread.send(AgentEvent::Completed {
                        agent_id: agent_id_clone.clone(),
                        exit_code: Some(0),
                        timestamp: Utc::now(),
                    });

                    terminated_thread.store(true, Ordering::Relaxed);
                });
            })
            .map_err(|e| AdapterError::SpawnFailed(format!("failed to spawn ACP thread: {e}")))?;

        let internal = Box::new(AcpInternal {
            msg_tx,
            terminated,
            status,
            event_tx: event_tx.clone(),
            approve_tx,
        });

        // We don't have a reliable PID at this point (the child is in the thread);
        // pass None. Termination is handled via channel drop.
        Ok(AgentHandle::new(agent_id, None, internal))
    }

    async fn send(&self, handle: &AgentHandle, message: AgentMessage) -> Result<(), AdapterError> {
        let state = handle
            .downcast_internal::<AcpInternal>()
            .ok_or(AdapterError::AgentNotFound)?;

        match message {
            AgentMessage::Instruction(text) => {
                state
                    .msg_tx
                    .send(text)
                    .await
                    .map_err(|e| AdapterError::SendFailed(format!("ACP channel closed: {e}")))?;
            }
            AgentMessage::Pause => {
                // ACP has no built-in pause; update logical status only.
                let mut w = state.status.write().unwrap();
                *w = AgentStatus::Paused;
            }
            AgentMessage::Resume => {
                let mut w = state.status.write().unwrap();
                *w = AgentStatus::Running;
            }
            AgentMessage::Data(v) => {
                // Serialise to JSON and send as text.
                let text = serde_json::to_string(&v).map_err(|e| {
                    AdapterError::SendFailed(format!("JSON serialisation error: {e}"))
                })?;
                state
                    .msg_tx
                    .send(text)
                    .await
                    .map_err(|e| AdapterError::SendFailed(format!("ACP channel closed: {e}")))?;
            }
        }

        Ok(())
    }

    async fn status(&self, handle: &AgentHandle) -> Result<AgentStatus, AdapterError> {
        let state = handle
            .downcast_internal::<AcpInternal>()
            .ok_or(AdapterError::AgentNotFound)?;

        Ok(state.status.read().unwrap().clone())
    }

    async fn terminate(&self, handle: &AgentHandle) -> Result<(), AdapterError> {
        let state = handle
            .downcast_internal::<AcpInternal>()
            .ok_or(AdapterError::AgentNotFound)?;

        // Closing the sender causes msg_rx.recv() to return None, ending the loop.
        // We signal this by marking terminated and closing the channel.
        // (We can't actually "close" Sender without dropping it, but marking terminated
        //  is enough for status() and abort() to behave correctly.)
        state.terminated.store(true, Ordering::Relaxed);
        {
            let mut w = state.status.write().unwrap();
            *w = AgentStatus::Terminated;
        }
        let _ = state.event_tx.send(AgentEvent::StatusChanged {
            agent_id: handle.agent_id.clone(),
            previous: AgentStatus::Running,
            current: AgentStatus::Terminated,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    async fn abort(&self, handle: &AgentHandle) -> Result<(), AdapterError> {
        // Same as terminate for ACP (no SIGKILL available without the child handle here).
        self.terminate(handle).await
    }
}

// ---------------------------------------------------------------------------
// Unified diff parser
// ---------------------------------------------------------------------------

/// Split unified diff output into (path, diff_text) pairs, one per file.
fn parse_unified_diff_by_file(diff: &str) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_diff = String::new();

    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            if let Some(path) = current_path.take() {
                files.push((path, std::mem::take(&mut current_diff)));
            }
            // Extract path from "diff --git a/foo b/foo"
            if let Some(b_path) = line.split(" b/").nth(1) {
                current_path = Some(b_path.to_string());
            }
        }
        current_diff.push_str(line);
        current_diff.push('\n');
    }
    if let Some(path) = current_path {
        files.push((path, current_diff));
    }
    files
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_opencode() {
        let (cmd, args) = AcpAdapter::resolve_command("opencode", &serde_json::json!({})).unwrap();
        assert_eq!(cmd, "opencode");
        assert_eq!(args, vec!["acp"]);
    }

    #[test]
    fn resolve_goose() {
        let (cmd, args) = AcpAdapter::resolve_command("goose", &serde_json::json!({})).unwrap();
        assert_eq!(cmd, "goose");
        assert_eq!(args, vec!["acp"]);
    }

    #[test]
    fn resolve_gemini() {
        let (cmd, args) = AcpAdapter::resolve_command("gemini", &serde_json::json!({})).unwrap();
        assert_eq!(cmd, "gemini");
        assert_eq!(args, vec!["--acp"]);
    }

    #[test]
    fn resolve_claude_agent_acp() {
        let (cmd, args) =
            AcpAdapter::resolve_command("claude-agent-acp", &serde_json::json!({})).unwrap();
        assert_eq!(cmd, "claude-agent-acp");
        assert!(args.is_empty());
    }

    #[test]
    fn resolve_claude_acp_alias() {
        let (cmd, args) =
            AcpAdapter::resolve_command("claude-acp", &serde_json::json!({})).unwrap();
        assert_eq!(cmd, "claude-agent-acp");
        assert!(args.is_empty());
    }

    #[test]
    fn resolve_generic_acp_no_command_fails() {
        let result = AcpAdapter::resolve_command("acp", &serde_json::json!({}));
        assert!(matches!(result, Err(AdapterError::SpawnFailed(_))));
    }

    #[test]
    fn resolve_generic_acp_with_command() {
        let cfg = serde_json::json!({ "command": "my-agent", "args": ["--serve"] });
        let (cmd, args) = AcpAdapter::resolve_command("acp", &cfg).unwrap();
        assert_eq!(cmd, "my-agent");
        assert_eq!(args, vec!["--serve"]);
    }

    #[test]
    fn resolve_command_override_beats_adapter_type() {
        // Even for a known type, an explicit "command" wins.
        let cfg = serde_json::json!({ "command": "custom-opencode" });
        let (cmd, args) = AcpAdapter::resolve_command("opencode", &cfg).unwrap();
        assert_eq!(cmd, "custom-opencode");
        assert!(args.is_empty());
    }

    #[test]
    fn resolve_claude() {
        let (cmd, args) = AcpAdapter::resolve_command("claude", &serde_json::json!({})).unwrap();
        assert_eq!(cmd, "claude-agent-acp");
        assert!(args.is_empty());
    }

    #[test]
    fn resolve_claude_code() {
        let (cmd, args) =
            AcpAdapter::resolve_command("claude-code", &serde_json::json!({})).unwrap();
        assert_eq!(cmd, "claude-agent-acp");
        assert!(args.is_empty());
    }

    #[test]
    fn adapter_type_returns_claude() {
        assert_eq!(AcpAdapter::new().adapter_type(), "claude");
    }
}
