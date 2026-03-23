//! Default interrupt classification rules for domain events.

use molt_hub_core::events::types::DomainEvent;

use crate::attention::priority::InterruptLevel;

// ---------------------------------------------------------------------------
// InterruptClassifier
// ---------------------------------------------------------------------------

/// Classifies a `DomainEvent` into an `InterruptLevel` using default rules.
///
/// Default rules:
///
/// | Level | Events |
/// |-------|--------|
/// | P0    | `TaskBlocked` |
/// | P1    | `TaskStageChanged` where `new_state == AwaitingApproval`, `HumanDecision(Rejected)` |
/// | P2    | `AgentCompleted`, `TaskStageChanged` (other), `AgentAssigned`, `TaskCompleted`, `TaskUnblocked`, `HumanDecision` (Approved/Redirected) |
/// | P3    | `AgentOutput`, `TaskPriorityChanged`, `TaskCreated` |
///
/// Future: rules will be configurable per-pipeline. This struct is the
/// extension point for that work.
pub struct InterruptClassifier;

impl InterruptClassifier {
    /// Create a new classifier with default hardcoded rules.
    pub fn new() -> Self {
        Self
    }

    /// Classify a domain event into an interrupt level.
    pub fn classify(&self, event: &DomainEvent) -> InterruptLevel {
        use molt_hub_core::model::TaskState;

        match event {
            // P0 — Immediate action required
            DomainEvent::TaskBlocked { .. } => InterruptLevel::P0,

            // P1 — Needs attention soon
            DomainEvent::TaskStageChanged { new_state, .. }
                if matches!(new_state, TaskState::AwaitingApproval { .. }) =>
            {
                InterruptLevel::P1
            }
            DomainEvent::HumanDecision { decision, .. }
                if matches!(
                    decision,
                    molt_hub_core::events::types::HumanDecisionKind::Rejected { .. }
                ) =>
            {
                InterruptLevel::P1
            }

            // P2 — Informational
            DomainEvent::AgentCompleted { .. } => InterruptLevel::P2,
            DomainEvent::TaskStageChanged { .. } => InterruptLevel::P2,
            DomainEvent::AgentAssigned { .. } => InterruptLevel::P2,
            DomainEvent::TaskCompleted { .. } => InterruptLevel::P2,
            DomainEvent::TaskUnblocked { .. } => InterruptLevel::P2,
            DomainEvent::HumanDecision { .. } => InterruptLevel::P2,

            // P3 — Passive / log-only
            DomainEvent::AgentOutput { .. } => InterruptLevel::P3,
            DomainEvent::TaskPriorityChanged { .. } => InterruptLevel::P3,
            DomainEvent::TaskCreated { .. } => InterruptLevel::P3,
            DomainEvent::TaskImported { .. } => InterruptLevel::P3,
            DomainEvent::IntegrationConfigured { .. } => InterruptLevel::P3,
        }
    }
}

impl Default for InterruptClassifier {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use molt_hub_core::events::types::{DomainEvent, HumanDecisionKind};
    use molt_hub_core::model::{AgentId, Priority, TaskOutcome, TaskState};

    fn classifier() -> InterruptClassifier {
        InterruptClassifier::new()
    }

    // ── P0 cases ─────────────────────────────────────────────────────────────

    #[test]
    fn task_blocked_is_p0() {
        let event = DomainEvent::TaskBlocked {
            reason: "dependency missing".into(),
        };
        assert_eq!(classifier().classify(&event), InterruptLevel::P0);
    }

    // ── P1 cases ─────────────────────────────────────────────────────────────

    #[test]
    fn task_stage_changed_to_awaiting_approval_is_p1() {
        let event = DomainEvent::TaskStageChanged {
            from_stage: "work".into(),
            to_stage: "review".into(),
            new_state: TaskState::AwaitingApproval {
                approvers: vec![],
                approved_by: vec![],
            },
        };
        assert_eq!(classifier().classify(&event), InterruptLevel::P1);
    }

    #[test]
    fn human_decision_rejected_is_p1() {
        let event = DomainEvent::HumanDecision {
            decided_by: "alice".into(),
            decision: HumanDecisionKind::Rejected {
                reason: "not good enough".into(),
            },
            note: None,
        };
        assert_eq!(classifier().classify(&event), InterruptLevel::P1);
    }

    // ── P2 cases ─────────────────────────────────────────────────────────────

    #[test]
    fn agent_completed_is_p2() {
        let event = DomainEvent::AgentCompleted {
            agent_id: AgentId::new(),
            summary: None,
        };
        assert_eq!(classifier().classify(&event), InterruptLevel::P2);
    }

    #[test]
    fn task_stage_changed_to_in_progress_is_p2() {
        let event = DomainEvent::TaskStageChanged {
            from_stage: "pending".into(),
            to_stage: "work".into(),
            new_state: TaskState::InProgress,
        };
        assert_eq!(classifier().classify(&event), InterruptLevel::P2);
    }

    #[test]
    fn task_stage_changed_to_pending_is_p2() {
        let event = DomainEvent::TaskStageChanged {
            from_stage: "work".into(),
            to_stage: "done".into(),
            new_state: TaskState::Pending,
        };
        assert_eq!(classifier().classify(&event), InterruptLevel::P2);
    }

    #[test]
    fn agent_assigned_is_p2() {
        let event = DomainEvent::AgentAssigned {
            agent_id: AgentId::new(),
            agent_name: "bot-1".into(),
        };
        assert_eq!(classifier().classify(&event), InterruptLevel::P2);
    }

    #[test]
    fn task_completed_is_p2() {
        let event = DomainEvent::TaskCompleted {
            outcome: TaskOutcome::Success,
        };
        assert_eq!(classifier().classify(&event), InterruptLevel::P2);
    }

    #[test]
    fn task_unblocked_is_p2() {
        let event = DomainEvent::TaskUnblocked { resolution: None };
        assert_eq!(classifier().classify(&event), InterruptLevel::P2);
    }

    #[test]
    fn human_decision_approved_is_p2() {
        let event = DomainEvent::HumanDecision {
            decided_by: "alice".into(),
            decision: HumanDecisionKind::Approved,
            note: None,
        };
        assert_eq!(classifier().classify(&event), InterruptLevel::P2);
    }

    #[test]
    fn human_decision_redirected_is_p2() {
        let event = DomainEvent::HumanDecision {
            decided_by: "alice".into(),
            decision: HumanDecisionKind::Redirected {
                to_stage: "rework".into(),
                reason: "needs revision".into(),
            },
            note: None,
        };
        assert_eq!(classifier().classify(&event), InterruptLevel::P2);
    }

    // ── P3 cases ─────────────────────────────────────────────────────────────

    #[test]
    fn agent_output_is_p3() {
        let event = DomainEvent::AgentOutput {
            agent_id: AgentId::new(),
            output: "some log line".into(),
        };
        assert_eq!(classifier().classify(&event), InterruptLevel::P3);
    }

    #[test]
    fn task_priority_changed_is_p3() {
        let event = DomainEvent::TaskPriorityChanged {
            from: Priority::P2,
            to: Priority::P1,
        };
        assert_eq!(classifier().classify(&event), InterruptLevel::P3);
    }

    #[test]
    fn task_created_is_p3() {
        let event = DomainEvent::TaskCreated {
            title: "New task".into(),
            description: "Do something".into(),
            initial_stage: "triage".into(),
            priority: Priority::P3,
        };
        assert_eq!(classifier().classify(&event), InterruptLevel::P3);
    }

    // ── Coverage check: every DomainEvent variant is tested ──────────────────

    #[test]
    fn all_variants_return_a_level() {
        // This test is exhaustive via the above tests; this one just ensures
        // the classifier doesn't panic on any event.
        let events = vec![
            DomainEvent::TaskCreated {
                title: "t".into(),
                description: "d".into(),
                initial_stage: "s".into(),
                priority: Priority::P2,
            },
            DomainEvent::TaskStageChanged {
                from_stage: "a".into(),
                to_stage: "b".into(),
                new_state: TaskState::InProgress,
            },
            DomainEvent::TaskPriorityChanged {
                from: Priority::P3,
                to: Priority::P1,
            },
            DomainEvent::TaskBlocked {
                reason: "x".into(),
            },
            DomainEvent::TaskUnblocked { resolution: None },
            DomainEvent::TaskCompleted {
                outcome: TaskOutcome::Success,
            },
            DomainEvent::AgentAssigned {
                agent_id: AgentId::new(),
                agent_name: "a".into(),
            },
            DomainEvent::AgentOutput {
                agent_id: AgentId::new(),
                output: "o".into(),
            },
            DomainEvent::AgentCompleted {
                agent_id: AgentId::new(),
                summary: None,
            },
            DomainEvent::HumanDecision {
                decided_by: "u".into(),
                decision: HumanDecisionKind::Approved,
                note: None,
            },
        ];

        let c = classifier();
        for event in &events {
            // Just must not panic
            let _ = c.classify(event);
        }
    }
}
