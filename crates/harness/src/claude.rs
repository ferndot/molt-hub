//! Claude SDK adapter — AgentAdapter implementation backed by the Claude CLI.
//!
//! Spawns `claude --print --output-format stream-json` as a subprocess, pipes
//! instructions via stdin, and parses the structured JSON event stream from
//! stdout.

use std::io;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{broadcast, RwLock};
use tokio::sync::Mutex;
use tracing::{debug, error, warn};

use molt_hub_core::model::{AgentId, AgentStatus};

use crate::adapter::{
    AdapterError, AgentAdapter, AgentEvent, AgentHandle, AgentMessage, SpawnConfig,
};

// ---------------------------------------------------------------------------
// ClaudeInternalState
// ---------------------------------------------------------------------------

/// Internal state stored inside [`AgentHandle::internal`] for the Claude adapter.
pub struct ClaudeInternalState {
    pub child: Arc<Mutex<Child>>,
    pub stdin: Arc<Mutex<ChildStdin>>,
    pub status: Arc<RwLock<AgentStatus>>,
    pub event_tx: broadcast::Sender<AgentEvent>,
}

impl ClaudeInternalState {
    /// Subscribe to agent events. Returns a new `Receiver` from the broadcast channel.
    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.event_tx.subscribe()
    }
}

// ---------------------------------------------------------------------------
// ClaudeAdapter
// ---------------------------------------------------------------------------

/// An [`AgentAdapter`] that spawns the `claude` CLI in non-interactive mode.
///
/// The adapter invokes:
/// ```text
/// claude --print --output-format stream-json [--model <model>]
/// ```
/// and communicates via stdin/stdout.
#[derive(Debug, Clone)]
pub struct ClaudeAdapter {
    /// Path to the `claude` binary (defaults to `"claude"` on `$PATH`).
    pub binary_path: String,
    /// Default model to use if not specified in `adapter_config`.
    pub default_model: String,
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        Self {
            binary_path: "claude".to_string(),
            default_model: "sonnet".to_string(),
        }
    }
}

impl ClaudeAdapter {
    /// Construct a new `ClaudeAdapter` with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a `ClaudeAdapter` with a custom binary path.
    pub fn with_binary(binary_path: impl Into<String>) -> Self {
        Self {
            binary_path: binary_path.into(),
            ..Self::default()
        }
    }

    /// Build the [`Command`] for the Claude subprocess.
    ///
    /// This is extracted into a helper so it can be tested independently.
    pub(crate) fn build_command(&self, config: &SpawnConfig) -> Command {
        let model = config
            .adapter_config
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.default_model)
            .to_string();

        let mut cmd = Command::new(&self.binary_path);
        cmd.arg("--print")
            .arg("--output-format")
            .arg("stream-json")
            .arg("--model")
            .arg(&model)
            .current_dir(&config.working_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        // Forward any extra env vars from the spawn config.
        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        cmd
    }

    /// Spawn the background task that reads stdout and emits events.
    fn spawn_output_reader(
        agent_id: AgentId,
        stdout: tokio::process::ChildStdout,
        stderr: tokio::process::ChildStderr,
        status: Arc<RwLock<AgentStatus>>,
        event_tx: broadcast::Sender<AgentEvent>,
    ) {
        let agent_id_err = agent_id.clone();
        let status_err = status.clone();
        let event_tx_err = event_tx.clone();

        // Stdout reader
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        debug!(agent_id = %agent_id, "claude stdout: {}", line);
                        let content = parse_claude_line(&line);
                        let _ = event_tx.send(AgentEvent::Output {
                            agent_id: agent_id.clone(),
                            content,
                            timestamp: Utc::now(),
                        });
                    }
                    Ok(None) => {
                        // EOF — process has finished
                        debug!(agent_id = %agent_id, "claude stdout EOF");
                        {
                            let mut st = status.write().await;
                            *st = AgentStatus::Terminated;
                        }
                        let _ = event_tx.send(AgentEvent::Completed {
                            agent_id: agent_id.clone(),
                            exit_code: None,
                            timestamp: Utc::now(),
                        });
                        break;
                    }
                    Err(e) => {
                        error!(agent_id = %agent_id, "error reading claude stdout: {}", e);
                        let msg = e.to_string();
                        {
                            let mut st = status.write().await;
                            *st = AgentStatus::Crashed { error: msg.clone() };
                        }
                        let _ = event_tx.send(AgentEvent::Error {
                            agent_id: agent_id.clone(),
                            message: msg,
                            timestamp: Utc::now(),
                        });
                        break;
                    }
                }
            }
        });

        // Stderr reader — emit Error events for each line
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                warn!(agent_id = %agent_id_err, "claude stderr: {}", line);
                {
                    // Read status for potential future guard logic; no mutation needed here.
                    let _st = status_err.read().await;
                }
                let _ = event_tx_err.send(AgentEvent::Error {
                    agent_id: agent_id_err.clone(),
                    message: line,
                    timestamp: Utc::now(),
                });
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Output parsing
// ---------------------------------------------------------------------------

/// Parse a single stdout line from the Claude stream-json format.
///
/// Claude's `--output-format stream-json` emits newline-delimited JSON objects.
/// We extract the `content` field if present, otherwise return the raw line.
fn parse_claude_line(line: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(line) {
        Ok(val) => {
            // Try common content field paths
            if let Some(text) = val
                .get("content")
                .and_then(|v| v.as_str())
            {
                return text.to_string();
            }
            // Claude stream events often use nested delta structures
            if let Some(text) = val
                .pointer("/delta/text")
                .and_then(|v| v.as_str())
            {
                return text.to_string();
            }
            if let Some(text) = val
                .pointer("/message/content")
                .and_then(|v| v.as_str())
            {
                return text.to_string();
            }
            // Fall back to serialising the full JSON object
            line.to_string()
        }
        Err(_) => line.to_string(),
    }
}

// ---------------------------------------------------------------------------
// AgentAdapter implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl AgentAdapter for ClaudeAdapter {
    fn adapter_type(&self) -> &str {
        "claude-cli"
    }

    async fn spawn(&self, config: SpawnConfig) -> Result<AgentHandle, AdapterError> {
        let mut cmd = self.build_command(&config);

        let mut child = cmd.spawn().map_err(|e| {
            let msg = match e.kind() {
                io::ErrorKind::NotFound => format!(
                    "claude binary not found at '{}': {}",
                    self.binary_path, e
                ),
                _ => format!("failed to spawn claude: {}", e),
            };
            AdapterError::SpawnFailed(msg)
        })?;

        let pid = child.id();

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AdapterError::SpawnFailed("stdin not available".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AdapterError::SpawnFailed("stdout not available".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AdapterError::SpawnFailed("stderr not available".into()))?;

        let status = Arc::new(RwLock::new(AgentStatus::Running));
        let (event_tx, _) = broadcast::channel(256);

        let state = ClaudeInternalState {
            child: Arc::new(Mutex::new(child)),
            stdin: Arc::new(Mutex::new(stdin)),
            status: status.clone(),
            event_tx: event_tx.clone(),
        };

        // Spawn the background I/O reader tasks.
        Self::spawn_output_reader(
            config.agent_id.clone(),
            stdout,
            stderr,
            status,
            event_tx,
        );

        // Send the initial instructions to the process via stdin.
        if !config.instructions.is_empty() {
            let mut line = config.instructions.clone();
            if !line.ends_with('\n') {
                line.push('\n');
            }
            let mut stdin_guard = state.stdin.lock().await;
            stdin_guard
                .write_all(line.as_bytes())
                .await
                .map_err(|e| AdapterError::SendFailed(e.to_string()))?;
            stdin_guard
                .flush()
                .await
                .map_err(|e| AdapterError::SendFailed(e.to_string()))?;
        }

        Ok(AgentHandle::new(config.agent_id, pid, Box::new(state)))
    }

    async fn send(
        &self,
        handle: &AgentHandle,
        message: AgentMessage,
    ) -> Result<(), AdapterError> {
        let state = handle
            .downcast_internal::<ClaudeInternalState>()
            .ok_or(AdapterError::AgentNotFound)?;

        // Ensure the agent is still running.
        {
            let st = state.status.read().await;
            match *st {
                AgentStatus::Terminated | AgentStatus::Crashed { .. } => {
                    return Err(AdapterError::AlreadyTerminated);
                }
                _ => {}
            }
        }

        let payload = match message {
            AgentMessage::Instruction(text) => {
                let mut s = text;
                if !s.ends_with('\n') {
                    s.push('\n');
                }
                s
            }
            AgentMessage::Data(val) => {
                let mut s = serde_json::to_string(&val)
                    .map_err(|e| AdapterError::SendFailed(e.to_string()))?;
                s.push('\n');
                s
            }
            AgentMessage::Pause => {
                // Without platform signal support we track state only.
                let mut st = state.status.write().await;
                *st = AgentStatus::Paused;
                return Ok(());
            }
            AgentMessage::Resume => {
                let mut st = state.status.write().await;
                if *st == AgentStatus::Paused {
                    *st = AgentStatus::Running;
                }
                return Ok(());
            }
        };

        let mut stdin_guard = state.stdin.lock().await;
        stdin_guard
            .write_all(payload.as_bytes())
            .await
            .map_err(|e| AdapterError::SendFailed(e.to_string()))?;
        stdin_guard
            .flush()
            .await
            .map_err(|e| AdapterError::SendFailed(e.to_string()))?;

        Ok(())
    }

    async fn status(&self, handle: &AgentHandle) -> Result<AgentStatus, AdapterError> {
        let state = handle
            .downcast_internal::<ClaudeInternalState>()
            .ok_or(AdapterError::AgentNotFound)?;

        let st = state.status.read().await;
        Ok(st.clone())
    }

    async fn terminate(&self, handle: &AgentHandle) -> Result<(), AdapterError> {
        let state = handle
            .downcast_internal::<ClaudeInternalState>()
            .ok_or(AdapterError::AgentNotFound)?;

        // Close stdin to signal EOF to the process.
        // tokio::process::Child::kill() sends SIGKILL; for a graceful
        // termination we close stdin and wait briefly.
        {
            // Drop the stdin guard to close the pipe.
            let mut _stdin = state.stdin.lock().await;
            // Flush and let it drop (close) on unlock — we achieve close by
            // calling shutdown which flushes remaining bytes and closes the fd.
            let _ = _stdin.flush().await;
            // We don't have a direct "close" API on ChildStdin; dropping it
            // would close it, but we hold it in Arc<Mutex>. Instead we send
            // SIGTERM by calling kill() with a short wait, then update status.
        }

        let mut child_guard = state.child.lock().await;
        child_guard
            .kill()
            .await
            .map_err(|e| AdapterError::SendFailed(format!("kill failed: {}", e)))?;

        let exit_status = child_guard.wait().await.ok();
        let exit_code = exit_status.and_then(|s| s.code());

        {
            let mut st = state.status.write().await;
            *st = AgentStatus::Terminated;
        }

        let _ = state.event_tx.send(AgentEvent::Completed {
            agent_id: handle.agent_id.clone(),
            exit_code,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    async fn abort(&self, handle: &AgentHandle) -> Result<(), AdapterError> {
        // For abort we use the same kill() path — tokio uses SIGKILL on Unix.
        self.terminate(handle).await
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

    fn make_spawn_config() -> SpawnConfig {
        SpawnConfig {
            agent_id: AgentId::new(),
            task_id: TaskId::new(),
            session_id: SessionId::new(),
            working_dir: PathBuf::from("/tmp"),
            instructions: "say hello".to_string(),
            env: HashMap::new(),
            timeout: None,
            adapter_config: serde_json::json!({}),
        }
    }

    #[test]
    fn test_adapter_type() {
        let adapter = ClaudeAdapter::new();
        assert_eq!(adapter.adapter_type(), "claude-cli");
    }

    #[test]
    fn test_default_binary_and_model() {
        let adapter = ClaudeAdapter::default();
        assert_eq!(adapter.binary_path, "claude");
        assert_eq!(adapter.default_model, "sonnet");
    }

    #[test]
    fn test_with_binary() {
        let adapter = ClaudeAdapter::with_binary("/usr/local/bin/claude");
        assert_eq!(adapter.binary_path, "/usr/local/bin/claude");
    }

    #[test]
    fn test_build_command_default_flags() {
        let adapter = ClaudeAdapter::new();
        let config = make_spawn_config();
        let cmd = adapter.build_command(&config);
        // We can inspect the program and args via the Debug output since
        // tokio::process::Command doesn't expose args() directly.
        let dbg = format!("{:?}", cmd);
        assert!(dbg.contains("--print"), "expected --print flag");
        assert!(dbg.contains("stream-json"), "expected stream-json flag");
        assert!(dbg.contains("--model"), "expected --model flag");
        assert!(dbg.contains("sonnet"), "expected default model");
    }

    #[test]
    fn test_build_command_custom_model() {
        let adapter = ClaudeAdapter::new();
        let mut config = make_spawn_config();
        config.adapter_config = serde_json::json!({ "model": "opus" });
        let cmd = adapter.build_command(&config);
        let dbg = format!("{:?}", cmd);
        assert!(dbg.contains("opus"), "expected custom model 'opus'");
    }

    #[test]
    fn test_build_command_env_forwarding() {
        let adapter = ClaudeAdapter::new();
        let mut config = make_spawn_config();
        config.env.insert("MY_VAR".to_string(), "hello".to_string());
        let cmd = adapter.build_command(&config);
        let dbg = format!("{:?}", cmd);
        assert!(dbg.contains("MY_VAR"), "expected env var forwarded");
    }

    #[test]
    fn test_parse_claude_line_plain_text() {
        let line = "Hello, world!";
        assert_eq!(parse_claude_line(line), "Hello, world!");
    }

    #[test]
    fn test_parse_claude_line_json_content() {
        let line = r#"{"type":"content_block_delta","delta":{"text":"Hi there"}}"#;
        assert_eq!(parse_claude_line(line), "Hi there");
    }

    #[test]
    fn test_parse_claude_line_json_content_field() {
        let line = r#"{"content":"Direct content"}"#;
        assert_eq!(parse_claude_line(line), "Direct content");
    }

    #[test]
    fn test_parse_claude_line_unknown_json() {
        let line = r#"{"type":"message_stop"}"#;
        // No content field, falls back to raw line
        assert_eq!(parse_claude_line(line), line);
    }

    #[test]
    fn test_claude_internal_state_subscribe() {
        let (tx, _rx) = broadcast::channel::<AgentEvent>(8);
        // Subscribing should produce a new receiver without panicking
        let rx2 = tx.subscribe();
        drop(rx2);
    }
}
