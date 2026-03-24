//! Claude SDK adapter — AgentAdapter implementation backed by the Claude CLI.
//!
//! Spawns `claude --print --output-format stream-json --verbose` as a subprocess, passes
//! the initial query as a positional argument (print-mode prompt), and parses
//! the structured JSON event stream from stdout. Follow-up input uses stdin.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{broadcast, Mutex, RwLock};
use tracing::{debug, error, warn};

use molt_hub_core::model::{AgentId, AgentStatus};

use crate::adapter::{
    collect_agent_print_output, AdapterError, AgentAdapter, AgentEvent, AgentHandle, AgentMessage,
    SpawnConfig,
};

// ---------------------------------------------------------------------------
// Parsed Claude event types
// ---------------------------------------------------------------------------

/// Typed representation of events emitted by the Claude CLI in stream-json mode.
///
/// The Claude CLI's `--output-format stream-json` emits newline-delimited JSON
/// with a `type` field identifying each event kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeStreamEvent {
    /// System-level message (session start, configuration echo).
    #[serde(rename = "system")]
    System { message: Option<String> },

    /// The assistant is producing text content.
    #[serde(rename = "assistant")]
    Assistant {
        #[serde(default)]
        message: Option<String>,
        /// Partial text content in streaming mode.
        #[serde(default)]
        content: Option<String>,
    },

    /// A content block delta (incremental text).
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        #[serde(default)]
        delta: Option<DeltaPayload>,
    },

    /// The assistant is requesting a tool use.
    #[serde(rename = "tool_use")]
    ToolUse {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        input: Option<serde_json::Value>,
    },

    /// Result from a tool invocation.
    #[serde(rename = "tool_result")]
    ToolResult {
        #[serde(default)]
        content: Option<String>,
        #[serde(default)]
        is_error: Option<bool>,
    },

    /// Final result message when the CLI completes.
    #[serde(rename = "result")]
    Result {
        #[serde(default)]
        result: Option<String>,
        #[serde(default)]
        cost_usd: Option<f64>,
        #[serde(default)]
        duration_ms: Option<u64>,
        #[serde(default)]
        is_error: Option<bool>,
    },

    /// Message stop sentinel.
    #[serde(rename = "message_stop")]
    MessageStop {},
}

/// Delta payload for incremental content updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaPayload {
    #[serde(default)]
    pub text: Option<String>,
}

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
/// claude --print --output-format stream-json --verbose [--model <model>] [<initial query>]
/// ```
/// `--verbose` is required by Claude Code when combining `--print` with `stream-json`.
/// Non-empty `SpawnConfig::instructions` are passed as the trailing positional
/// query (see Claude Code CLI print mode). Follow-up input uses stdin.
#[derive(Debug, Clone)]
pub struct ClaudeAdapter {
    /// Path to the `claude` binary (defaults to `"claude"` on `$PATH`).
    pub binary_path: String,
    /// Default model to use if not specified in `adapter_config`.
    pub default_model: String,
    /// Default timeout applied if `SpawnConfig::timeout` is `None`.
    pub default_timeout: Option<Duration>,
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        Self {
            binary_path: "claude".to_string(),
            default_model: "sonnet".to_string(),
            default_timeout: None,
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

    /// Set the default timeout for agent processes.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = Some(timeout);
        self
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
            // Claude Code: print mode + stream-json requires verbose (full event stream).
            .arg("--verbose")
            .arg("--model")
            .arg(&model);

        // Print mode: the user message is a positional argument, not `--prompt`
        // (that flag does not exist on Claude Code CLI; `-p` is `--print`).
        if !config.instructions.is_empty() {
            cmd.arg(&config.instructions);
        }

        cmd.current_dir(&config.working_dir)
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

    /// Resolve the effective timeout from config or adapter defaults.
    fn effective_timeout(&self, config: &SpawnConfig) -> Option<Duration> {
        config.timeout.or(self.default_timeout)
    }

    /// Single non-interactive `--print` run: spawn Claude, collect streamed text, wait for exit 0.
    ///
    /// Stdin is closed immediately so the CLI does not wait for interactive input. Subscribe to the
    /// event channel before starting the stdout reader to avoid missing early events.
    pub async fn run_print_collect(&self, config: SpawnConfig) -> Result<String, AdapterError> {
        let timeout = self.effective_timeout(&config);
        let mut cmd = self.build_command(&config);

        let mut child = cmd.spawn().map_err(|e| {
            let msg = match e.kind() {
                io::ErrorKind::NotFound => {
                    format!("claude binary not found at '{}': {}", self.binary_path, e)
                }
                _ => format!("failed to spawn claude: {}", e),
            };
            AdapterError::SpawnFailed(msg)
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AdapterError::SpawnFailed("stdin not available".into()))?;
        drop(stdin);

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AdapterError::SpawnFailed("stdout not available".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AdapterError::SpawnFailed("stderr not available".into()))?;

        let agent_id = config.agent_id.clone();
        let status = Arc::new(RwLock::new(AgentStatus::Running));
        let (event_tx, _) = tokio::sync::broadcast::channel(256);
        let mut rx = event_tx.subscribe();
        let child_arc = Arc::new(Mutex::new(child));

        Self::spawn_output_reader(
            agent_id.clone(),
            stdout,
            stderr,
            status.clone(),
            event_tx.clone(),
            Arc::clone(&child_arc),
        );

        if let Some(t) = timeout {
            Self::spawn_timeout_watchdog(
                agent_id.clone(),
                t,
                Arc::clone(&child_arc),
                status,
                event_tx,
            );
        }

        collect_agent_print_output(&mut rx, &agent_id).await
    }

    /// Spawn the background task that reads stdout and emits events.
    fn spawn_output_reader(
        agent_id: AgentId,
        stdout: tokio::process::ChildStdout,
        stderr: tokio::process::ChildStderr,
        status: Arc<RwLock<AgentStatus>>,
        event_tx: broadcast::Sender<AgentEvent>,
        child: Arc<Mutex<Child>>,
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
                        // EOF — process has finished. Reap exit code.
                        debug!(agent_id = %agent_id, "claude stdout EOF");
                        let exit_code = {
                            let mut c = child.lock().await;
                            c.wait().await.ok().and_then(|s| s.code())
                        };
                        let success = exit_code.map(|c| c == 0).unwrap_or(false);
                        {
                            let mut st = status.write().await;
                            *st = if success {
                                AgentStatus::Completed
                            } else {
                                AgentStatus::Failed
                            };
                        }
                        let _ = event_tx.send(AgentEvent::Completed {
                            agent_id: agent_id.clone(),
                            exit_code,
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

    /// Spawn a timeout watchdog that aborts the process after the deadline.
    fn spawn_timeout_watchdog(
        agent_id: AgentId,
        timeout: Duration,
        child: Arc<Mutex<Child>>,
        status: Arc<RwLock<AgentStatus>>,
        event_tx: broadcast::Sender<AgentEvent>,
    ) {
        tokio::spawn(async move {
            tokio::time::sleep(timeout).await;

            // Check if the process is still running before killing.
            let still_running = {
                let st = status.read().await;
                matches!(*st, AgentStatus::Running | AgentStatus::Paused)
            };

            if still_running {
                warn!(
                    agent_id = %agent_id,
                    timeout_secs = timeout.as_secs(),
                    "agent timeout reached, killing process"
                );
                let mut c = child.lock().await;
                let _ = c.kill().await;
                {
                    let mut st = status.write().await;
                    *st = AgentStatus::Failed;
                }
                let _ = event_tx.send(AgentEvent::Error {
                    agent_id: agent_id.clone(),
                    message: format!("agent timed out after {} seconds", timeout.as_secs()),
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
/// We extract meaningful text content when present, falling back to the raw line.
pub fn parse_claude_line(line: &str) -> String {
    // Fast path: if the line doesn't start with '{', it's not JSON.
    let trimmed = line.trim();
    if !trimmed.starts_with('{') {
        return line.to_string();
    }

    // Try typed deserialization first for known event types.
    if let Ok(event) = serde_json::from_str::<ClaudeStreamEvent>(trimmed) {
        match event {
            ClaudeStreamEvent::Assistant {
                content, message, ..
            } => {
                if let Some(text) = content.or(message) {
                    if !text.is_empty() {
                        return text;
                    }
                }
            }
            ClaudeStreamEvent::ContentBlockDelta { delta } => {
                if let Some(d) = delta {
                    if let Some(text) = d.text {
                        return text;
                    }
                }
            }
            ClaudeStreamEvent::Result { result, .. } => {
                if let Some(text) = result {
                    if !text.is_empty() {
                        return text;
                    }
                }
            }
            ClaudeStreamEvent::ToolUse { name, .. } => {
                if let Some(n) = name {
                    return format!("[tool_use: {n}]");
                }
            }
            ClaudeStreamEvent::ToolResult { content, is_error } => {
                if let Some(text) = content {
                    let prefix = if is_error == Some(true) {
                        "[tool_error] "
                    } else {
                        "[tool_result] "
                    };
                    return format!("{prefix}{text}");
                }
            }
            // System, MessageStop — no meaningful text
            _ => {}
        }
    }

    // Fall back to generic JSON field extraction for unknown schemas.
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        // Try common content field paths
        if let Some(text) = val.get("content").and_then(|v| v.as_str()) {
            return text.to_string();
        }
        if let Some(text) = val.pointer("/delta/text").and_then(|v| v.as_str()) {
            return text.to_string();
        }
        if let Some(text) = val.pointer("/message/content").and_then(|v| v.as_str()) {
            return text.to_string();
        }
    }

    // Return the raw line as-is.
    line.to_string()
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
        let timeout = self.effective_timeout(&config);
        let mut cmd = self.build_command(&config);

        let mut child = cmd.spawn().map_err(|e| {
            let msg = match e.kind() {
                io::ErrorKind::NotFound => {
                    format!("claude binary not found at '{}': {}", self.binary_path, e)
                }
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

        let child_arc = Arc::new(Mutex::new(child));

        let state = ClaudeInternalState {
            child: Arc::clone(&child_arc),
            stdin: Arc::new(Mutex::new(stdin)),
            status: status.clone(),
            event_tx: event_tx.clone(),
        };

        // Spawn the background I/O reader tasks.
        Self::spawn_output_reader(
            config.agent_id.clone(),
            stdout,
            stderr,
            status.clone(),
            event_tx.clone(),
            Arc::clone(&child_arc),
        );

        // Spawn timeout watchdog if configured.
        if let Some(t) = timeout {
            Self::spawn_timeout_watchdog(
                config.agent_id.clone(),
                t,
                Arc::clone(&child_arc),
                status,
                event_tx,
            );
        }

        Ok(AgentHandle::new(config.agent_id, pid, Box::new(state)))
    }

    async fn send(&self, handle: &AgentHandle, message: AgentMessage) -> Result<(), AdapterError> {
        let state = handle
            .downcast_internal::<ClaudeInternalState>()
            .ok_or(AdapterError::AgentNotFound)?;

        // Ensure the agent is still running.
        {
            let st = state.status.read().await;
            match *st {
                AgentStatus::Terminated
                | AgentStatus::Completed
                | AgentStatus::Failed
                | AgentStatus::Crashed { .. } => {
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

        // Flush stdin before killing to signal EOF intent.
        {
            let mut stdin_guard = state.stdin.lock().await;
            let _ = stdin_guard.flush().await;
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
    use molt_hub_core::model::{AgentId, SessionId, TaskId};
    use std::collections::HashMap;
    use std::path::PathBuf;

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
            project_id: None,
        }
    }

    // -------------------------------------------------------------------
    // Adapter construction
    // -------------------------------------------------------------------

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
        assert!(adapter.default_timeout.is_none());
    }

    #[test]
    fn test_with_binary() {
        let adapter = ClaudeAdapter::with_binary("/usr/local/bin/claude");
        assert_eq!(adapter.binary_path, "/usr/local/bin/claude");
    }

    #[test]
    fn test_with_timeout() {
        let adapter = ClaudeAdapter::new().with_timeout(Duration::from_secs(300));
        assert_eq!(adapter.default_timeout, Some(Duration::from_secs(300)));
    }

    // -------------------------------------------------------------------
    // Command building
    // -------------------------------------------------------------------

    #[test]
    fn test_build_command_default_flags() {
        let adapter = ClaudeAdapter::new();
        let config = make_spawn_config();
        let cmd = adapter.build_command(&config);
        let dbg = format!("{:?}", cmd);
        assert!(dbg.contains("--print"), "expected --print flag");
        assert!(dbg.contains("stream-json"), "expected stream-json flag");
        assert!(dbg.contains("--verbose"), "expected --verbose (required with print + stream-json)");
        assert!(dbg.contains("--model"), "expected --model flag");
        assert!(dbg.contains("sonnet"), "expected default model");
    }

    #[test]
    fn test_build_command_includes_positional_query() {
        let adapter = ClaudeAdapter::new();
        let config = make_spawn_config();
        let cmd = adapter.build_command(&config);
        let dbg = format!("{:?}", cmd);
        assert!(
            !dbg.contains("--prompt"),
            "Claude Code CLI has no --prompt; query must be positional"
        );
        assert!(
            dbg.contains("say hello"),
            "expected instructions as positional arg"
        );
    }

    #[test]
    fn test_build_command_no_positional_query_when_empty() {
        let adapter = ClaudeAdapter::new();
        let mut config = make_spawn_config();
        config.instructions = String::new();
        let cmd = adapter.build_command(&config);
        let dbg = format!("{:?}", cmd);
        assert!(
            !dbg.contains("say hello"),
            "should not include instruction text when instructions are empty"
        );
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
    fn test_effective_timeout_from_config() {
        let adapter = ClaudeAdapter::new().with_timeout(Duration::from_secs(600));
        let mut config = make_spawn_config();
        config.timeout = Some(Duration::from_secs(120));
        // Config timeout takes precedence over adapter default.
        assert_eq!(
            adapter.effective_timeout(&config),
            Some(Duration::from_secs(120))
        );
    }

    #[test]
    fn test_effective_timeout_falls_back_to_adapter() {
        let adapter = ClaudeAdapter::new().with_timeout(Duration::from_secs(600));
        let config = make_spawn_config();
        assert_eq!(
            adapter.effective_timeout(&config),
            Some(Duration::from_secs(600))
        );
    }

    #[test]
    fn test_effective_timeout_none_when_both_absent() {
        let adapter = ClaudeAdapter::new();
        let config = make_spawn_config();
        assert_eq!(adapter.effective_timeout(&config), None);
    }

    // -------------------------------------------------------------------
    // Output parsing — plain text
    // -------------------------------------------------------------------

    #[test]
    fn test_parse_claude_line_plain_text() {
        let line = "Hello, world!";
        assert_eq!(parse_claude_line(line), "Hello, world!");
    }

    #[test]
    fn test_parse_claude_line_empty() {
        assert_eq!(parse_claude_line(""), "");
    }

    #[test]
    fn test_parse_claude_line_whitespace_only() {
        assert_eq!(parse_claude_line("   "), "   ");
    }

    // -------------------------------------------------------------------
    // Output parsing — typed Claude events
    // -------------------------------------------------------------------

    #[test]
    fn test_parse_assistant_content() {
        let line = r#"{"type":"assistant","content":"Hello from Claude"}"#;
        assert_eq!(parse_claude_line(line), "Hello from Claude");
    }

    #[test]
    fn test_parse_assistant_message() {
        let line = r#"{"type":"assistant","message":"Hello from Claude"}"#;
        assert_eq!(parse_claude_line(line), "Hello from Claude");
    }

    #[test]
    fn test_parse_content_block_delta() {
        let line = r#"{"type":"content_block_delta","delta":{"text":"Hi there"}}"#;
        assert_eq!(parse_claude_line(line), "Hi there");
    }

    #[test]
    fn test_parse_result_event() {
        let line = r#"{"type":"result","result":"Task completed successfully","cost_usd":0.05,"duration_ms":12000}"#;
        assert_eq!(parse_claude_line(line), "Task completed successfully");
    }

    #[test]
    fn test_parse_result_event_error() {
        let line = r#"{"type":"result","result":"Failed","is_error":true}"#;
        assert_eq!(parse_claude_line(line), "Failed");
    }

    #[test]
    fn test_parse_tool_use_event() {
        let line = r#"{"type":"tool_use","name":"Read","input":{"file_path":"/tmp/test.txt"}}"#;
        assert_eq!(parse_claude_line(line), "[tool_use: Read]");
    }

    #[test]
    fn test_parse_tool_result_success() {
        let line = r#"{"type":"tool_result","content":"file contents here","is_error":false}"#;
        assert_eq!(parse_claude_line(line), "[tool_result] file contents here");
    }

    #[test]
    fn test_parse_tool_result_error() {
        let line = r#"{"type":"tool_result","content":"not found","is_error":true}"#;
        assert_eq!(parse_claude_line(line), "[tool_error] not found");
    }

    #[test]
    fn test_parse_system_event() {
        // System events don't have meaningful text content — should fall through
        // to the raw line.
        let line = r#"{"type":"system","message":"Session started"}"#;
        assert_eq!(parse_claude_line(line), line);
    }

    #[test]
    fn test_parse_message_stop() {
        let line = r#"{"type":"message_stop"}"#;
        assert_eq!(parse_claude_line(line), line);
    }

    // -------------------------------------------------------------------
    // Output parsing — generic JSON fallback
    // -------------------------------------------------------------------

    #[test]
    fn test_parse_claude_line_json_content_field() {
        let line = r#"{"content":"Direct content"}"#;
        assert_eq!(parse_claude_line(line), "Direct content");
    }

    #[test]
    fn test_parse_claude_line_unknown_json() {
        let line = r#"{"type":"unknown_event","data":"stuff"}"#;
        // No recognized content field, falls back to raw line
        assert_eq!(parse_claude_line(line), line);
    }

    #[test]
    fn test_parse_claude_line_delta_text_fallback() {
        // A non-typed delta that matches the generic pointer path
        let line = r#"{"delta":{"text":"incremental"}}"#;
        assert_eq!(parse_claude_line(line), "incremental");
    }

    #[test]
    fn test_parse_claude_line_message_content_fallback() {
        let line = r#"{"message":{"content":"nested content"}}"#;
        assert_eq!(parse_claude_line(line), "nested content");
    }

    // -------------------------------------------------------------------
    // ClaudeStreamEvent serde roundtrip
    // -------------------------------------------------------------------

    #[test]
    fn test_stream_event_deserialize_assistant() {
        let json = r#"{"type":"assistant","content":"hello","message":null}"#;
        let event: ClaudeStreamEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, ClaudeStreamEvent::Assistant { .. }));
    }

    #[test]
    fn test_stream_event_deserialize_result() {
        let json = r#"{"type":"result","result":"done","cost_usd":0.01,"duration_ms":500,"is_error":false}"#;
        let event: ClaudeStreamEvent = serde_json::from_str(json).unwrap();
        match event {
            ClaudeStreamEvent::Result {
                result,
                cost_usd,
                duration_ms,
                is_error,
            } => {
                assert_eq!(result.as_deref(), Some("done"));
                assert_eq!(cost_usd, Some(0.01));
                assert_eq!(duration_ms, Some(500));
                assert_eq!(is_error, Some(false));
            }
            _ => panic!("expected Result event"),
        }
    }

    #[test]
    fn test_stream_event_deserialize_tool_use() {
        let json = r#"{"type":"tool_use","name":"Bash","input":{"command":"ls"}}"#;
        let event: ClaudeStreamEvent = serde_json::from_str(json).unwrap();
        match event {
            ClaudeStreamEvent::ToolUse { name, input } => {
                assert_eq!(name.as_deref(), Some("Bash"));
                assert!(input.is_some());
            }
            _ => panic!("expected ToolUse event"),
        }
    }

    // -------------------------------------------------------------------
    // Broadcast subscription
    // -------------------------------------------------------------------

    #[test]
    fn test_claude_internal_state_subscribe() {
        let (tx, _rx) = broadcast::channel::<AgentEvent>(8);
        let rx2 = tx.subscribe();
        drop(rx2);
    }

    // -------------------------------------------------------------------
    // Integration tests with mock process (cat)
    // -------------------------------------------------------------------

    /// Helper: create a ClaudeAdapter that spawns `cat` instead of `claude`.
    /// `cat` echoes stdin to stdout, making it a perfect mock for integration
    /// tests without requiring the actual Claude binary.
    fn mock_adapter() -> ClaudeAdapter {
        ClaudeAdapter {
            binary_path: "cat".to_string(),
            default_model: "sonnet".to_string(),
            default_timeout: None,
        }
    }

    fn mock_spawn_config() -> SpawnConfig {
        SpawnConfig {
            agent_id: AgentId::new(),
            task_id: TaskId::new(),
            session_id: SessionId::new(),
            working_dir: PathBuf::from("/tmp"),
            // Empty instructions so we control what goes to stdin manually.
            instructions: String::new(),
            env: HashMap::new(),
            timeout: None,
            adapter_config: serde_json::json!({}),
            project_id: None,
        }
    }

    #[tokio::test]
    async fn integration_spawn_and_status() {
        let adapter = mock_adapter();
        let config = mock_spawn_config();
        let handle = adapter.spawn(config).await.expect("spawn should succeed");
        let status = adapter.status(&handle).await.expect("status should work");
        assert_eq!(status, AgentStatus::Running);
        adapter.abort(&handle).await.ok();
    }

    #[tokio::test]
    async fn integration_spawn_returns_pid() {
        let adapter = mock_adapter();
        let config = mock_spawn_config();
        let handle = adapter.spawn(config).await.expect("spawn should succeed");
        assert!(handle.pid().is_some(), "spawned process should have a PID");
        adapter.abort(&handle).await.ok();
    }

    #[tokio::test]
    async fn integration_send_data_message() {
        // Use `sleep 10` as a long-running process that stays alive for send().
        let adapter = ClaudeAdapter {
            binary_path: "sleep".to_string(),
            default_model: "sonnet".to_string(),
            default_timeout: None,
        };
        let mut config = mock_spawn_config();
        config.instructions = String::new();
        let handle = adapter.spawn(config).await.expect("spawn should succeed");

        // Sending a Data message should succeed while the process is alive.
        let result = adapter
            .send(
                &handle,
                AgentMessage::Data(serde_json::json!({"task": "test"})),
            )
            .await;

        // `sleep` ignores stdin, so write may succeed or fail depending on pipe
        // buffering, but it should not panic. We just verify it doesn't return
        // AgentNotFound or AlreadyTerminated.
        match &result {
            Ok(()) => {}                           // expected
            Err(AdapterError::SendFailed(_)) => {} // acceptable — pipe may be closed
            other => panic!("unexpected result: {:?}", other),
        }

        adapter.abort(&handle).await.ok();
    }

    #[tokio::test]
    async fn integration_terminate_sets_terminated() {
        let adapter = mock_adapter();
        let config = mock_spawn_config();
        let handle = adapter.spawn(config).await.expect("spawn should succeed");

        adapter
            .terminate(&handle)
            .await
            .expect("terminate should succeed");

        let status = adapter.status(&handle).await.expect("status should work");
        assert_eq!(status, AgentStatus::Terminated);
    }

    #[tokio::test]
    async fn integration_pause_resume() {
        let adapter = mock_adapter();
        let config = mock_spawn_config();
        let handle = adapter.spawn(config).await.expect("spawn should succeed");

        adapter
            .send(&handle, AgentMessage::Pause)
            .await
            .expect("pause should succeed");
        let status = adapter.status(&handle).await.unwrap();
        assert_eq!(status, AgentStatus::Paused);

        adapter
            .send(&handle, AgentMessage::Resume)
            .await
            .expect("resume should succeed");
        let status = adapter.status(&handle).await.unwrap();
        assert_eq!(status, AgentStatus::Running);

        adapter.abort(&handle).await.ok();
    }

    #[tokio::test]
    async fn integration_send_to_terminated_fails() {
        let adapter = mock_adapter();
        let config = mock_spawn_config();
        let handle = adapter.spawn(config).await.expect("spawn should succeed");

        adapter.terminate(&handle).await.unwrap();

        let result = adapter
            .send(&handle, AgentMessage::Instruction("nope".into()))
            .await;
        assert!(
            matches!(result, Err(AdapterError::AlreadyTerminated)),
            "expected AlreadyTerminated, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn integration_spawn_nonexistent_binary_fails() {
        let adapter = ClaudeAdapter::with_binary("/nonexistent/path/claude_fake_binary");
        let config = mock_spawn_config();
        let result = adapter.spawn(config).await;
        assert!(
            matches!(result, Err(AdapterError::SpawnFailed(_))),
            "expected SpawnFailed"
        );
    }

    #[tokio::test]
    async fn integration_process_eof_emits_completed() {
        // Spawn `echo hello` which will print and exit immediately.
        let adapter = ClaudeAdapter {
            binary_path: "echo".to_string(),
            default_model: "sonnet".to_string(),
            default_timeout: None,
        };
        let config = mock_spawn_config();
        let handle = adapter.spawn(config).await.expect("spawn should succeed");

        // Give the background reader a moment to process EOF.
        tokio::time::sleep(Duration::from_millis(200)).await;

        let status = adapter.status(&handle).await.unwrap();
        assert!(
            matches!(status, AgentStatus::Completed | AgentStatus::Failed),
            "expected Completed or Failed after process exit, got {:?}",
            status
        );
    }

    #[tokio::test]
    async fn integration_timeout_kills_process() {
        let adapter = ClaudeAdapter {
            binary_path: "cat".to_string(),
            default_model: "sonnet".to_string(),
            default_timeout: Some(Duration::from_millis(200)),
        };
        let config = mock_spawn_config();
        let handle = adapter.spawn(config).await.expect("spawn should succeed");

        // Wait for the timeout to fire.
        tokio::time::sleep(Duration::from_millis(500)).await;

        let status = adapter.status(&handle).await.unwrap();
        assert!(
            matches!(
                status,
                AgentStatus::Failed | AgentStatus::Terminated | AgentStatus::Crashed { .. }
            ),
            "expected Failed/Terminated/Crashed after timeout, got {:?}",
            status
        );
    }
}
