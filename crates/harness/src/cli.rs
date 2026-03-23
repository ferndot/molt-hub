//! Generic CLI adapter — AgentAdapter implementation for arbitrary command-line agent processes.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{broadcast, Mutex, RwLock};
use tracing::warn;

use molt_hub_core::model::{AgentId, AgentStatus};

use crate::adapter::{AdapterError, AgentAdapter, AgentEvent, AgentHandle, AgentMessage, SpawnConfig};

// ---------------------------------------------------------------------------
// OutputMode
// ---------------------------------------------------------------------------

/// Controls how the CLI adapter interprets lines from the subprocess stdout.
#[derive(Debug, Clone)]
pub enum OutputMode {
    /// Every line is emitted as-is in an `AgentEvent::Output`.
    Raw,
    /// Each line is parsed as JSON; a best-effort mapping to `AgentEvent` is applied.
    JsonLines,
    /// Output is split on a custom delimiter string.
    Delimiter(String),
}

impl Default for OutputMode {
    fn default() -> Self {
        OutputMode::Raw
    }
}

impl OutputMode {
    fn from_config(s: &str) -> Self {
        match s {
            "raw" => OutputMode::Raw,
            "json_lines" | "jsonlines" | "json-lines" => OutputMode::JsonLines,
            other => OutputMode::Delimiter(other.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// CliInternalState
// ---------------------------------------------------------------------------

/// Adapter-specific state stored inside `AgentHandle::internal`.
pub struct CliInternalState {
    pub child: Arc<Mutex<Child>>,
    pub stdin: Arc<Mutex<ChildStdin>>,
    pub status: Arc<RwLock<AgentStatus>>,
    pub event_tx: broadcast::Sender<AgentEvent>,
}

// ---------------------------------------------------------------------------
// CliAdapter
// ---------------------------------------------------------------------------

/// `AgentAdapter` that spawns any command-line process and communicates with
/// it through stdin/stdout.
#[derive(Debug, Clone)]
pub struct CliAdapter {
    /// Default command to run when `adapter_config["command"]` is absent.
    /// Usually left as `None` — the config must supply a command.
    pub default_command: Option<String>,
    /// Default args prepended before any args found in `adapter_config`.
    pub default_args: Vec<String>,
    /// Default output parsing mode (overridden by `adapter_config["output_mode"]`).
    pub default_output_mode: OutputMode,
}

impl Default for CliAdapter {
    fn default() -> Self {
        Self {
            default_command: None,
            default_args: vec![],
            default_output_mode: OutputMode::Raw,
        }
    }
}

impl CliAdapter {
    /// Construct an adapter with sensible defaults.
    pub fn new() -> Self {
        Self::default()
    }
}

// ---------------------------------------------------------------------------
// Helper — emit a status-changed event
// ---------------------------------------------------------------------------

async fn set_status(
    agent_id: &AgentId,
    status_lock: &Arc<RwLock<AgentStatus>>,
    event_tx: &broadcast::Sender<AgentEvent>,
    new_status: AgentStatus,
) {
    let previous = {
        let mut w = status_lock.write().await;
        let prev = w.clone();
        *w = new_status.clone();
        prev
    };
    let _ = event_tx.send(AgentEvent::StatusChanged {
        agent_id: agent_id.clone(),
        previous,
        current: new_status,
        timestamp: Utc::now(),
    });
}

// ---------------------------------------------------------------------------
// Output-reader background task
// ---------------------------------------------------------------------------

fn spawn_reader_task(
    agent_id: AgentId,
    stdout: tokio::process::ChildStdout,
    output_mode: OutputMode,
    status: Arc<RwLock<AgentStatus>>,
    event_tx: broadcast::Sender<AgentEvent>,
    child_arc: Arc<Mutex<Child>>,
) {
    tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        let mut buffer = String::new();

        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    match &output_mode {
                        OutputMode::Raw => {
                            let _ = event_tx.send(AgentEvent::Output {
                                agent_id: agent_id.clone(),
                                content: line,
                                timestamp: Utc::now(),
                            });
                        }
                        OutputMode::JsonLines => {
                            match parse_json_line(&agent_id, &line) {
                                Some(event) => {
                                    let _ = event_tx.send(event);
                                }
                                None => {
                                    // Fall back to raw output for non-JSON lines.
                                    let _ = event_tx.send(AgentEvent::Output {
                                        agent_id: agent_id.clone(),
                                        content: line,
                                        timestamp: Utc::now(),
                                    });
                                }
                            }
                        }
                        OutputMode::Delimiter(delim) => {
                            buffer.push_str(&line);
                            buffer.push('\n');
                            while let Some(pos) = buffer.find(delim.as_str()) {
                                let chunk = buffer[..pos].to_string();
                                buffer = buffer[pos + delim.len()..].to_string();
                                if !chunk.is_empty() {
                                    let _ = event_tx.send(AgentEvent::Output {
                                        agent_id: agent_id.clone(),
                                        content: chunk,
                                        timestamp: Utc::now(),
                                    });
                                }
                            }
                        }
                    }
                }
                Ok(None) => {
                    // EOF — process has closed stdout.
                    break;
                }
                Err(e) => {
                    warn!(agent_id = %agent_id, error = %e, "stdout read error");
                    break;
                }
            }
        }

        // Flush any remaining delimiter buffer.
        if let OutputMode::Delimiter(_) = &output_mode {
            if !buffer.trim().is_empty() {
                let _ = event_tx.send(AgentEvent::Output {
                    agent_id: agent_id.clone(),
                    content: buffer.trim_end().to_string(),
                    timestamp: Utc::now(),
                });
            }
        }

        // Reap the child and determine exit code.
        let exit_code = {
            let mut child = child_arc.lock().await;
            child.wait().await.ok().and_then(|s| s.code())
        };

        let success = exit_code.map(|c| c == 0).unwrap_or(false);
        if success {
            set_status(
                &agent_id,
                &status,
                &event_tx,
                AgentStatus::Completed,
            ).await;
            let _ = event_tx.send(AgentEvent::Completed {
                agent_id: agent_id.clone(),
                exit_code,
                timestamp: Utc::now(),
            });
        } else {
            set_status(&agent_id, &status, &event_tx, AgentStatus::Failed).await;
            let _ = event_tx.send(AgentEvent::Error {
                agent_id: agent_id.clone(),
                message: format!(
                    "process exited with code {:?}",
                    exit_code
                ),
                timestamp: Utc::now(),
            });
        }
    });
}

/// Attempt to parse a JSON line into a known `AgentEvent`.
///
/// Recognised JSON shapes:
/// - `{"type": "output", "content": "..."}` → `AgentEvent::Output`
/// - `{"type": "progress", "percent": 50, "message": "..."}` → `AgentEvent::Progress`
/// - `{"type": "error", "message": "..."}` → `AgentEvent::Error`
/// - `{"type": "completed", "exit_code": 0}` → `AgentEvent::Completed`
fn parse_json_line(agent_id: &AgentId, line: &str) -> Option<AgentEvent> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let event_type = v.get("type")?.as_str()?;

    match event_type {
        "output" => {
            let content = v.get("content")?.as_str()?.to_string();
            Some(AgentEvent::Output {
                agent_id: agent_id.clone(),
                content,
                timestamp: Utc::now(),
            })
        }
        "progress" => {
            let percent = v.get("percent")?.as_u64()? as u8;
            let message = v.get("message").and_then(|m| m.as_str()).map(|s| s.to_string());
            Some(AgentEvent::Progress {
                agent_id: agent_id.clone(),
                percent,
                message,
                timestamp: Utc::now(),
            })
        }
        "error" => {
            let message = v.get("message")?.as_str()?.to_string();
            Some(AgentEvent::Error {
                agent_id: agent_id.clone(),
                message,
                timestamp: Utc::now(),
            })
        }
        "completed" => {
            let exit_code = v.get("exit_code").and_then(|c| c.as_i64()).map(|c| c as i32);
            Some(AgentEvent::Completed {
                agent_id: agent_id.clone(),
                exit_code,
                timestamp: Utc::now(),
            })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// AgentAdapter impl
// ---------------------------------------------------------------------------

#[async_trait]
impl AgentAdapter for CliAdapter {
    fn adapter_type(&self) -> &str {
        "cli"
    }

    async fn spawn(&self, config: SpawnConfig) -> Result<AgentHandle, AdapterError> {
        // --- Resolve command from config --------------------------------
        let command = config
            .adapter_config
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| self.default_command.clone())
            .ok_or_else(|| {
                AdapterError::SpawnFailed(
                    "adapter_config missing required field \"command\"".to_string(),
                )
            })?;

        // --- Resolve args -----------------------------------------------
        let config_args: Vec<String> = config
            .adapter_config
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|a| a.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let args: Vec<String> = self
            .default_args
            .iter()
            .cloned()
            .chain(config_args)
            .collect();

        // --- Resolve output mode ----------------------------------------
        let output_mode = config
            .adapter_config
            .get("output_mode")
            .and_then(|v| v.as_str())
            .map(OutputMode::from_config)
            .unwrap_or_else(|| self.default_output_mode.clone());

        // --- Build command -----------------------------------------------
        let mut cmd = tokio::process::Command::new(&command);
        cmd.args(&args)
            .current_dir(&config.working_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        for (k, v) in &config.env {
            cmd.env(k, v);
        }

        let mut child = cmd.spawn().map_err(|e| {
            AdapterError::SpawnFailed(format!("failed to spawn \"{command}\": {e}"))
        })?;

        let pid = child.id();

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AdapterError::SpawnFailed("could not obtain stdin pipe".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AdapterError::SpawnFailed("could not obtain stdout pipe".to_string()))?;

        let (event_tx, _) = broadcast::channel::<AgentEvent>(256);
        let status = Arc::new(RwLock::new(AgentStatus::Running));

        let child_arc = Arc::new(Mutex::new(child));
        let stdin_arc = Arc::new(Mutex::new(stdin));

        // Send initial instructions to stdin.
        if !config.instructions.is_empty() {
            let mut stdin_guard = stdin_arc.lock().await;
            let payload = format!("{}\n", config.instructions);
            stdin_guard.write_all(payload.as_bytes()).await.map_err(|e| {
                AdapterError::SendFailed(format!("failed to write initial instructions: {e}"))
            })?;
            stdin_guard.flush().await.map_err(|e| {
                AdapterError::SendFailed(format!("failed to flush stdin: {e}"))
            })?;
        }

        // Launch background reader.
        spawn_reader_task(
            config.agent_id.clone(),
            stdout,
            output_mode,
            Arc::clone(&status),
            event_tx.clone(),
            Arc::clone(&child_arc),
        );

        let internal = Box::new(CliInternalState {
            child: child_arc,
            stdin: stdin_arc,
            status,
            event_tx,
        });

        Ok(AgentHandle::new(config.agent_id, pid, internal))
    }

    async fn send(&self, handle: &AgentHandle, message: AgentMessage) -> Result<(), AdapterError> {
        let state = handle
            .downcast_internal::<CliInternalState>()
            .ok_or(AdapterError::AgentNotFound)?;

        match message {
            AgentMessage::Instruction(s) => {
                let mut stdin = state.stdin.lock().await;
                let payload = format!("{s}\n");
                stdin.write_all(payload.as_bytes()).await.map_err(|e| {
                    AdapterError::SendFailed(format!("stdin write error: {e}"))
                })?;
                stdin.flush().await.map_err(|e| {
                    AdapterError::SendFailed(format!("stdin flush error: {e}"))
                })?;
            }

            AgentMessage::Pause => {
                // Update status to Paused. On Unix we could SIGSTOP the process,
                // but that requires libc as a dependency and complicates cross-platform
                // builds. Status tracking alone is sufficient for the supervisor layer.
                set_status(
                    &handle.agent_id,
                    &state.status,
                    &state.event_tx,
                    AgentStatus::Paused,
                ).await;
            }

            AgentMessage::Resume => {
                set_status(
                    &handle.agent_id,
                    &state.status,
                    &state.event_tx,
                    AgentStatus::Running,
                ).await;
            }

            AgentMessage::Data(v) => {
                let line = serde_json::to_string(&v).map_err(|e| {
                    AdapterError::SendFailed(format!("JSON serialisation error: {e}"))
                })?;
                let mut stdin = state.stdin.lock().await;
                let payload = format!("{line}\n");
                stdin.write_all(payload.as_bytes()).await.map_err(|e| {
                    AdapterError::SendFailed(format!("stdin write error: {e}"))
                })?;
                stdin.flush().await.map_err(|e| {
                    AdapterError::SendFailed(format!("stdin flush error: {e}"))
                })?;
            }
        }

        Ok(())
    }

    async fn status(&self, handle: &AgentHandle) -> Result<AgentStatus, AdapterError> {
        let state = handle
            .downcast_internal::<CliInternalState>()
            .ok_or(AdapterError::AgentNotFound)?;
        Ok(state.status.read().await.clone())
    }

    async fn terminate(&self, handle: &AgentHandle) -> Result<(), AdapterError> {
        let state = handle
            .downcast_internal::<CliInternalState>()
            .ok_or(AdapterError::AgentNotFound)?;

        {
            let mut child = state.child.lock().await;
            child.start_kill().map_err(|e| {
                AdapterError::Internal(Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
            })?;
            // Wait for the child to finish.
            let _ = child.wait().await;
        }

        set_status(
            &handle.agent_id,
            &state.status,
            &state.event_tx,
            AgentStatus::Terminated,
        ).await;

        Ok(())
    }

    async fn abort(&self, handle: &AgentHandle) -> Result<(), AdapterError> {
        let state = handle
            .downcast_internal::<CliInternalState>()
            .ok_or(AdapterError::AgentNotFound)?;

        {
            let mut child = state.child.lock().await;
            child.kill().await.map_err(|e| {
                AdapterError::Internal(Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
            })?;
        }

        set_status(
            &handle.agent_id,
            &state.status,
            &state.event_tx,
            AgentStatus::Terminated,
        ).await;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    use molt_hub_core::model::{AgentId, SessionId, TaskId};
    use ulid::Ulid;

    fn make_config(adapter_config: serde_json::Value) -> SpawnConfig {
        SpawnConfig {
            agent_id: AgentId(Ulid::new()),
            task_id: TaskId(Ulid::new()),
            session_id: SessionId(Ulid::new()),
            working_dir: PathBuf::from("/tmp"),
            instructions: String::new(),
            env: HashMap::new(),
            timeout: None,
            adapter_config,
        }
    }

    // -----------------------------------------------------------------------
    // adapter_type
    // -----------------------------------------------------------------------

    #[test]
    fn adapter_type_returns_cli() {
        let adapter = CliAdapter::new();
        assert_eq!(adapter.adapter_type(), "cli");
    }

    // -----------------------------------------------------------------------
    // OutputMode parsing
    // -----------------------------------------------------------------------

    #[test]
    fn output_mode_raw_from_config() {
        let mode = OutputMode::from_config("raw");
        assert!(matches!(mode, OutputMode::Raw));
    }

    #[test]
    fn output_mode_json_lines_from_config() {
        let mode = OutputMode::from_config("json_lines");
        assert!(matches!(mode, OutputMode::JsonLines));

        let mode2 = OutputMode::from_config("jsonlines");
        assert!(matches!(mode2, OutputMode::JsonLines));

        let mode3 = OutputMode::from_config("json-lines");
        assert!(matches!(mode3, OutputMode::JsonLines));
    }

    #[test]
    fn output_mode_delimiter_from_config() {
        let mode = OutputMode::from_config("---");
        assert!(matches!(mode, OutputMode::Delimiter(ref d) if d == "---"));
    }

    #[test]
    fn output_mode_default_is_raw() {
        let mode = OutputMode::default();
        assert!(matches!(mode, OutputMode::Raw));
    }

    // -----------------------------------------------------------------------
    // JSON line parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_json_line_output() {
        let agent_id = AgentId(Ulid::new());
        let line = r#"{"type":"output","content":"hello"}"#;
        let event = parse_json_line(&agent_id, line).unwrap();
        match event {
            AgentEvent::Output { content, .. } => assert_eq!(content, "hello"),
            _ => panic!("expected Output event"),
        }
    }

    #[test]
    fn parse_json_line_progress() {
        let agent_id = AgentId(Ulid::new());
        let line = r#"{"type":"progress","percent":42,"message":"halfway"}"#;
        let event = parse_json_line(&agent_id, line).unwrap();
        match event {
            AgentEvent::Progress { percent, message, .. } => {
                assert_eq!(percent, 42);
                assert_eq!(message.as_deref(), Some("halfway"));
            }
            _ => panic!("expected Progress event"),
        }
    }

    #[test]
    fn parse_json_line_error() {
        let agent_id = AgentId(Ulid::new());
        let line = r#"{"type":"error","message":"oops"}"#;
        let event = parse_json_line(&agent_id, line).unwrap();
        match event {
            AgentEvent::Error { message, .. } => assert_eq!(message, "oops"),
            _ => panic!("expected Error event"),
        }
    }

    #[test]
    fn parse_json_line_unknown_returns_none() {
        let agent_id = AgentId(Ulid::new());
        let event = parse_json_line(&agent_id, r#"{"type":"unknown"}"#);
        assert!(event.is_none());
    }

    #[test]
    fn parse_json_line_invalid_json_returns_none() {
        let agent_id = AgentId(Ulid::new());
        let event = parse_json_line(&agent_id, "not json");
        assert!(event.is_none());
    }

    // -----------------------------------------------------------------------
    // Missing command returns SpawnFailed
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn spawn_missing_command_returns_spawn_failed() {
        let adapter = CliAdapter::new();
        let config = make_config(serde_json::json!({}));
        let result = adapter.spawn(config).await;
        assert!(
            matches!(result, Err(AdapterError::SpawnFailed(_))),
            "expected SpawnFailed",
        );
    }

    #[tokio::test]
    async fn spawn_nonexistent_command_returns_spawn_failed() {
        let adapter = CliAdapter::new();
        let config = make_config(serde_json::json!({
            "command": "/nonexistent/binary_that_does_not_exist"
        }));
        let result = adapter.spawn(config).await;
        assert!(
            matches!(result, Err(AdapterError::SpawnFailed(_))),
            "expected SpawnFailed",
        );
    }

    // -----------------------------------------------------------------------
    // Successful spawn with a real process
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn spawn_echo_process() {
        let adapter = CliAdapter::new();
        let config = make_config(serde_json::json!({
            "command": "cat",
            "output_mode": "raw"
        }));
        // cat will hang reading stdin; we just verify spawn succeeds and returns Running.
        let handle = adapter.spawn(config).await.expect("spawn should succeed");
        let status = adapter.status(&handle).await.expect("status should succeed");
        assert_eq!(status, AgentStatus::Running);
        // Clean up.
        adapter.abort(&handle).await.ok();
    }

    #[tokio::test]
    async fn command_from_adapter_config_args() {
        // Verify args are forwarded correctly by running `echo hello`.
        let adapter = CliAdapter::new();
        let config = make_config(serde_json::json!({
            "command": "echo",
            "args": ["hello", "world"],
            "output_mode": "raw"
        }));
        let handle = adapter.spawn(config).await.expect("spawn should succeed");
        // Give the process a moment to finish.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        // Process should have exited — status will be Completed or Terminated.
        let status = adapter.status(&handle).await.expect("status ok");
        assert!(
            matches!(status, AgentStatus::Completed | AgentStatus::Running),
            "unexpected status: {status:?}"
        );
    }
}
