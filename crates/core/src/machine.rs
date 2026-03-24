//! State machines — task lifecycle and agent workflow state transitions.

use chrono::Utc;
use thiserror::Error;

use crate::events::{DomainEvent, HumanDecisionKind};
use crate::model::{TaskOutcome, TaskState};

// ---------------------------------------------------------------------------
// TransitionError
// ---------------------------------------------------------------------------

/// Errors that can occur when applying a domain event to a task state machine.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransitionError {
    /// The event is not valid from the current state.
    #[error("invalid transition from '{from}' on event '{event}'")]
    InvalidTransition { from: String, event: String },

    /// A guard condition blocked the transition.
    #[error("guard '{guard}' failed: {reason}")]
    GuardFailed { guard: String, reason: String },

    /// The transition requires human approval before it can proceed.
    #[error("approval required before this transition")]
    ApprovalRequired,

    /// The task is in a terminal state and cannot be transitioned further.
    #[error("task is in a terminal state and cannot be transitioned")]
    TerminalState,
}

// ---------------------------------------------------------------------------
// TaskMachine
// ---------------------------------------------------------------------------

/// Drives a single task through its lifecycle state machine.
///
/// `TaskMachine` owns the current `TaskState` and the name of the stage the
/// task occupies. It validates and applies `DomainEvent`s, returning the new
/// state on success or a `TransitionError` on failure.
pub struct TaskMachine {
    pub state: TaskState,
    pub current_stage: String,
}

impl TaskMachine {
    /// Create a new machine in the `Pending` state at the given stage.
    pub fn new(initial_stage: String) -> Self {
        Self {
            state: TaskState::Pending,
            current_stage: initial_stage,
        }
    }

    /// Returns `true` if the task is in a terminal state (`Completed` or `Failed`).
    pub fn is_terminal(&self) -> bool {
        matches!(self.state, TaskState::Completed { .. } | TaskState::Failed { .. })
    }

    /// Returns `true` if the machine can accept the given event without erroring.
    ///
    /// This is a read-only, side-effect-free check. It does not account for
    /// the `requires_approval` flag — use `apply_with_approval_flag` for that.
    pub fn can_accept(&self, event: &DomainEvent) -> bool {
        if self.is_terminal() {
            return false;
        }
        match (&self.state, event) {
            // Passthrough events valid from any non-terminal state
            (_, DomainEvent::TaskPriorityChanged { .. }) => true,
            (_, DomainEvent::AgentOutput { .. }) => true,

            // State-specific events
            (TaskState::Pending, DomainEvent::AgentAssigned { .. }) => true,
            (TaskState::InProgress, DomainEvent::TaskBlocked { .. }) => true,
            (TaskState::InProgress, DomainEvent::AgentCompleted { .. }) => true,
            (TaskState::InProgress, DomainEvent::TaskStageChanged { .. }) => true,
            (TaskState::Blocked { .. }, DomainEvent::TaskUnblocked { .. }) => true,
            (TaskState::AwaitingApproval { .. }, DomainEvent::HumanDecision { .. }) => true,

            _ => false,
        }
    }

    /// Apply a domain event, assuming `requires_approval = false`.
    ///
    /// When the current stage requires approval, use `apply_with_approval_flag`
    /// instead so the machine can route `AgentCompleted` correctly.
    pub fn apply(&mut self, event: &DomainEvent) -> Result<TaskState, TransitionError> {
        self.apply_with_approval_flag(event, false)
    }

    /// Apply a domain event with an explicit approval requirement flag.
    ///
    /// `requires_approval` controls whether an `AgentCompleted` event routes
    /// the task to `AwaitingApproval` (true) or directly to `Completed(Success)`
    /// (false).
    pub fn apply_with_approval_flag(
        &mut self,
        event: &DomainEvent,
        requires_approval: bool,
    ) -> Result<TaskState, TransitionError> {
        // Terminal state guard — no transitions allowed once completed or failed.
        if self.is_terminal() {
            return Err(TransitionError::TerminalState);
        }

        let event_name = event_name(event);

        let new_state = match (&self.state, event) {
            // ------------------------------------------------------------------
            // Passthrough events — valid from any non-terminal state
            // ------------------------------------------------------------------
            (_, DomainEvent::TaskPriorityChanged { .. }) => self.state.clone(),
            (_, DomainEvent::AgentOutput { .. }) => self.state.clone(),

            // ------------------------------------------------------------------
            // Pending → InProgress
            // ------------------------------------------------------------------
            (TaskState::Pending, DomainEvent::AgentAssigned { .. }) => TaskState::InProgress,

            // ------------------------------------------------------------------
            // InProgress → Blocked
            // ------------------------------------------------------------------
            (TaskState::InProgress, DomainEvent::TaskBlocked { reason }) => TaskState::Blocked {
                reason: reason.clone(),
                blocked_at: Utc::now(),
            },

            // ------------------------------------------------------------------
            // InProgress → AwaitingApproval | Completed(Success)
            // ------------------------------------------------------------------
            (TaskState::InProgress, DomainEvent::AgentCompleted { .. }) => {
                if requires_approval {
                    TaskState::AwaitingApproval {
                        approvers: vec![],
                        approved_by: vec![],
                    }
                } else {
                    TaskState::Completed {
                        outcome: TaskOutcome::Success,
                    }
                }
            }

            // ------------------------------------------------------------------
            // InProgress → InProgress (stage change)
            // ------------------------------------------------------------------
            (TaskState::InProgress, DomainEvent::TaskStageChanged { to_stage, .. }) => {
                self.current_stage = to_stage.clone();
                TaskState::InProgress
            }

            // ------------------------------------------------------------------
            // Blocked → InProgress
            // ------------------------------------------------------------------
            (TaskState::Blocked { .. }, DomainEvent::TaskUnblocked { .. }) => TaskState::InProgress,

            // ------------------------------------------------------------------
            // AwaitingApproval → Completed | InProgress
            // ------------------------------------------------------------------
            (
                TaskState::AwaitingApproval { .. },
                DomainEvent::HumanDecision { decision, .. },
            ) => match decision {
                HumanDecisionKind::Approved => TaskState::Completed {
                    outcome: TaskOutcome::Success,
                },
                HumanDecisionKind::Rejected { .. } => TaskState::InProgress,
                HumanDecisionKind::Redirected { to_stage, .. } => {
                    self.current_stage = to_stage.clone();
                    TaskState::InProgress
                }
            },

            // ------------------------------------------------------------------
            // Any other combination is invalid
            // ------------------------------------------------------------------
            _ => {
                return Err(TransitionError::InvalidTransition {
                    from: state_name(&self.state).to_string(),
                    event: event_name.to_string(),
                });
            }
        };

        self.state = new_state.clone();
        Ok(new_state)
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn state_name(state: &TaskState) -> &'static str {
    match state {
        TaskState::Pending => "Pending",
        TaskState::InProgress => "InProgress",
        TaskState::Blocked { .. } => "Blocked",
        TaskState::AwaitingApproval { .. } => "AwaitingApproval",
        TaskState::Completed { .. } => "Completed",
        TaskState::Failed { .. } => "Failed",
    }
}

fn event_name(event: &DomainEvent) -> &'static str {
    match event {
        DomainEvent::TaskCreated { .. } => "TaskCreated",
        DomainEvent::TaskStageChanged { .. } => "TaskStageChanged",
        DomainEvent::TaskPriorityChanged { .. } => "TaskPriorityChanged",
        DomainEvent::TaskBlocked { .. } => "TaskBlocked",
        DomainEvent::TaskUnblocked { .. } => "TaskUnblocked",
        DomainEvent::TaskCompleted { .. } => "TaskCompleted",
        DomainEvent::AgentAssigned { .. } => "AgentAssigned",
        DomainEvent::AgentOutput { .. } => "AgentOutput",
        DomainEvent::AgentCompleted { .. } => "AgentCompleted",
        DomainEvent::HumanDecision { .. } => "HumanDecision",
        DomainEvent::TaskImported { .. } => "TaskImported",
        DomainEvent::IntegrationConfigured { .. } => "IntegrationConfigured",
        DomainEvent::ProjectCreated { .. } => "ProjectCreated",
        DomainEvent::ProjectArchived { .. } => "ProjectArchived",
        DomainEvent::ProjectUpdated { .. } => "ProjectUpdated",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::HumanDecisionKind;
    use crate::model::{AgentId, Priority};

    fn agent_assigned() -> DomainEvent {
        DomainEvent::AgentAssigned {
            agent_id: AgentId::new(),
            agent_name: "test-agent".into(),
        }
    }

    fn agent_completed() -> DomainEvent {
        DomainEvent::AgentCompleted {
            agent_id: AgentId::new(),
            summary: None,
        }
    }

    fn task_blocked() -> DomainEvent {
        DomainEvent::TaskBlocked {
            reason: "waiting for dependency".into(),
        }
    }

    fn task_unblocked() -> DomainEvent {
        DomainEvent::TaskUnblocked { resolution: None }
    }

    fn human_approved() -> DomainEvent {
        DomainEvent::HumanDecision {
            decided_by: "alice".into(),
            decision: HumanDecisionKind::Approved,
            note: None,
        }
    }

    fn human_rejected() -> DomainEvent {
        DomainEvent::HumanDecision {
            decided_by: "alice".into(),
            decision: HumanDecisionKind::Rejected {
                reason: "not good enough".into(),
            },
            note: None,
        }
    }

    fn human_redirected(to_stage: &str) -> DomainEvent {
        DomainEvent::HumanDecision {
            decided_by: "alice".into(),
            decision: HumanDecisionKind::Redirected {
                to_stage: to_stage.into(),
                reason: "needs rework".into(),
            },
            note: None,
        }
    }

    fn priority_changed() -> DomainEvent {
        DomainEvent::TaskPriorityChanged {
            from: Priority::P2,
            to: Priority::P1,
        }
    }

    // --- Construction ---

    #[test]
    fn new_machine_is_pending() {
        let m = TaskMachine::new("planning".into());
        assert_eq!(m.state, TaskState::Pending);
        assert_eq!(m.current_stage, "planning");
        assert!(!m.is_terminal());
    }

    // --- Pending → InProgress ---

    #[test]
    fn pending_agent_assigned_yields_in_progress() {
        let mut m = TaskMachine::new("planning".into());
        let result = m.apply(&agent_assigned()).unwrap();
        assert_eq!(result, TaskState::InProgress);
    }

    // --- InProgress → Blocked ---

    #[test]
    fn in_progress_blocked_yields_blocked() {
        let mut m = TaskMachine::new("impl".into());
        m.apply(&agent_assigned()).unwrap();
        let result = m.apply(&task_blocked()).unwrap();
        assert!(matches!(result, TaskState::Blocked { .. }));
    }

    // --- Blocked → InProgress ---

    #[test]
    fn blocked_unblocked_yields_in_progress() {
        let mut m = TaskMachine::new("impl".into());
        m.apply(&agent_assigned()).unwrap();
        m.apply(&task_blocked()).unwrap();
        let result = m.apply(&task_unblocked()).unwrap();
        assert_eq!(result, TaskState::InProgress);
    }

    // --- InProgress → Completed (no approval) ---

    #[test]
    fn agent_completed_without_approval_yields_success() {
        let mut m = TaskMachine::new("impl".into());
        m.apply(&agent_assigned()).unwrap();
        let result = m.apply_with_approval_flag(&agent_completed(), false).unwrap();
        assert_eq!(
            result,
            TaskState::Completed {
                outcome: TaskOutcome::Success
            }
        );
    }

    // --- InProgress → AwaitingApproval (requires_approval) ---

    #[test]
    fn agent_completed_with_approval_yields_awaiting_approval() {
        let mut m = TaskMachine::new("review".into());
        m.apply(&agent_assigned()).unwrap();
        let result = m.apply_with_approval_flag(&agent_completed(), true).unwrap();
        assert!(matches!(result, TaskState::AwaitingApproval { .. }));
    }

    // --- AwaitingApproval → Completed(Success) ---

    #[test]
    fn awaiting_approval_approved_yields_completed_success() {
        let mut m = TaskMachine::new("review".into());
        m.apply(&agent_assigned()).unwrap();
        m.apply_with_approval_flag(&agent_completed(), true).unwrap();
        let result = m.apply(&human_approved()).unwrap();
        assert_eq!(
            result,
            TaskState::Completed {
                outcome: TaskOutcome::Success
            }
        );
        assert!(m.is_terminal());
    }

    // --- AwaitingApproval → InProgress (Rejected) ---

    #[test]
    fn awaiting_approval_rejected_yields_in_progress() {
        let mut m = TaskMachine::new("review".into());
        m.apply(&agent_assigned()).unwrap();
        m.apply_with_approval_flag(&agent_completed(), true).unwrap();
        let result = m.apply(&human_rejected()).unwrap();
        assert!(matches!(result, TaskState::InProgress));
        assert!(!m.is_terminal());
    }

    // --- AwaitingApproval → InProgress (Redirected) ---

    #[test]
    fn awaiting_approval_redirected_yields_in_progress_with_new_stage() {
        let mut m = TaskMachine::new("review".into());
        m.apply(&agent_assigned()).unwrap();
        m.apply_with_approval_flag(&agent_completed(), true).unwrap();
        let result = m.apply(&human_redirected("impl")).unwrap();
        assert_eq!(result, TaskState::InProgress);
        assert_eq!(m.current_stage, "impl");
    }

    // --- Stage change while InProgress ---

    #[test]
    fn in_progress_stage_changed_updates_current_stage() {
        let mut m = TaskMachine::new("planning".into());
        m.apply(&agent_assigned()).unwrap();
        let event = DomainEvent::TaskStageChanged {
            from_stage: "planning".into(),
            to_stage: "implementation".into(),
            new_state: TaskState::InProgress,
        };
        let result = m.apply(&event).unwrap();
        assert_eq!(result, TaskState::InProgress);
        assert_eq!(m.current_stage, "implementation");
    }

    // --- Priority changed passthrough ---

    #[test]
    fn priority_changed_is_passthrough_from_any_state() {
        // Pending
        let mut m = TaskMachine::new("planning".into());
        let result = m.apply(&priority_changed()).unwrap();
        assert_eq!(result, TaskState::Pending);

        // InProgress
        m.apply(&agent_assigned()).unwrap();
        let result = m.apply(&priority_changed()).unwrap();
        assert_eq!(result, TaskState::InProgress);
    }

    // --- AgentOutput passthrough ---

    #[test]
    fn agent_output_is_passthrough() {
        let mut m = TaskMachine::new("impl".into());
        m.apply(&agent_assigned()).unwrap();
        let event = DomainEvent::AgentOutput {
            agent_id: AgentId::new(),
            output: "some log line".into(),
        };
        let result = m.apply(&event).unwrap();
        assert_eq!(result, TaskState::InProgress);
    }

    // --- Terminal state guard ---

    #[test]
    fn terminal_state_rejects_any_event() {
        let mut m = TaskMachine::new("impl".into());
        m.apply(&agent_assigned()).unwrap();
        m.apply_with_approval_flag(&agent_completed(), false).unwrap();
        assert!(m.is_terminal());

        let err = m.apply(&agent_assigned()).unwrap_err();
        assert_eq!(err, TransitionError::TerminalState);
    }

    // --- Invalid transition ---

    #[test]
    fn pending_blocked_is_invalid() {
        let mut m = TaskMachine::new("planning".into());
        let err = m.apply(&task_blocked()).unwrap_err();
        assert!(matches!(err, TransitionError::InvalidTransition { .. }));
    }

    // --- can_accept ---

    #[test]
    fn can_accept_reflects_valid_transitions() {
        let m = TaskMachine::new("planning".into());
        assert!(m.can_accept(&agent_assigned()));
        assert!(!m.can_accept(&task_blocked()));
        assert!(m.can_accept(&priority_changed()));
    }

    #[test]
    fn can_accept_returns_false_for_terminal() {
        let mut m = TaskMachine::new("impl".into());
        m.apply(&agent_assigned()).unwrap();
        m.apply_with_approval_flag(&agent_completed(), false).unwrap();
        assert!(!m.can_accept(&priority_changed()));
    }
}
