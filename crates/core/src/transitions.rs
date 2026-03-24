//! Transition rules — predicates and guards that govern state machine advancement.
//!
//! This module sits between the raw state machine (`machine.rs`) and the pipeline
//! config (`config.rs`), making the state machine config-driven.
//!
//! # Overview
//!
//! Given a `DomainEvent` and a `StageDefinition`, the engine:
//! 1. Finds the matching `TransitionDefinition` by comparing event type to `when` trigger.
//! 2. Evaluates the guard predicate (if any) against a `GuardContext`.
//! 3. Returns a `TransitionResult` describing what should happen next.

use serde_json::Value;

use crate::config::{PipelineConfig, StageDefinition, TransitionTrigger};
use crate::events::{DomainEvent, HumanDecisionKind};
use crate::model::{Priority, TaskOutcome, TaskState};

// ─── TransitionResult ─────────────────────────────────────────────────────────

/// The outcome of evaluating transition rules against an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionResult {
    /// No rule matched the incoming event.
    NoMatch,
    /// Transition the task to a different (non-terminal) stage.
    MoveTo { stage: String },
    /// Task has reached a terminal state.
    Complete { outcome: TaskOutcome },
    /// Block for human review before proceeding.
    RequiresApproval,
}

// ─── GuardContext ─────────────────────────────────────────────────────────────

/// Data available to guard predicates when evaluating a transition rule.
#[derive(Debug, Clone)]
pub struct GuardContext {
    pub current_stage: String,
    pub task_state: TaskState,
    pub priority: Priority,
    pub event: DomainEvent,
}

// ─── Guard evaluation ─────────────────────────────────────────────────────────

/// Evaluate a guard predicate against the current context.
///
/// Supported guard forms:
/// - `{"priority": "p0"}` — matches if task priority equals the given value (case-insensitive).
/// - `{"state": "in_progress"}` — matches if task state name equals the given value.
/// - `{"all": [...]}` — all sub-guards must pass.
/// - `{"any": [...]}` — at least one sub-guard must pass.
/// - `null` / empty object / unknown keys — passes unconditionally.
pub fn evaluate_guard(guard: &Value, ctx: &GuardContext) -> bool {
    match guard {
        Value::Null => true,
        Value::Object(map) => {
            if map.is_empty() {
                return true;
            }

            // Check each key we understand; unknown keys are ignored (pass through).
            let mut any_known_key = false;

            // `all` combinator
            if let Some(Value::Array(sub_guards)) = map.get("all") {
                any_known_key = true;
                if !sub_guards.iter().all(|g| evaluate_guard(g, ctx)) {
                    return false;
                }
            }

            // `any` combinator
            if let Some(Value::Array(sub_guards)) = map.get("any") {
                any_known_key = true;
                if !sub_guards.iter().any(|g| evaluate_guard(g, ctx)) {
                    return false;
                }
            }

            // `priority` predicate — compare against Priority enum
            if let Some(Value::String(expected)) = map.get("priority") {
                any_known_key = true;
                let actual = priority_name(&ctx.priority);
                if !strings_equal_ci(actual, expected) {
                    return false;
                }
            }

            // `state` predicate — compare against TaskState discriminant
            if let Some(Value::String(expected)) = map.get("state") {
                any_known_key = true;
                let actual = state_name(&ctx.task_state);
                if !strings_equal_ci(actual, expected) {
                    return false;
                }
            }

            // `stage` predicate — compare against current stage name
            if let Some(Value::String(expected)) = map.get("stage") {
                any_known_key = true;
                if !strings_equal_ci(&ctx.current_stage, expected) {
                    return false;
                }
            }

            // If no known keys were present at all, treat as no restriction.
            let _ = any_known_key;
            true
        }
        // Any non-object, non-null guard value is treated as "no restriction".
        _ => true,
    }
}

fn strings_equal_ci(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn priority_name(p: &Priority) -> &'static str {
    match p {
        Priority::P0 => "p0",
        Priority::P1 => "p1",
        Priority::P2 => "p2",
        Priority::P3 => "p3",
    }
}

fn state_name(s: &TaskState) -> &'static str {
    match s {
        TaskState::Pending => "pending",
        TaskState::InProgress => "in_progress",
        TaskState::Blocked { .. } => "blocked",
        TaskState::AwaitingApproval { .. } => "awaiting_approval",
        TaskState::Completed { .. } => "completed",
        TaskState::Failed { .. } => "failed",
    }
}

// ─── Trigger matching ─────────────────────────────────────────────────────────

/// Returns `true` if the given `DomainEvent` corresponds to the given `TransitionTrigger`.
pub fn event_matches_trigger(event: &DomainEvent, trigger: &TransitionTrigger) -> bool {
    match (event, trigger) {
        (DomainEvent::AgentCompleted { .. }, TransitionTrigger::AgentCompleted) => true,
        (
            DomainEvent::HumanDecision {
                decision: HumanDecisionKind::Approved,
                ..
            },
            TransitionTrigger::Approved,
        ) => true,
        (
            DomainEvent::HumanDecision {
                decision: HumanDecisionKind::Rejected { .. },
                ..
            },
            TransitionTrigger::Rejected,
        ) => true,
        // Manual trigger has no automatic event mapping — it must be driven externally.
        // Timeout trigger similarly has no event in DomainEvent yet.
        _ => false,
    }
}

// ─── is_terminal_stage ────────────────────────────────────────────────────────

/// Returns `true` if the named stage is marked `terminal` in the pipeline config.
pub fn is_terminal_stage(config: &PipelineConfig, stage_name: &str) -> bool {
    config
        .stages
        .iter()
        .any(|s| s.name == stage_name && s.terminal)
}

// ─── TransitionEngine ─────────────────────────────────────────────────────────

/// Pure, stateless evaluator that maps (stage config, event, context) → `TransitionResult`.
///
/// The engine does not mutate any state; callers are responsible for acting on
/// the returned `TransitionResult`.
pub struct TransitionEngine;

impl TransitionEngine {
    /// Evaluate transition rules for a single stage definition.
    ///
    /// Steps:
    /// 1. Find the first `TransitionDefinition` whose `when` trigger matches the event.
    /// 2. Evaluate the guard (if any); if it fails, return `NoMatch`.
    /// 3. If the stage requires approval, return `RequiresApproval`.
    /// 4. Resolve the `then` target: terminal stage → `Complete`, otherwise `MoveTo`.
    pub fn evaluate(
        stage: &StageDefinition,
        event: &DomainEvent,
        ctx: &GuardContext,
        config: &PipelineConfig,
    ) -> TransitionResult {
        // Find the first matching rule.
        let matched = stage
            .transition_rules
            .iter()
            .find(|rule| event_matches_trigger(event, &rule.when));

        let rule = match matched {
            Some(r) => r,
            None => return TransitionResult::NoMatch,
        };

        // Evaluate guard if present.
        if let Some(guard) = &rule.guard {
            if !evaluate_guard(guard, ctx) {
                return TransitionResult::NoMatch;
            }
        }

        // Check requires_approval on the *current* stage before advancing.
        if stage.requires_approval {
            return TransitionResult::RequiresApproval;
        }

        // Resolve the target stage.
        let target = &rule.then;
        if is_terminal_stage(config, target) {
            TransitionResult::Complete {
                outcome: TaskOutcome::Success,
            }
        } else {
            TransitionResult::MoveTo {
                stage: target.clone(),
            }
        }
    }

    /// Evaluate transition rules by looking up the current stage in the pipeline config.
    ///
    /// Returns `NoMatch` if `current_stage` is not found in the pipeline.
    pub fn evaluate_pipeline(
        config: &PipelineConfig,
        current_stage: &str,
        event: &DomainEvent,
        ctx: &GuardContext,
    ) -> TransitionResult {
        let stage = config.stages.iter().find(|s| s.name == current_stage);
        match stage {
            Some(s) => Self::evaluate(s, event, ctx, config),
            None => TransitionResult::NoMatch,
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PipelineConfig, StageDefinition, TransitionDefinition, TransitionTrigger};
    use crate::events::DomainEvent;
    use crate::model::{AgentId, Priority, TaskOutcome, TaskState};

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn agent_completed_event() -> DomainEvent {
        DomainEvent::AgentCompleted {
            agent_id: AgentId::new(),
            summary: None,
        }
    }

    fn human_approved_event() -> DomainEvent {
        DomainEvent::HumanDecision {
            decided_by: "alice".into(),
            decision: HumanDecisionKind::Approved,
            note: None,
        }
    }

    fn human_rejected_event() -> DomainEvent {
        DomainEvent::HumanDecision {
            decided_by: "alice".into(),
            decision: HumanDecisionKind::Rejected {
                reason: "not good enough".into(),
            },
            note: None,
        }
    }

    fn default_ctx(event: DomainEvent) -> GuardContext {
        GuardContext {
            current_stage: "implementation".into(),
            task_state: TaskState::InProgress,
            priority: Priority::P2,
            event,
        }
    }

    fn p0_ctx(event: DomainEvent) -> GuardContext {
        GuardContext {
            priority: Priority::P0,
            ..default_ctx(event)
        }
    }

    /// Build a minimal pipeline with two stages:
    ///   implementation (non-terminal) → done (terminal)
    fn two_stage_pipeline(transition_rules: Vec<TransitionDefinition>) -> PipelineConfig {
        PipelineConfig {
            name: "test".into(),
            description: None,
            version: 1,
            stages: vec![
                StageDefinition {
                    name: "implementation".into(),
                    label: None,
                    instructions: None,
                    instructions_template: None,
                    requires_approval: false,
                    approvers: vec![],
                    timeout_seconds: None,
                    terminal: false,
                    hooks: vec![],
                    transition_rules,
                    color: None,
                    order: 0,
                    wip_limit: None,
                },
                StageDefinition {
                    name: "done".into(),
                    label: None,
                    instructions: None,
                    instructions_template: None,
                    requires_approval: false,
                    approvers: vec![],
                    timeout_seconds: None,
                    terminal: true,
                    hooks: vec![],
                    transition_rules: vec![],
                    color: None,
                    order: 0,
                    wip_limit: None,
                },
            ],
            integrations: vec![],
            columns: vec![],
        }
    }

    fn three_stage_pipeline() -> PipelineConfig {
        PipelineConfig {
            name: "test".into(),
            description: None,
            version: 1,
            stages: vec![
                StageDefinition {
                    name: "implementation".into(),
                    label: None,
                    instructions: None,
                    instructions_template: None,
                    requires_approval: false,
                    approvers: vec![],
                    timeout_seconds: None,
                    terminal: false,
                    hooks: vec![],
                    transition_rules: vec![TransitionDefinition {
                        when: TransitionTrigger::AgentCompleted,
                        then: "review".into(),
                        guard: None,
                    }],
                    color: None,
                    order: 0,
                    wip_limit: None,
                },
                StageDefinition {
                    name: "review".into(),
                    label: None,
                    instructions: None,
                    instructions_template: None,
                    requires_approval: false,
                    approvers: vec![],
                    timeout_seconds: None,
                    terminal: false,
                    hooks: vec![],
                    transition_rules: vec![TransitionDefinition {
                        when: TransitionTrigger::Approved,
                        then: "done".into(),
                        guard: None,
                    }],
                    color: None,
                    order: 0,
                    wip_limit: None,
                },
                StageDefinition {
                    name: "done".into(),
                    label: None,
                    instructions: None,
                    instructions_template: None,
                    requires_approval: false,
                    approvers: vec![],
                    timeout_seconds: None,
                    terminal: true,
                    hooks: vec![],
                    transition_rules: vec![],
                    color: None,
                    order: 0,
                    wip_limit: None,
                },
            ],
            integrations: vec![],
            columns: vec![],
        }
    }

    // ── event_matches_trigger ─────────────────────────────────────────────────

    #[test]
    fn agent_completed_matches_trigger() {
        assert!(event_matches_trigger(
            &agent_completed_event(),
            &TransitionTrigger::AgentCompleted
        ));
    }

    #[test]
    fn human_approved_matches_trigger() {
        assert!(event_matches_trigger(
            &human_approved_event(),
            &TransitionTrigger::Approved
        ));
    }

    #[test]
    fn human_rejected_matches_trigger() {
        assert!(event_matches_trigger(
            &human_rejected_event(),
            &TransitionTrigger::Rejected
        ));
    }

    #[test]
    fn agent_completed_does_not_match_approved() {
        assert!(!event_matches_trigger(
            &agent_completed_event(),
            &TransitionTrigger::Approved
        ));
    }

    // ── is_terminal_stage ─────────────────────────────────────────────────────

    #[test]
    fn is_terminal_stage_identifies_terminal() {
        let cfg = two_stage_pipeline(vec![]);
        assert!(is_terminal_stage(&cfg, "done"));
        assert!(!is_terminal_stage(&cfg, "implementation"));
        assert!(!is_terminal_stage(&cfg, "nonexistent"));
    }

    // ── evaluate_guard ────────────────────────────────────────────────────────

    #[test]
    fn null_guard_passes() {
        let ctx = default_ctx(agent_completed_event());
        assert!(evaluate_guard(&Value::Null, &ctx));
    }

    #[test]
    fn empty_object_guard_passes() {
        let ctx = default_ctx(agent_completed_event());
        assert!(evaluate_guard(&serde_json::json!({}), &ctx));
    }

    #[test]
    fn priority_guard_passes_when_matching() {
        let ctx = p0_ctx(agent_completed_event());
        assert!(evaluate_guard(&serde_json::json!({"priority": "p0"}), &ctx));
    }

    #[test]
    fn priority_guard_fails_when_not_matching() {
        let ctx = default_ctx(agent_completed_event()); // P2
        assert!(!evaluate_guard(&serde_json::json!({"priority": "p0"}), &ctx));
    }

    #[test]
    fn state_guard_passes_when_matching() {
        let ctx = default_ctx(agent_completed_event()); // InProgress
        assert!(evaluate_guard(
            &serde_json::json!({"state": "in_progress"}),
            &ctx
        ));
    }

    #[test]
    fn state_guard_fails_when_not_matching() {
        let ctx = default_ctx(agent_completed_event()); // InProgress
        assert!(!evaluate_guard(&serde_json::json!({"state": "pending"}), &ctx));
    }

    #[test]
    fn all_combinator_passes_when_all_sub_guards_pass() {
        let ctx = p0_ctx(agent_completed_event());
        let guard = serde_json::json!({
            "all": [
                {"priority": "p0"},
                {"state": "in_progress"}
            ]
        });
        assert!(evaluate_guard(&guard, &ctx));
    }

    #[test]
    fn all_combinator_fails_when_one_sub_guard_fails() {
        let ctx = p0_ctx(agent_completed_event());
        let guard = serde_json::json!({
            "all": [
                {"priority": "p0"},
                {"state": "pending"}  // fails
            ]
        });
        assert!(!evaluate_guard(&guard, &ctx));
    }

    #[test]
    fn any_combinator_passes_when_one_sub_guard_passes() {
        let ctx = default_ctx(agent_completed_event()); // P2, InProgress
        let guard = serde_json::json!({
            "any": [
                {"priority": "p0"},  // fails
                {"state": "in_progress"}  // passes
            ]
        });
        assert!(evaluate_guard(&guard, &ctx));
    }

    #[test]
    fn any_combinator_fails_when_all_sub_guards_fail() {
        let ctx = default_ctx(agent_completed_event()); // P2, InProgress
        let guard = serde_json::json!({
            "any": [
                {"priority": "p0"},  // fails
                {"state": "pending"}  // fails
            ]
        });
        assert!(!evaluate_guard(&guard, &ctx));
    }

    #[test]
    fn unknown_guard_key_passes_unconditionally() {
        let ctx = default_ctx(agent_completed_event());
        let guard = serde_json::json!({"unknown_field": "whatever"});
        assert!(evaluate_guard(&guard, &ctx));
    }

    // ── TransitionEngine::evaluate ────────────────────────────────────────────

    #[test]
    fn agent_completed_triggers_matching_rule_returns_move_to() {
        let cfg = two_stage_pipeline(vec![TransitionDefinition {
            when: TransitionTrigger::AgentCompleted,
            then: "review".into(),
            guard: None,
        }]);
        // Add a review stage so MoveTo is returned, not Complete
        let mut config = cfg;
        config.stages.insert(
            1,
            StageDefinition {
                name: "review".into(),
                label: None,
                instructions: None,
                instructions_template: None,
                requires_approval: false,
                approvers: vec![],
                timeout_seconds: None,
                terminal: false,
                hooks: vec![],
                transition_rules: vec![],
                color: None,
                order: 0,
                wip_limit: None,
            },
        );

        let event = agent_completed_event();
        let ctx = default_ctx(event.clone());
        let result = TransitionEngine::evaluate(&config.stages[0], &event, &ctx, &config);
        assert_eq!(
            result,
            TransitionResult::MoveTo {
                stage: "review".into()
            }
        );
    }

    #[test]
    fn guard_blocks_transition_when_condition_not_met() {
        let cfg = two_stage_pipeline(vec![TransitionDefinition {
            when: TransitionTrigger::AgentCompleted,
            then: "done".into(),
            guard: Some(serde_json::json!({"priority": "p0"})),
        }]);
        let event = agent_completed_event();
        let ctx = default_ctx(event.clone()); // P2 — guard should fail
        let result = TransitionEngine::evaluate(&cfg.stages[0], &event, &ctx, &cfg);
        assert_eq!(result, TransitionResult::NoMatch);
    }

    #[test]
    fn guard_passes_when_condition_met() {
        let cfg = two_stage_pipeline(vec![TransitionDefinition {
            when: TransitionTrigger::AgentCompleted,
            then: "done".into(),
            guard: Some(serde_json::json!({"priority": "p0"})),
        }]);
        let event = agent_completed_event();
        let ctx = p0_ctx(event.clone()); // P0 — guard should pass
        let result = TransitionEngine::evaluate(&cfg.stages[0], &event, &ctx, &cfg);
        assert_eq!(
            result,
            TransitionResult::Complete {
                outcome: TaskOutcome::Success
            }
        );
    }

    #[test]
    fn no_matching_rule_returns_no_match() {
        let cfg = two_stage_pipeline(vec![TransitionDefinition {
            when: TransitionTrigger::Approved,
            then: "done".into(),
            guard: None,
        }]);
        let event = agent_completed_event(); // Doesn't match Approved trigger
        let ctx = default_ctx(event.clone());
        let result = TransitionEngine::evaluate(&cfg.stages[0], &event, &ctx, &cfg);
        assert_eq!(result, TransitionResult::NoMatch);
    }

    #[test]
    fn transition_to_terminal_stage_returns_complete() {
        let cfg = two_stage_pipeline(vec![TransitionDefinition {
            when: TransitionTrigger::AgentCompleted,
            then: "done".into(),
            guard: None,
        }]);
        let event = agent_completed_event();
        let ctx = default_ctx(event.clone());
        let result = TransitionEngine::evaluate(&cfg.stages[0], &event, &ctx, &cfg);
        assert_eq!(
            result,
            TransitionResult::Complete {
                outcome: TaskOutcome::Success
            }
        );
    }

    #[test]
    fn stage_requiring_approval_returns_requires_approval() {
        let mut cfg = two_stage_pipeline(vec![TransitionDefinition {
            when: TransitionTrigger::AgentCompleted,
            then: "done".into(),
            guard: None,
        }]);
        cfg.stages[0].requires_approval = true;

        let event = agent_completed_event();
        let ctx = default_ctx(event.clone());
        let result = TransitionEngine::evaluate(&cfg.stages[0], &event, &ctx, &cfg);
        assert_eq!(result, TransitionResult::RequiresApproval);
    }

    // ── TransitionEngine::evaluate_pipeline ───────────────────────────────────

    #[test]
    fn pipeline_level_evaluation_finds_correct_stage() {
        let cfg = three_stage_pipeline();
        let event = agent_completed_event();
        let ctx = GuardContext {
            current_stage: "implementation".into(),
            task_state: TaskState::InProgress,
            priority: Priority::P2,
            event: event.clone(),
        };
        let result = TransitionEngine::evaluate_pipeline(&cfg, "implementation", &event, &ctx);
        assert_eq!(
            result,
            TransitionResult::MoveTo {
                stage: "review".into()
            }
        );
    }

    #[test]
    fn pipeline_level_approved_in_review_returns_complete() {
        let cfg = three_stage_pipeline();
        let event = human_approved_event();
        let ctx = GuardContext {
            current_stage: "review".into(),
            task_state: TaskState::AwaitingApproval {
                approvers: vec![],
                approved_by: vec![],
            },
            priority: Priority::P2,
            event: event.clone(),
        };
        let result = TransitionEngine::evaluate_pipeline(&cfg, "review", &event, &ctx);
        assert_eq!(
            result,
            TransitionResult::Complete {
                outcome: TaskOutcome::Success
            }
        );
    }

    #[test]
    fn unknown_stage_returns_no_match() {
        let cfg = three_stage_pipeline();
        let event = agent_completed_event();
        let ctx = GuardContext {
            current_stage: "nonexistent".into(),
            task_state: TaskState::InProgress,
            priority: Priority::P2,
            event: event.clone(),
        };
        let result = TransitionEngine::evaluate_pipeline(&cfg, "nonexistent", &event, &ctx);
        assert_eq!(result, TransitionResult::NoMatch);
    }

    #[test]
    fn no_matching_rule_in_pipeline_returns_no_match() {
        let cfg = three_stage_pipeline();
        // Send a rejected event to the implementation stage which has no rejected rule
        let event = human_rejected_event();
        let ctx = GuardContext {
            current_stage: "implementation".into(),
            task_state: TaskState::InProgress,
            priority: Priority::P2,
            event: event.clone(),
        };
        let result = TransitionEngine::evaluate_pipeline(&cfg, "implementation", &event, &ctx);
        assert_eq!(result, TransitionResult::NoMatch);
    }
}
