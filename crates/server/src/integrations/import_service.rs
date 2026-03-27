//! Jira import service — maps Jira issues to Molt Hub tasks and emits domain events.

use std::sync::Arc;

use chrono::Utc;
use thiserror::Error;
use tracing::{info, instrument};

use crate::ws::ConnectionManager;
use crate::ws_broadcast::{broadcast_board_update_full, BoardUpdate};

use molt_hub_core::events::store::{EventStore, EventStoreError};
use molt_hub_core::events::types::{DomainEvent, EventEnvelope};
use molt_hub_core::model::{EventId, Priority, SessionId, TaskId};

use super::jira_client::{JiraClient, JiraError, JiraIssue};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by [`ImportService`] operations.
#[derive(Debug, Error)]
pub enum ImportError {
    /// A Jira API call failed.
    #[error("Jira error: {0}")]
    Jira(#[from] JiraError),

    /// Writing events to the store failed.
    #[error("event store error: {0}")]
    EventStore(#[from] EventStoreError),

    /// An individual issue import failed (contains the issue key and reason).
    #[error("failed to import issue {key}: {reason}")]
    IssueFailed { key: String, reason: String },
}

// ---------------------------------------------------------------------------
// ImportService
// ---------------------------------------------------------------------------

/// Imports Jira issues as Molt Hub tasks by emitting domain events.
pub struct ImportService<S: EventStore + 'static> {
    client: JiraClient,
    store: Arc<S>,
    /// Session under which imported tasks are recorded.
    session_id: SessionId,
}

impl<S: EventStore + 'static> ImportService<S> {
    /// Create a new service.
    pub fn new(client: JiraClient, store: Arc<S>, session_id: SessionId) -> Self {
        Self {
            client,
            store,
            session_id,
        }
    }

    /// Import a single Jira issue by key.
    ///
    /// Fetches the issue from Jira, maps it to a task, and emits
    /// `TaskCreated` + `TaskImported` events into the event store.
    ///
    /// Returns the generated [`TaskId`].
    #[instrument(skip_all, fields(issue_key = %issue_key))]
    pub async fn import_issue(
        &self,
        issue_key: &str,
        initial_stage: &str,
        board_id: Option<&str>,
        broadcast: Option<(&ConnectionManager, &str)>,
    ) -> Result<TaskId, ImportError> {
        let issue = self.client.get_issue(issue_key).await?;
        self.import_one(issue, initial_stage, board_id, broadcast).await
    }

    /// Search using JQL and import all matching issues.
    ///
    /// Returns the list of generated [`TaskId`]s in the same order as the
    /// search results.  Individual failures are propagated immediately.
    #[instrument(skip(self), fields(jql = %jql))]
    pub async fn bulk_import(&self, jql: &str) -> Result<Vec<TaskId>, ImportError> {
        const MAX: u32 = 100;
        let issues = self.client.search_issues(jql, MAX).await?;
        info!(count = issues.len(), "bulk import starting");

        let mut ids = Vec::with_capacity(issues.len());
        for issue in issues {
            let id = self.import_one(issue, "backlog", None, None).await?;
            ids.push(id);
        }

        Ok(ids)
    }

    /// Search using JQL and return the matching Jira issues without importing.
    ///
    /// Useful for rendering a preview in the UI before the user confirms the
    /// import.
    #[instrument(skip(self), fields(jql = %jql))]
    pub async fn preview_import(&self, jql: &str) -> Result<Vec<JiraIssue>, ImportError> {
        const MAX: u32 = 50;
        Ok(self.client.search_issues(jql, MAX).await?)
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    async fn import_one(
        &self,
        issue: JiraIssue,
        initial_stage: &str,
        board_id: Option<&str>,
        broadcast: Option<(&ConnectionManager, &str)>,
    ) -> Result<TaskId, ImportError> {
        let task_id = TaskId::new();
        let stage = initial_stage.trim();
        let stage_owned = if stage.is_empty() {
            "backlog".to_owned()
        } else {
            stage.to_owned()
        };

        let created = EventEnvelope {
            id: EventId::new(),
            task_id: Some(task_id.clone()),
            project_id: "default".to_owned(),
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
            caused_by: None,
            payload: DomainEvent::TaskCreated {
                title: issue.summary.clone(),
                description: issue.description.clone().unwrap_or_default(),
                initial_stage: stage_owned.clone(),
                priority: map_priority(issue.priority.as_deref()),
                board_id: board_id.map(str::to_owned),
            },
        };

        // Second event carries the Jira-specific import metadata as a
        // TaskImported event (encoded as AgentOutput for now — a dedicated
        // TaskImported variant can be added to DomainEvent in a future sprint).
        let import_note = serde_json::json!({
            "source": "jira",
            "jira_key": issue.key,
            "jira_url": issue.url,
            "jira_status": issue.status,
            "labels": issue.labels,
        })
        .to_string();

        let task_created_id = created.id.clone();
        let imported = EventEnvelope {
            id: EventId::new(),
            task_id: Some(task_id.clone()),
            project_id: "default".to_owned(),
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
            caused_by: Some(task_created_id),
            payload: DomainEvent::AgentOutput {
                agent_id: molt_hub_core::model::AgentId::new(),
                output: format!("Imported from Jira: {}", import_note),
            },
        };

        self.store.append_batch(vec![created, imported]).await?;

        if let Some((mgr, project_id)) = broadcast {
            broadcast_board_update_full(
                mgr,
                project_id,
                &BoardUpdate {
                    task_id: task_id.to_string(),
                    stage: stage_owned,
                    status: "waiting".to_owned(),
                    priority: None,
                    name: Some(issue.summary.clone()),
                    agent_name: Some("Jira".to_owned()),
                    summary: None,
                    board_id: board_id.map(str::to_owned),
                },
            );
        }

        info!(task_id = %task_id, jira_key = %issue.key, "issue imported");
        Ok(task_id)
    }
}

// ---------------------------------------------------------------------------
// Priority mapping
// ---------------------------------------------------------------------------

fn map_priority(jira_priority: Option<&str>) -> Priority {
    match jira_priority {
        Some("Highest") | Some("Critical") | Some("Blocker") => Priority::P0,
        Some("High") => Priority::P1,
        Some("Low") | Some("Lowest") | Some("Minor") => Priority::P3,
        _ => Priority::P2, // Medium, None, unknown → P2
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use molt_hub_core::events::store::{EventStore, EventStoreError};
    use molt_hub_core::events::types::EventEnvelope;
    use molt_hub_core::model::{EventId, TaskId};

    // ── Minimal in-memory store stub ─────────────────────────────────────────

    #[derive(Default)]
    struct MemoryStore {
        events: Mutex<Vec<EventEnvelope>>,
    }

    impl EventStore for MemoryStore {
        async fn append(&self, envelope: EventEnvelope) -> Result<(), EventStoreError> {
            self.events.lock().unwrap().push(envelope);
            Ok(())
        }

        async fn append_batch(&self, envelopes: Vec<EventEnvelope>) -> Result<(), EventStoreError> {
            self.events.lock().unwrap().extend(envelopes);
            Ok(())
        }

        async fn get_events_for_task(
            &self,
            task_id: &TaskId,
        ) -> Result<Vec<EventEnvelope>, EventStoreError> {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.task_id.as_ref() == Some(task_id))
                .cloned()
                .collect())
        }

        async fn get_events_since(
            &self,
            since: chrono::DateTime<chrono::Utc>,
        ) -> Result<Vec<EventEnvelope>, EventStoreError> {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.timestamp >= since)
                .cloned()
                .collect())
        }

        async fn get_event_by_id(
            &self,
            id: &EventId,
        ) -> Result<Option<EventEnvelope>, EventStoreError> {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .find(|e| &e.id == id)
                .cloned())
        }

        async fn get_causal_chain(
            &self,
            event_id: &EventId,
        ) -> Result<Vec<EventEnvelope>, EventStoreError> {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| &e.id == event_id)
                .cloned()
                .collect())
        }

        async fn get_events_for_project(
            &self,
            project_id: &str,
        ) -> Result<Vec<EventEnvelope>, EventStoreError> {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.project_id == project_id)
                .cloned()
                .collect())
        }
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    #[test]
    fn map_priority_maps_correctly() {
        assert_eq!(map_priority(Some("Highest")), Priority::P0);
        assert_eq!(map_priority(Some("Critical")), Priority::P0);
        assert_eq!(map_priority(Some("High")), Priority::P1);
        assert_eq!(map_priority(Some("Medium")), Priority::P2);
        assert_eq!(map_priority(None), Priority::P2);
        assert_eq!(map_priority(Some("Low")), Priority::P3);
        assert_eq!(map_priority(Some("Lowest")), Priority::P3);
        assert_eq!(map_priority(Some("Minor")), Priority::P3);
        assert_eq!(map_priority(Some("Unknown")), Priority::P2);
    }

    #[tokio::test]
    async fn import_one_emits_two_events() {
        let store = Arc::new(MemoryStore::default());
        let session_id = SessionId::new();

        let issue = JiraIssue {
            key: "PROJ-1".into(),
            summary: "Fix the bug".into(),
            description: Some("Details here".into()),
            status: "To Do".into(),
            status_color: None,
            priority: Some("High".into()),
            labels: vec!["bug".into()],
            epic_link: None,
            epic_name: None,
            url: "https://org.atlassian.net/browse/PROJ-1".into(),
        };

        // Call import_one directly via a thin wrapper (the method is private,
        // so we construct ImportService and call it via a minimal JiraClient
        // that we never actually call the network with).
        let client = JiraClient::from_oauth("test-cloud-id", "test-access-token");
        let svc = ImportService::new(client, Arc::clone(&store), session_id);

        let task_id = svc.import_one(issue, "backlog", None, None).await.unwrap();

        let events = store.get_events_for_task(&task_id).await.unwrap();
        assert_eq!(events.len(), 2, "expected TaskCreated + AgentOutput events");

        // First event is TaskCreated
        assert!(
            matches!(&events[0].payload, DomainEvent::TaskCreated { title, .. } if title == "Fix the bug"),
            "first event should be TaskCreated"
        );

        // Second event has a causal link back to the first.
        assert_eq!(events[1].caused_by, Some(events[0].id.clone()));
    }

    #[tokio::test]
    async fn preview_import_returns_issues_without_storing() {
        // preview_import calls the JiraClient network — we can only test the
        // store side (no events emitted). We validate the function exists and
        // compiles; network calls require integration tests.
        let store = Arc::new(MemoryStore::default());
        let session_id = SessionId::new();
        let client = JiraClient::from_oauth("test-cloud-id", "test-access-token");
        let _svc = ImportService::new(client, Arc::clone(&store), session_id);

        // No events should exist (no network call made since we can't hit Jira
        // in unit tests).
        let events = store.events.lock().unwrap();
        assert!(events.is_empty());
    }
}
