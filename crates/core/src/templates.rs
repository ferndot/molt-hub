//! Instruction templating — variable interpolation for agent prompts and stage instructions.
//!
//! Provides a Handlebars-based template engine with a restricted set of context variables.
//! Strict mode is enabled so typos in variable names produce clear errors rather than silent
//! empty strings.

use std::collections::HashMap;

use handlebars::Handlebars;
use serde::Serialize;
use thiserror::Error;

use crate::config::PipelineConfig;
use crate::model::{Agent, Priority, Task};

// ─── Error type ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Error)]
pub enum TemplateError {
    #[error("template parse error: {0}")]
    ParseError(String),

    #[error("template render error: {0}")]
    RenderError(String),

    #[error("template not found: '{0}'")]
    TemplateNotFound(String),

    #[error("invalid variable in template: '{0}'")]
    InvalidVariable(String),
}

impl From<handlebars::TemplateError> for TemplateError {
    fn from(e: handlebars::TemplateError) -> Self {
        TemplateError::ParseError(e.to_string())
    }
}

impl From<handlebars::RenderError> for TemplateError {
    fn from(e: handlebars::RenderError) -> Self {
        // Handlebars strict mode surfaces missing variables as a RenderError.
        // Try to surface a more actionable variant when applicable.
        let msg = e.to_string();
        if msg.contains("Variable") || msg.contains("not found") || msg.contains("strict") {
            TemplateError::InvalidVariable(msg)
        } else {
            TemplateError::RenderError(msg)
        }
    }
}

// ─── TemplateContext ──────────────────────────────────────────────────────────

/// The restricted set of variables available inside an instruction template.
///
/// Derive `Serialize` so Handlebars can traverse the struct directly as the
/// data context (no manual map building required).
#[derive(Debug, Clone, Serialize)]
pub struct TemplateContext {
    pub task_id: String,
    pub task_title: String,
    pub task_description: String,
    pub stage_name: String,
    pub pipeline_name: String,
    pub agent_name: Option<String>,
    pub agent_type: Option<String>,
    pub priority: String,
    /// User-defined key/value pairs from the pipeline config.
    pub custom: HashMap<String, String>,
}

impl TemplateContext {
    /// Convenience constructor from runtime domain types.
    pub fn from_task_and_stage(
        task: &Task,
        pipeline_name: impl Into<String>,
        stage_name: impl Into<String>,
        agent: Option<&Agent>,
    ) -> Self {
        let priority = match task.priority {
            Priority::P0 => "p0",
            Priority::P1 => "p1",
            Priority::P2 => "p2",
            Priority::P3 => "p3",
        }
        .to_string();

        TemplateContext {
            task_id: task.id.to_string(),
            task_title: task.title.clone(),
            task_description: task.description.clone(),
            stage_name: stage_name.into(),
            pipeline_name: pipeline_name.into(),
            agent_name: agent.map(|a| a.name.clone()),
            agent_type: agent.map(|a| a.adapter_type.clone()),
            priority,
            custom: HashMap::new(),
        }
    }

    /// Attach user-defined custom variables to the context.
    pub fn with_custom(mut self, custom: HashMap<String, String>) -> Self {
        self.custom = custom;
        self
    }
}

// ─── TemplateEngine ───────────────────────────────────────────────────────────

/// A thin wrapper around `handlebars::Handlebars` that enforces strict variable
/// checking and provides a domain-oriented API.
pub struct TemplateEngine {
    hbs: Handlebars<'static>,
}

impl TemplateEngine {
    /// Create a new engine with strict mode enabled.
    ///
    /// In strict mode, referencing a variable that is not present in the context
    /// produces a `TemplateError::InvalidVariable` rather than rendering as empty.
    pub fn new() -> Self {
        let mut hbs = Handlebars::new();
        hbs.set_strict_mode(true);
        TemplateEngine { hbs }
    }

    /// Register a named template from a source string.
    pub fn register_template(
        &mut self,
        name: impl AsRef<str>,
        source: impl AsRef<str>,
    ) -> Result<(), TemplateError> {
        self.hbs
            .register_template_string(name.as_ref(), source.as_ref())
            .map_err(TemplateError::from)
    }

    /// Register all stage `instructions_template` values found in a `PipelineConfig`.
    ///
    /// Template names are derived from the stage name: `"stage/<stage_name>"`.
    /// Stages that have no `instructions_template` are silently skipped.
    pub fn register_templates_from_config(
        &mut self,
        config: &PipelineConfig,
    ) -> Result<(), TemplateError> {
        for stage in &config.stages {
            if let Some(ref tmpl) = stage.instructions_template {
                let name = format!("stage/{}", stage.name);
                self.register_template(&name, tmpl)?;
            }
        }
        Ok(())
    }

    /// Render a previously registered named template with the given context.
    pub fn render(
        &self,
        template_name: impl AsRef<str>,
        context: &TemplateContext,
    ) -> Result<String, TemplateError> {
        let name = template_name.as_ref();
        if !self.hbs.has_template(name) {
            return Err(TemplateError::TemplateNotFound(name.to_string()));
        }
        self.hbs
            .render(name, context)
            .map_err(TemplateError::from)
    }

    /// Render an inline template string directly without registering it.
    pub fn render_inline(
        &self,
        template_source: impl AsRef<str>,
        context: &TemplateContext,
    ) -> Result<String, TemplateError> {
        self.hbs
            .render_template(template_source.as_ref(), context)
            .map_err(TemplateError::from)
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        AgentId, AgentStatus, PipelineId, Priority, SessionId, Task, TaskId, TaskState,
    };
    use chrono::Utc;
    use ulid::Ulid;

    fn make_task() -> Task {
        Task {
            id: TaskId(Ulid::new()),
            pipeline_id: PipelineId(Ulid::new()),
            title: "My Task".to_string(),
            description: "A detailed description.".to_string(),
            current_stage: "planning".to_string(),
            state: TaskState::InProgress,
            priority: Priority::P1,
            assigned_agent: None,
            session_id: SessionId(Ulid::new()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn make_agent() -> Agent {
        Agent {
            id: AgentId(Ulid::new()),
            name: "agent-alpha".to_string(),
            adapter_type: "claude-sdk".to_string(),
            status: AgentStatus::Running,
            task_id: None,
            session_id: SessionId(Ulid::new()),
            started_at: Utc::now(),
            last_activity_at: Utc::now(),
        }
    }

    fn basic_context() -> TemplateContext {
        TemplateContext {
            task_id: "task-001".to_string(),
            task_title: "My Task".to_string(),
            task_description: "A detailed description.".to_string(),
            stage_name: "planning".to_string(),
            pipeline_name: "default".to_string(),
            agent_name: None,
            agent_type: None,
            priority: "p1".to_string(),
            custom: HashMap::new(),
        }
    }

    // ── Basic substitution ────────────────────────────────────────────────────

    #[test]
    fn basic_variable_substitution() {
        let engine = TemplateEngine::new();
        let ctx = basic_context();
        let result = engine
            .render_inline("Hello {{task_title}}", &ctx)
            .expect("should render");
        assert_eq!(result, "Hello My Task");
    }

    // ── All TemplateContext fields are accessible ──────────────────────────────

    #[test]
    fn all_fields_accessible() {
        let engine = TemplateEngine::new();
        let ctx = TemplateContext {
            task_id: "t1".to_string(),
            task_title: "Title".to_string(),
            task_description: "Desc".to_string(),
            stage_name: "impl".to_string(),
            pipeline_name: "pipe".to_string(),
            agent_name: Some("bot".to_string()),
            agent_type: Some("claude-sdk".to_string()),
            priority: "p0".to_string(),
            custom: HashMap::new(),
        };
        let tmpl = "{{task_id}} {{task_title}} {{task_description}} {{stage_name}} \
                    {{pipeline_name}} {{agent_name}} {{agent_type}} {{priority}}";
        let result = engine.render_inline(tmpl, &ctx).expect("render");
        assert!(result.contains("t1"));
        assert!(result.contains("Title"));
        assert!(result.contains("Desc"));
        assert!(result.contains("impl"));
        assert!(result.contains("pipe"));
        assert!(result.contains("bot"));
        assert!(result.contains("claude-sdk"));
        assert!(result.contains("p0"));
    }

    // ── Custom variables ──────────────────────────────────────────────────────

    #[test]
    fn custom_variables_accessible() {
        let engine = TemplateEngine::new();
        let mut custom = HashMap::new();
        custom.insert("repo_url".to_string(), "https://github.com/org/repo".to_string());
        let ctx = basic_context().with_custom(custom);
        let result = engine
            .render_inline("{{custom.repo_url}}", &ctx)
            .expect("render");
        assert_eq!(result, "https://github.com/org/repo");
    }

    // ── Strict mode: undefined variable ───────────────────────────────────────

    #[test]
    fn undefined_variable_in_strict_mode_returns_error() {
        let engine = TemplateEngine::new();
        let ctx = basic_context();
        let result = engine.render_inline("{{nonexistent_field}}", &ctx);
        assert!(result.is_err(), "expected error for undefined variable");
        match result.unwrap_err() {
            TemplateError::InvalidVariable(_) => {}
            other => panic!("expected InvalidVariable, got {other:?}"),
        }
    }

    // ── Named template registration and rendering ─────────────────────────────

    #[test]
    fn register_and_render_named_template() {
        let mut engine = TemplateEngine::new();
        engine
            .register_template("greet", "Task: {{task_title}} — Stage: {{stage_name}}")
            .expect("register");
        let ctx = basic_context();
        let result = engine.render("greet", &ctx).expect("render");
        assert_eq!(result, "Task: My Task — Stage: planning");
    }

    #[test]
    fn render_missing_named_template_returns_error() {
        let engine = TemplateEngine::new();
        let ctx = basic_context();
        let result = engine.render("does_not_exist", &ctx);
        assert!(matches!(result, Err(TemplateError::TemplateNotFound(_))));
    }

    // ── Inline rendering ──────────────────────────────────────────────────────

    #[test]
    fn inline_template_renders_correctly() {
        let engine = TemplateEngine::new();
        let ctx = basic_context();
        let result = engine
            .render_inline("Priority: {{priority}}", &ctx)
            .expect("render");
        assert_eq!(result, "Priority: p1");
    }

    // ── Conditional template ──────────────────────────────────────────────────

    #[test]
    fn conditional_with_agent_name_present() {
        let engine = TemplateEngine::new();
        let ctx = TemplateContext {
            agent_name: Some("agent-alpha".to_string()),
            ..basic_context()
        };
        let result = engine
            .render_inline(
                "{{#if agent_name}}Agent: {{agent_name}}{{/if}}",
                &ctx,
            )
            .expect("render");
        assert_eq!(result, "Agent: agent-alpha");
    }

    #[test]
    fn conditional_with_agent_name_absent() {
        let engine = TemplateEngine::new();
        let ctx = basic_context(); // agent_name is None
        let result = engine
            .render_inline(
                "{{#if agent_name}}Agent: {{agent_name}}{{/if}}",
                &ctx,
            )
            .expect("render");
        assert_eq!(result, "");
    }

    // ── register_templates_from_config ───────────────────────────────────────

    #[test]
    fn register_from_config_and_render() {
        use crate::config::{PipelineConfig, StageDefinition};

        let config = PipelineConfig {
            name: "test-pipe".to_string(),
            description: None,
            version: 1,
            stages: vec![
                StageDefinition {
                    name: "work".to_string(),
                    instructions: None,
                    instructions_template: Some(
                        "Work on: {{task_title}} ({{priority}})".to_string(),
                    ),
                    requires_approval: false,
                    approvers: vec![],
                    timeout_seconds: None,
                    terminal: false,
                    hooks: vec![],
                    transition_rules: vec![],
                },
                StageDefinition {
                    name: "done".to_string(),
                    instructions: Some("You are done.".to_string()),
                    instructions_template: None, // no template for this stage
                    requires_approval: false,
                    approvers: vec![],
                    timeout_seconds: None,
                    terminal: true,
                    hooks: vec![],
                    transition_rules: vec![],
                },
            ],
            integrations: vec![],
            columns: vec![],
        };

        let mut engine = TemplateEngine::new();
        engine
            .register_templates_from_config(&config)
            .expect("register from config");

        let ctx = basic_context();
        let result = engine.render("stage/work", &ctx).expect("render stage/work");
        assert_eq!(result, "Work on: My Task (p1)");

        // "done" stage has no template, so it should not be registered
        assert!(matches!(
            engine.render("stage/done", &ctx),
            Err(TemplateError::TemplateNotFound(_))
        ));
    }

    // ── from_task_and_stage convenience builder ───────────────────────────────

    #[test]
    fn from_task_and_stage_without_agent() {
        let task = make_task();
        let ctx =
            TemplateContext::from_task_and_stage(&task, "my-pipeline", "planning", None);
        assert_eq!(ctx.task_title, "My Task");
        assert_eq!(ctx.pipeline_name, "my-pipeline");
        assert_eq!(ctx.stage_name, "planning");
        assert_eq!(ctx.priority, "p1");
        assert!(ctx.agent_name.is_none());
        assert!(ctx.agent_type.is_none());
    }

    #[test]
    fn from_task_and_stage_with_agent() {
        let task = make_task();
        let agent = make_agent();
        let ctx = TemplateContext::from_task_and_stage(
            &task,
            "my-pipeline",
            "implementation",
            Some(&agent),
        );
        assert_eq!(ctx.agent_name.as_deref(), Some("agent-alpha"));
        assert_eq!(ctx.agent_type.as_deref(), Some("claude-sdk"));
    }
}
