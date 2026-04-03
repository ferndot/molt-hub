//! Hook executor — runs container and process hooks at pipeline stage boundaries.
//!
//! When a task enters or exits a stage, `HookExecutor::execute_hooks` runs all configured
//! hooks that match the trigger, applying the configured failure policy for each.
//!
//! ## AgentDispatch hook
//!
//! When an `agent_dispatch` hook fires, the executor spawns a sub-agent via the
//! configured [`AgentAdapter`]. The hook config controls:
//! - `adapter`: adapter type string passed to [`SpawnConfig::adapter_config`]
//! - `instruction`: the instruction string sent to the sub-agent
//! - `timeout_seconds`: optional timeout; defaults to 300 s
//! - `working_dir`: optional working directory for the sub-agent; defaults to `"."`

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use molt_hub_core::config::{HookDefinition, HookKind, HookTrigger, StageDefinition};
use molt_hub_core::model::{AgentId, SessionId, TaskId};

use molt_hub_harness::adapter::{AdapterError, AgentAdapter, SpawnConfig};
use molt_hub_harness::supervisor::Supervisor;

// ─── Context ─────────────────────────────────────────────────────────────────

/// Rich context passed to every hook execution.
#[derive(Debug, Clone)]
pub struct HookContext {
    pub task_id: TaskId,
    pub agent_id: Option<AgentId>,
    pub session_id: SessionId,
    pub stage_name: String,
    pub trigger: HookTrigger,
    pub pipeline_name: String,
    /// Additional environment variables injected into shell hooks.
    pub env: HashMap<String, String>,
    pub task_title: String,
    pub task_description: String,
    pub priority: String,
}

// ─── Results ─────────────────────────────────────────────────────────────────

/// Outcome of a single hook execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookResult {
    Success { output: Option<String> },
    Failed { error: String, retryable: bool },
    Skipped { reason: String },
}

// ─── Failure policy ──────────────────────────────────────────────────────────

/// What to do when a hook fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailurePolicy {
    /// Stop processing; fail the whole transition.
    Abort,
    /// Log the error and continue to the next hook.
    Continue,
    /// Retry up to `max_attempts` times with `delay_ms` between attempts.
    Retry { max_attempts: u32, delay_ms: u64 },
}

impl FailurePolicy {
    fn from_config(config: &serde_json::Value) -> Self {
        match config.get("failure_policy").and_then(|v| v.as_str()) {
            Some("continue") => FailurePolicy::Continue,
            Some("retry") => {
                let max_attempts = config
                    .get("retry_max_attempts")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(3) as u32;
                let delay_ms = config
                    .get("retry_delay_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(500);
                FailurePolicy::Retry {
                    max_attempts,
                    delay_ms,
                }
            }
            // "abort" or anything unrecognised → abort
            _ => FailurePolicy::Abort,
        }
    }
}

// ─── Execution mode ──────────────────────────────────────────────────────────

/// Whether hooks for a trigger run one-by-one or all at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionMode {
    Sequential,
    Parallel,
}

impl ExecutionMode {
    fn from_config(config: &serde_json::Value) -> Self {
        match config.get("execution_mode").and_then(|v| v.as_str()) {
            Some("parallel") => ExecutionMode::Parallel,
            _ => ExecutionMode::Sequential,
        }
    }
}

// ─── Errors ──────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum HookExecutorError {
    #[error("hook '{hook_kind}' execution failed: {error}")]
    ExecutionFailed { hook_kind: String, error: String },

    #[error("hook '{hook_kind}' aborted pipeline: {error}")]
    Aborted { hook_kind: String, error: String },

    #[error("hook '{hook_kind}' exhausted {attempts} retry attempts")]
    AllRetriesExhausted { hook_kind: String, attempts: u32 },

    #[error("hook timed out")]
    Timeout,
}

// ─── Executor ────────────────────────────────────────────────────────────────

/// Hook executor — receives config and context, runs hooks, returns results.
///
/// For `agent_dispatch` hooks, either a [`Supervisor`] (preferred, registers
/// agents in the UI) or a raw [`AgentAdapter`] (fallback, used in tests) must
/// be provided. Without either, those hooks return [`HookResult::Skipped`].
pub struct HookExecutor {
    adapter: Option<Arc<dyn AgentAdapter>>,
    supervisor: Option<Arc<Supervisor>>,
}

impl HookExecutor {
    /// Create a bare executor without an agent adapter.
    ///
    /// `agent_dispatch` hooks will be skipped when no adapter is configured.
    pub fn new() -> Self {
        HookExecutor {
            adapter: None,
            supervisor: None,
        }
    }

    /// Create an executor with an agent adapter for `agent_dispatch` hooks.
    ///
    /// Agents spawned this way bypass the supervisor registry and won't appear
    /// in the UI. Prefer [`HookExecutor::with_supervisor`] in production.
    pub fn with_adapter(adapter: Arc<dyn AgentAdapter>) -> Self {
        HookExecutor {
            adapter: Some(adapter),
            supervisor: None,
        }
    }

    /// Create an executor backed by a [`Supervisor`] for `agent_dispatch` hooks.
    ///
    /// Agents spawned via the supervisor are registered in the DashMap and
    /// appear in the agents view with full event streaming.
    pub fn with_supervisor(supervisor: Arc<Supervisor>) -> Self {
        HookExecutor {
            adapter: None,
            supervisor: Some(supervisor),
        }
    }

    /// Run all hooks on `stage` that match `trigger`.
    ///
    /// Hooks are filtered by trigger, then executed according to the execution mode
    /// derived from the first matching hook's config (sequential by default).
    /// Returns `Err(HookExecutorError::Aborted)` if any hook with `Abort` policy fails.
    pub async fn execute_hooks(
        &self,
        stage: &StageDefinition,
        trigger: HookTrigger,
        ctx: &HookContext,
    ) -> Result<Vec<HookResult>, HookExecutorError> {
        let matching: Vec<&HookDefinition> =
            stage.hooks.iter().filter(|h| h.on == trigger).collect();

        if matching.is_empty() {
            return Ok(vec![]);
        }

        // Determine execution mode from the first hook's config.
        let mode = ExecutionMode::from_config(&matching[0].config);

        match mode {
            ExecutionMode::Sequential => self.run_sequential(&matching, ctx).await,
            ExecutionMode::Parallel => self.run_parallel(&matching, ctx).await,
        }
    }

    // ── Sequential ──────────────────────────────────────────────────────────

    async fn run_sequential(
        &self,
        hooks: &[&HookDefinition],
        ctx: &HookContext,
    ) -> Result<Vec<HookResult>, HookExecutorError> {
        let mut results = Vec::with_capacity(hooks.len());

        for hook in hooks {
            let policy = FailurePolicy::from_config(&hook.config);
            let result = self.execute_with_policy(hook, ctx, &policy).await?;
            results.push(result);
        }

        Ok(results)
    }

    // ── Parallel ────────────────────────────────────────────────────────────

    async fn run_parallel(
        &self,
        hooks: &[&HookDefinition],
        ctx: &HookContext,
    ) -> Result<Vec<HookResult>, HookExecutorError> {
        let mut handles = Vec::with_capacity(hooks.len());

        for hook in hooks {
            // Clone what we need to move into the spawned task.
            let hook_clone = (*hook).clone();
            let ctx_clone = ctx.clone();
            let executor = HookExecutor {
                adapter: self.adapter.clone(),
                supervisor: self.supervisor.clone(),
            };
            let policy = FailurePolicy::from_config(&hook.config);

            handles.push(tokio::spawn(async move {
                executor
                    .execute_with_policy(&hook_clone, &ctx_clone, &policy)
                    .await
            }));
        }

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            // Propagate join errors as execution failures.
            let result = handle
                .await
                .map_err(|e| HookExecutorError::ExecutionFailed {
                    hook_kind: "unknown".into(),
                    error: e.to_string(),
                })??;
            results.push(result);
        }

        Ok(results)
    }

    // ── Execute with policy ─────────────────────────────────────────────────

    async fn execute_with_policy(
        &self,
        hook: &HookDefinition,
        ctx: &HookContext,
        policy: &FailurePolicy,
    ) -> Result<HookResult, HookExecutorError> {
        match policy {
            FailurePolicy::Abort => {
                let result = self.execute_single(hook, ctx).await;
                if let HookResult::Failed { ref error, .. } = result {
                    return Err(HookExecutorError::Aborted {
                        hook_kind: hook_kind_name(&hook.kind),
                        error: error.clone(),
                    });
                }
                Ok(result)
            }

            FailurePolicy::Continue => {
                let result = self.execute_single(hook, ctx).await;
                if let HookResult::Failed { ref error, .. } = result {
                    warn!(
                        hook_kind = hook_kind_name(&hook.kind),
                        error = %error,
                        "hook failed; policy=continue, proceeding"
                    );
                }
                Ok(result)
            }

            FailurePolicy::Retry {
                max_attempts,
                delay_ms,
            } => {
                let mut last_result = HookResult::Skipped {
                    reason: "no attempts made".into(),
                };
                for attempt in 1..=*max_attempts {
                    last_result = self.execute_single(hook, ctx).await;
                    match &last_result {
                        HookResult::Success { .. } => return Ok(last_result),
                        HookResult::Failed { retryable, .. } if !retryable => {
                            // Non-retryable failure — give up immediately.
                            return Ok(last_result);
                        }
                        HookResult::Failed { .. } => {
                            if attempt < *max_attempts {
                                debug!(attempt, max_attempts, "hook failed; retrying");
                                tokio::time::sleep(Duration::from_millis(*delay_ms)).await;
                            }
                        }
                        HookResult::Skipped { .. } => return Ok(last_result),
                    }
                }
                // If we reach here the last result is a failure after all retries.
                match &last_result {
                    HookResult::Failed { .. } => Err(HookExecutorError::AllRetriesExhausted {
                        hook_kind: hook_kind_name(&hook.kind),
                        attempts: *max_attempts,
                    }),
                    _ => Ok(last_result),
                }
            }
        }
    }

    // ── Single hook dispatch ─────────────────────────────────────────────────

    /// Execute a single hook and return its result (no policy applied here).
    pub async fn execute_single(&self, hook: &HookDefinition, ctx: &HookContext) -> HookResult {
        match hook.kind {
            HookKind::Shell => self.execute_shell(hook, ctx).await,
            HookKind::StartDevEnvironment => {
                debug!(stage = %ctx.stage_name, "StartDevEnvironment hook placeholder");
                HookResult::Success { output: None }
            }
            HookKind::TeardownDevEnvironment => {
                debug!(stage = %ctx.stage_name, "TeardownDevEnvironment hook placeholder");
                HookResult::Success { output: None }
            }
            HookKind::AgentDispatch => self.execute_agent_dispatch(hook, ctx).await,
            HookKind::Webhook => self.execute_webhook(hook, ctx).await,
        }
    }

    // ── AgentDispatch hook ────────────────────────────────────────────────────

    /// Spawn a sub-agent for the `agent_dispatch` hook kind.
    ///
    /// Config fields:
    /// - `instruction` (required): instruction string sent to the sub-agent
    /// - `timeout_seconds` (optional, default 300): max wait time
    /// - `working_dir` (optional, default `"."`): working directory for the agent
    /// - `adapter_config` (optional): opaque JSON forwarded to the adapter
    async fn execute_agent_dispatch(&self, hook: &HookDefinition, ctx: &HookContext) -> HookResult {
        // Require either a supervisor or a raw adapter.
        if self.supervisor.is_none() && self.adapter.is_none() {
            warn!(
                stage = %ctx.stage_name,
                "agent_dispatch hook skipped: no Supervisor or AgentAdapter configured on HookExecutor"
            );
            return HookResult::Skipped {
                reason: "no Supervisor or AgentAdapter configured".into(),
            };
        }

        let config = &hook.config;

        let instruction_template = match config.get("instruction").and_then(|v| v.as_str()) {
            Some(i) => i.to_string(),
            None => {
                return HookResult::Failed {
                    error: "agent_dispatch hook missing required 'instruction' field".into(),
                    retryable: false,
                }
            }
        };

        let engine = molt_hub_core::templates::TemplateEngine::new();
        let tmpl_ctx = molt_hub_core::templates::TemplateContext {
            task_id: ctx.task_id.to_string(),
            task_title: ctx.task_title.clone(),
            task_description: ctx.task_description.clone(),
            stage_name: ctx.stage_name.clone(),
            pipeline_name: ctx.pipeline_name.clone(),
            agent_name: None,
            agent_type: None,
            priority: ctx.priority.clone(),
            custom: std::collections::HashMap::new(),
        };
        let instruction = match engine.render_inline(&instruction_template, &tmpl_ctx) {
            Ok(rendered) => rendered,
            Err(e) => {
                return HookResult::Failed {
                    error: format!("instruction template render error: {e}"),
                    retryable: false,
                }
            }
        };

        let timeout_secs = config
            .get("timeout_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(300);

        let working_dir = config
            .get("working_dir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));

        let adapter_config = config
            .get("adapter_config")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let sub_agent_id = AgentId::new();
        let spawn_cfg = SpawnConfig {
            agent_id: sub_agent_id.clone(),
            task_id: ctx.task_id.clone(),
            session_id: ctx.session_id.clone(),
            working_dir,
            instructions: instruction.clone(),
            env: ctx.env.clone(),
            timeout: Some(Duration::from_secs(timeout_secs)),
            adapter_config,
            project_id: None,
            name: None,
            event_tx: None,
        };

        info!(
            task_id = %ctx.task_id,
            stage = %ctx.stage_name,
            sub_agent_id = %sub_agent_id,
            "agent_dispatch: spawning sub-agent"
        );

        // Prefer supervisor path so the agent is registered and visible in the UI.
        if let Some(ref supervisor) = self.supervisor {
            // The supervisor needs an adapter to do the actual spawning. We use
            // the adapter stored on self if available; otherwise we create a
            // default ACP adapter (same as the manual spawn endpoint does).
            let adapter: Arc<dyn AgentAdapter> = match &self.adapter {
                Some(a) => Arc::clone(a),
                None => Arc::new(molt_hub_harness::acp::AcpAdapter::new()),
            };
            return match supervisor.spawn_agent(adapter, spawn_cfg).await {
                Ok(_agent_id) => {
                    tracing::info!(
                        task_id = %ctx.task_id,
                        stage = %ctx.stage_name,
                        agent_id = %sub_agent_id,
                        "agent_dispatch: sub-agent registered with supervisor"
                    );
                    HookResult::Success {
                        output: Some(format!("agent spawned: {}", sub_agent_id)),
                    }
                }
                Err(e) => HookResult::Failed {
                    error: format!("agent_dispatch spawn failed: {e}"),
                    retryable: true,
                },
            };
        }

        // Fallback to raw adapter (used in tests / bare HookExecutor::with_adapter).
        let adapter = self.adapter.as_ref().expect("checked above that adapter is Some when supervisor is None");
        let handle = match adapter.spawn(spawn_cfg).await {
            Ok(h) => h,
            Err(AdapterError::SpawnFailed(msg)) => {
                return HookResult::Failed {
                    error: format!("agent_dispatch spawn failed: {msg}"),
                    retryable: true,
                }
            }
            Err(e) => {
                return HookResult::Failed {
                    error: format!("agent_dispatch adapter error: {e}"),
                    retryable: false,
                }
            }
        };

        // Fire-and-forget: agent runs in background; status tracked via WebSocket events.
        tracing::info!(
            task_id = %ctx.task_id,
            stage = %ctx.stage_name,
            agent_id = %handle.agent_id(),
            "agent_dispatch: sub-agent spawned via adapter (fire-and-forget, not registered in supervisor)"
        );
        HookResult::Success {
            output: Some(format!("agent spawned: {}", handle.agent_id())),
        }
    }

    // ── Webhook hook ────────────────────────────────────────────────────────

    /// Send an HTTP request for the `webhook` hook kind.
    ///
    /// Config fields:
    /// - `url` (required): the URL to call
    /// - `method` (optional, default `"POST"`): POST, GET, PUT, PATCH
    /// - `headers` (optional): object of key→value string pairs to add
    /// - `body` (optional): string body to send (for POST/PUT/PATCH); defaults to task context JSON
    /// - `timeout_seconds` (optional, default 10)
    async fn execute_webhook(&self, hook: &HookDefinition, ctx: &HookContext) -> HookResult {
        let config = &hook.config;

        let url = match config.get("url").and_then(|v| v.as_str()) {
            Some(u) => u.to_string(),
            None => {
                return HookResult::Failed {
                    error: "webhook hook missing required 'url' field".into(),
                    retryable: false,
                }
            }
        };

        let method_str = config
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("POST")
            .to_uppercase();
        let timeout_secs = config
            .get("timeout_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(10);

        // Build payload: task context as JSON
        let payload = serde_json::json!({
            "task_id": ctx.task_id.to_string(),
            "task_title": ctx.task_title,
            "task_description": ctx.task_description,
            "stage_name": ctx.stage_name,
            "pipeline_name": ctx.pipeline_name,
            "priority": ctx.priority,
            "trigger": match ctx.trigger {
                HookTrigger::Enter => "enter",
                HookTrigger::Exit => "exit",
                HookTrigger::OnStall => "on_stall",
            },
        });

        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                return HookResult::Failed {
                    error: format!("webhook: failed to build HTTP client: {e}"),
                    retryable: false,
                }
            }
        };

        let mut req = match method_str.as_str() {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "PATCH" => client.patch(&url),
            other => {
                return HookResult::Failed {
                    error: format!("webhook: unsupported method '{other}'"),
                    retryable: false,
                }
            }
        };

        // Add custom headers
        if let Some(headers) = config.get("headers").and_then(|v| v.as_object()) {
            for (k, v) in headers {
                if let Some(val) = v.as_str() {
                    req = req.header(k.as_str(), val);
                }
            }
        }

        // Add body for POST/PUT/PATCH
        if matches!(method_str.as_str(), "POST" | "PUT" | "PATCH") {
            let body = config
                .get("body")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| payload.to_string());
            req = req.header("Content-Type", "application/json").body(body);
        }

        tracing::info!(
            task_id = %ctx.task_id,
            stage = %ctx.stage_name,
            url = %url,
            method = %method_str,
            "webhook hook: sending request"
        );

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    HookResult::Success {
                        output: Some(format!("webhook responded {}", status.as_u16())),
                    }
                } else {
                    HookResult::Failed {
                        error: format!(
                            "webhook responded with non-2xx status: {}",
                            status.as_u16()
                        ),
                        retryable: status.as_u16() >= 500,
                    }
                }
            }
            Err(e) => HookResult::Failed {
                error: format!("webhook request failed: {e}"),
                retryable: true,
            },
        }
    }

    // ── Shell hook ──────────────────────────────────────────────────────────

    async fn execute_shell(&self, hook: &HookDefinition, ctx: &HookContext) -> HookResult {
        let config = &hook.config;

        let command = match config.get("command").and_then(|v| v.as_str()) {
            Some(cmd) => cmd.to_string(),
            None => {
                return HookResult::Failed {
                    error: "shell hook missing required 'command' field".into(),
                    retryable: false,
                }
            }
        };

        let timeout_secs = config.get("timeout_seconds").and_then(|v| v.as_u64());

        let working_dir = config
            .get("working_dir")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let trigger_str = match ctx.trigger {
            HookTrigger::Enter => "enter",
            HookTrigger::Exit => "exit",
            HookTrigger::OnStall => "on_stall",
        };

        // Build the command.
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&command);

        // Standard Molt env vars.
        cmd.env("MOLT_TASK_ID", ctx.task_id.to_string());
        cmd.env("MOLT_STAGE", &ctx.stage_name);
        cmd.env("MOLT_TRIGGER", trigger_str);
        cmd.env("MOLT_PIPELINE", &ctx.pipeline_name);
        cmd.env("MOLT_SESSION_ID", ctx.session_id.to_string());

        if let Some(ref agent_id) = ctx.agent_id {
            cmd.env("MOLT_AGENT_ID", agent_id.to_string());
        }

        // User-supplied extra env vars.
        for (k, v) in &ctx.env {
            cmd.env(k, v);
        }

        if let Some(ref dir) = working_dir {
            cmd.current_dir(dir);
        }

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        // Run, optionally with a timeout.
        let output_result = if let Some(secs) = timeout_secs {
            match timeout(Duration::from_secs(secs), cmd.output()).await {
                Ok(r) => r,
                Err(_) => {
                    return HookResult::Failed {
                        error: "hook timed out".into(),
                        retryable: false,
                    }
                }
            }
        } else {
            cmd.output().await
        };

        match output_result {
            Err(e) => HookResult::Failed {
                error: format!("failed to spawn process: {e}"),
                retryable: true,
            },
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let combined = if stderr.is_empty() {
                    stdout.clone()
                } else if stdout.is_empty() {
                    stderr.clone()
                } else {
                    format!("{stdout}\n{stderr}")
                };

                if output.status.success() {
                    HookResult::Success {
                        output: if combined.is_empty() {
                            None
                        } else {
                            Some(combined)
                        },
                    }
                } else {
                    let code = output
                        .status
                        .code()
                        .map(|c| format!("exit code {c}"))
                        .unwrap_or_else(|| "killed by signal".into());
                    HookResult::Failed {
                        error: if combined.is_empty() {
                            code
                        } else {
                            format!("{code}: {combined}")
                        },
                        retryable: true,
                    }
                }
            }
        }
    }
}

impl Default for HookExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for HookExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookExecutor")
            .field("has_adapter", &self.adapter.is_some())
            .finish()
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn hook_kind_name(kind: &HookKind) -> String {
    match kind {
        HookKind::Shell => "shell",
        HookKind::StartDevEnvironment => "start_dev_environment",
        HookKind::TeardownDevEnvironment => "teardown_dev_environment",
        HookKind::AgentDispatch => "agent_dispatch",
        HookKind::Webhook => "webhook",
    }
    .to_string()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use molt_hub_core::config::{HookDefinition, HookKind, HookTrigger, StageDefinition};
    use molt_hub_core::model::{AgentId, SessionId, TaskId};
    use serde_json::json;

    fn make_ctx() -> HookContext {
        HookContext {
            task_id: TaskId::new(),
            agent_id: Some(AgentId::new()),
            session_id: SessionId::new(),
            stage_name: "build".into(),
            trigger: HookTrigger::Enter,
            pipeline_name: "ci".into(),
            env: HashMap::new(),
            task_title: "Test Task".to_string(),
            task_description: "A test description.".to_string(),
            priority: "p1".to_string(),
        }
    }

    fn shell_hook(command: &str) -> HookDefinition {
        HookDefinition {
            kind: HookKind::Shell,
            on: HookTrigger::Enter,
            config: json!({ "command": command }),
        }
    }

    fn stage_with_hooks(hooks: Vec<HookDefinition>) -> StageDefinition {
        StageDefinition {
            name: "build".into(),
            label: None,
            instructions: None,
            instructions_template: None,
            requires_approval: false,
            approvers: vec![],
            timeout_seconds: None,
            terminal: false,
            hooks,
            transition_rules: vec![],
            color: None,
            order: 0,
            wip_limit: None,
        }
    }

    // ── Basic shell execution ────────────────────────────────────────────────

    #[tokio::test]
    async fn shell_hook_captures_output() {
        let executor = HookExecutor::new();
        let hook = shell_hook("echo hello");
        let ctx = make_ctx();

        let result = executor.execute_single(&hook, &ctx).await;

        assert_eq!(
            result,
            HookResult::Success {
                output: Some("hello".into())
            }
        );
    }

    #[tokio::test]
    async fn shell_hook_nonzero_exit_returns_failed() {
        let executor = HookExecutor::new();
        let hook = shell_hook("exit 1");
        let ctx = make_ctx();

        let result = executor.execute_single(&hook, &ctx).await;

        assert!(matches!(result, HookResult::Failed { .. }));
    }

    #[tokio::test]
    async fn shell_hook_missing_command_returns_failed() {
        let executor = HookExecutor::new();
        let hook = HookDefinition {
            kind: HookKind::Shell,
            on: HookTrigger::Enter,
            config: json!({}),
        };
        let ctx = make_ctx();

        let result = executor.execute_single(&hook, &ctx).await;

        assert!(matches!(
            result,
            HookResult::Failed {
                retryable: false,
                ..
            }
        ));
    }

    // ── Environment variables ────────────────────────────────────────────────

    #[tokio::test]
    async fn shell_hook_receives_molt_env_vars() {
        let executor = HookExecutor::new();
        let hook = shell_hook("echo $MOLT_STAGE");
        let ctx = make_ctx();

        let result = executor.execute_single(&hook, &ctx).await;

        assert_eq!(
            result,
            HookResult::Success {
                output: Some("build".into())
            }
        );
    }

    #[tokio::test]
    async fn shell_hook_receives_custom_env_vars() {
        let executor = HookExecutor::new();
        let hook = shell_hook("echo $MY_VAR");
        let mut ctx = make_ctx();
        ctx.env.insert("MY_VAR".into(), "custom_value".into());

        let result = executor.execute_single(&hook, &ctx).await;

        assert_eq!(
            result,
            HookResult::Success {
                output: Some("custom_value".into())
            }
        );
    }

    // ── Trigger filtering ────────────────────────────────────────────────────

    #[tokio::test]
    async fn hooks_filtered_by_trigger() {
        let executor = HookExecutor::new();

        let enter_hook = HookDefinition {
            kind: HookKind::Shell,
            on: HookTrigger::Enter,
            config: json!({ "command": "echo enter" }),
        };
        let exit_hook = HookDefinition {
            kind: HookKind::Shell,
            on: HookTrigger::Exit,
            config: json!({ "command": "echo exit" }),
        };
        let on_stall_hook = HookDefinition {
            kind: HookKind::Shell,
            on: HookTrigger::OnStall,
            config: json!({ "command": "echo stall" }),
        };

        let stage = stage_with_hooks(vec![enter_hook, exit_hook, on_stall_hook]);
        let ctx = make_ctx(); // trigger = Enter

        let results = executor
            .execute_hooks(&stage, HookTrigger::Enter, &ctx)
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0],
            HookResult::Success {
                output: Some("enter".into())
            }
        );
    }

    #[tokio::test]
    async fn no_hooks_for_trigger_returns_empty() {
        let executor = HookExecutor::new();
        let stage = stage_with_hooks(vec![]);
        let ctx = make_ctx();

        let results = executor
            .execute_hooks(&stage, HookTrigger::Exit, &ctx)
            .await
            .unwrap();

        assert!(results.is_empty());
    }

    // ── Sequential execution ─────────────────────────────────────────────────

    #[tokio::test]
    async fn sequential_execution_runs_in_order() {
        let executor = HookExecutor::new();

        // Use a temp file to track order; each hook appends a number.
        let tmp = std::env::temp_dir().join(format!("molt-hook-order-{}.txt", ulid::Ulid::new()));
        let path_str = tmp.to_string_lossy().to_string();

        let make_hook = |n: u32| HookDefinition {
            kind: HookKind::Shell,
            on: HookTrigger::Enter,
            config: json!({ "command": format!("printf {n} >> {path_str}") }),
        };

        let stage = stage_with_hooks(vec![make_hook(1), make_hook(2), make_hook(3)]);
        let ctx = make_ctx();

        let results = executor
            .execute_hooks(&stage, HookTrigger::Enter, &ctx)
            .await
            .unwrap();

        assert_eq!(results.len(), 3);
        let content = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert_eq!(content, "123");

        let _ = std::fs::remove_file(tmp);
    }

    // ── Failure policies ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn abort_policy_stops_on_first_failure() {
        let executor = HookExecutor::new();

        let failing = HookDefinition {
            kind: HookKind::Shell,
            on: HookTrigger::Enter,
            config: json!({ "command": "exit 1", "failure_policy": "abort" }),
        };
        let should_not_run = HookDefinition {
            kind: HookKind::Shell,
            on: HookTrigger::Enter,
            config: json!({ "command": "echo should-not-run", "failure_policy": "abort" }),
        };

        let stage = stage_with_hooks(vec![failing, should_not_run]);
        let ctx = make_ctx();

        let err = executor
            .execute_hooks(&stage, HookTrigger::Enter, &ctx)
            .await
            .unwrap_err();

        assert!(matches!(err, HookExecutorError::Aborted { .. }));
    }

    #[tokio::test]
    async fn continue_policy_proceeds_past_failures() {
        let executor = HookExecutor::new();

        let failing = HookDefinition {
            kind: HookKind::Shell,
            on: HookTrigger::Enter,
            config: json!({ "command": "exit 1", "failure_policy": "continue" }),
        };
        let succeeding = HookDefinition {
            kind: HookKind::Shell,
            on: HookTrigger::Enter,
            config: json!({ "command": "echo ok", "failure_policy": "continue" }),
        };

        let stage = stage_with_hooks(vec![failing, succeeding]);
        let ctx = make_ctx();

        let results = executor
            .execute_hooks(&stage, HookTrigger::Enter, &ctx)
            .await
            .unwrap();

        assert_eq!(results.len(), 2);
        assert!(matches!(results[0], HookResult::Failed { .. }));
        assert_eq!(
            results[1],
            HookResult::Success {
                output: Some("ok".into())
            }
        );
    }

    #[tokio::test]
    async fn retry_policy_retries_specified_times() {
        let executor = HookExecutor::new();

        // A command that always fails.
        let hook = HookDefinition {
            kind: HookKind::Shell,
            on: HookTrigger::Enter,
            config: json!({
                "command": "exit 1",
                "failure_policy": "retry",
                "retry_max_attempts": 3,
                "retry_delay_ms": 0
            }),
        };

        let stage = stage_with_hooks(vec![hook]);
        let ctx = make_ctx();

        let err = executor
            .execute_hooks(&stage, HookTrigger::Enter, &ctx)
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            HookExecutorError::AllRetriesExhausted { attempts: 3, .. }
        ));
    }

    // ── Timeout ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn timeout_cancels_long_running_hook() {
        let executor = HookExecutor::new();
        let hook = HookDefinition {
            kind: HookKind::Shell,
            on: HookTrigger::Enter,
            config: json!({
                "command": "sleep 60",
                "timeout_seconds": 1
            }),
        };
        let ctx = make_ctx();

        let result = executor.execute_single(&hook, &ctx).await;

        assert!(matches!(
            result,
            HookResult::Failed { ref error, .. } if error.contains("timed out")
        ));
    }

    // ── Placeholder hooks ────────────────────────────────────────────────────

    #[tokio::test]
    async fn placeholder_hooks_return_success() {
        let executor = HookExecutor::new();
        let ctx = make_ctx();

        for kind in [
            HookKind::StartDevEnvironment,
            HookKind::TeardownDevEnvironment,
        ] {
            let hook = HookDefinition {
                kind,
                on: HookTrigger::Enter,
                config: json!({}),
            };
            let result = executor.execute_single(&hook, &ctx).await;
            assert!(
                matches!(result, HookResult::Success { output: None }),
                "expected Success{{None}} for placeholder hook"
            );
        }
    }

    // ── AgentDispatch hook ────────────────────────────────────────────────────

    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use molt_hub_core::model::AgentStatus;
    use molt_hub_harness::adapter::{
        AdapterError, AgentAdapter, AgentHandle, AgentMessage, SpawnConfig,
    };

    struct MockAdapter {
        spawn_count: Arc<AtomicUsize>,
        /// Status that `status()` returns after spawn.
        fixed_status: AgentStatus,
        fail_spawn: bool,
    }

    impl MockAdapter {
        fn completing() -> Self {
            Self {
                spawn_count: Arc::new(AtomicUsize::new(0)),
                fixed_status: AgentStatus::Completed,
                fail_spawn: false,
            }
        }

        fn failing_spawn() -> Self {
            Self {
                spawn_count: Arc::new(AtomicUsize::new(0)),
                fixed_status: AgentStatus::Running,
                fail_spawn: true,
            }
        }

        fn crashing() -> Self {
            Self {
                spawn_count: Arc::new(AtomicUsize::new(0)),
                fixed_status: AgentStatus::Crashed {
                    error: "oh no".into(),
                },
                fail_spawn: false,
            }
        }
    }

    #[async_trait]
    impl AgentAdapter for MockAdapter {
        async fn spawn(&self, config: SpawnConfig) -> Result<AgentHandle, AdapterError> {
            if self.fail_spawn {
                return Err(AdapterError::SpawnFailed("mock spawn failure".into()));
            }
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
            Ok(self.fixed_status.clone())
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

    #[tokio::test]
    async fn agent_dispatch_without_adapter_skips() {
        let executor = HookExecutor::new(); // no adapter
        let hook = HookDefinition {
            kind: HookKind::AgentDispatch,
            on: HookTrigger::Enter,
            config: json!({ "instruction": "do something" }),
        };
        let result = executor.execute_single(&hook, &make_ctx()).await;
        assert!(matches!(result, HookResult::Skipped { .. }));
    }

    #[tokio::test]
    async fn agent_dispatch_missing_instruction_returns_failed() {
        let adapter: Arc<dyn AgentAdapter> = Arc::new(MockAdapter::completing());
        let executor = HookExecutor::with_adapter(adapter);
        let hook = HookDefinition {
            kind: HookKind::AgentDispatch,
            on: HookTrigger::Enter,
            config: json!({}), // no instruction
        };
        let result = executor.execute_single(&hook, &make_ctx()).await;
        assert!(matches!(
            result,
            HookResult::Failed {
                retryable: false,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn agent_dispatch_spawn_failure_returns_failed() {
        let adapter: Arc<dyn AgentAdapter> = Arc::new(MockAdapter::failing_spawn());
        let executor = HookExecutor::with_adapter(adapter);
        let hook = HookDefinition {
            kind: HookKind::AgentDispatch,
            on: HookTrigger::Enter,
            config: json!({ "instruction": "do something" }),
        };
        let result = executor.execute_single(&hook, &make_ctx()).await;
        assert!(
            matches!(
                result,
                HookResult::Failed {
                    retryable: true,
                    ..
                }
            ),
            "spawn failure should be retryable, got {result:?}"
        );
    }

    #[tokio::test]
    async fn agent_dispatch_completing_agent_returns_success() {
        let adapter: Arc<dyn AgentAdapter> = Arc::new(MockAdapter::completing());
        let executor = HookExecutor::with_adapter(adapter);
        let hook = HookDefinition {
            kind: HookKind::AgentDispatch,
            on: HookTrigger::Enter,
            config: json!({ "instruction": "run tests", "timeout_seconds": 10 }),
        };
        let result = executor.execute_single(&hook, &make_ctx()).await;
        assert!(
            matches!(result, HookResult::Success { .. }),
            "expected Success, got {result:?}"
        );
    }

    #[tokio::test]
    async fn agent_dispatch_crashed_agent_returns_success_fire_and_forget() {
        // With fire-and-forget dispatch, we return Success immediately after spawn
        // regardless of eventual agent status; crash detection happens via WS events.
        let adapter: Arc<dyn AgentAdapter> = Arc::new(MockAdapter::crashing());
        let executor = HookExecutor::with_adapter(adapter);
        let hook = HookDefinition {
            kind: HookKind::AgentDispatch,
            on: HookTrigger::Enter,
            config: json!({ "instruction": "run tests", "timeout_seconds": 10 }),
        };
        let result = executor.execute_single(&hook, &make_ctx()).await;
        assert!(
            matches!(result, HookResult::Success { .. }),
            "expected Success (fire-and-forget), got {result:?}"
        );
    }

    // ── FailurePolicy parsing ────────────────────────────────────────────────

    #[test]
    fn failure_policy_defaults_to_abort() {
        let policy = FailurePolicy::from_config(&json!({}));
        assert_eq!(policy, FailurePolicy::Abort);
    }

    #[test]
    fn failure_policy_continue_parsed() {
        let policy = FailurePolicy::from_config(&json!({ "failure_policy": "continue" }));
        assert_eq!(policy, FailurePolicy::Continue);
    }

    #[test]
    fn failure_policy_retry_parsed_with_defaults() {
        let policy = FailurePolicy::from_config(&json!({ "failure_policy": "retry" }));
        assert_eq!(
            policy,
            FailurePolicy::Retry {
                max_attempts: 3,
                delay_ms: 500
            }
        );
    }

    // ── ExecutionMode parsing ────────────────────────────────────────────────

    #[test]
    fn execution_mode_defaults_to_sequential() {
        let mode = ExecutionMode::from_config(&json!({}));
        assert_eq!(mode, ExecutionMode::Sequential);
    }

    #[test]
    fn execution_mode_parallel_parsed() {
        let mode = ExecutionMode::from_config(&json!({ "execution_mode": "parallel" }));
        assert_eq!(mode, ExecutionMode::Parallel);
    }

    // ── Webhook hook ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn webhook_missing_url_returns_failed_non_retryable() {
        let executor = HookExecutor::new();
        let hook = HookDefinition {
            kind: HookKind::Webhook,
            on: HookTrigger::Enter,
            config: json!({}),
        };
        let ctx = make_ctx();

        let result = executor.execute_single(&hook, &ctx).await;

        assert!(matches!(
            result,
            HookResult::Failed {
                retryable: false,
                ..
            }
        ));
        if let HookResult::Failed { error, .. } = result {
            assert!(error.contains("url"), "error should mention 'url': {error}");
        }
    }

    #[tokio::test]
    async fn webhook_invalid_url_returns_failed_retryable() {
        let executor = HookExecutor::new();
        let hook = HookDefinition {
            kind: HookKind::Webhook,
            on: HookTrigger::Enter,
            config: json!({ "url": "http://127.0.0.1:1", "timeout_seconds": 1 }),
        };
        let ctx = make_ctx();

        let result = executor.execute_single(&hook, &ctx).await;

        assert!(matches!(result, HookResult::Failed { retryable: true, .. }));
    }

    #[tokio::test]
    async fn webhook_unsupported_method_returns_failed_non_retryable() {
        let executor = HookExecutor::new();
        let hook = HookDefinition {
            kind: HookKind::Webhook,
            on: HookTrigger::Enter,
            config: json!({ "url": "http://example.com", "method": "DELETE" }),
        };
        let ctx = make_ctx();

        let result = executor.execute_single(&hook, &ctx).await;

        assert!(matches!(
            result,
            HookResult::Failed {
                retryable: false,
                ..
            }
        ));
    }
}
