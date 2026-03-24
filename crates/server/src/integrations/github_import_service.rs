//! GitHub issue import — maps GitHub issues to tasks and persists domain events.

use std::collections::HashSet;
use std::sync::Arc;

use chrono::Utc;
use thiserror::Error;
use tracing::{info, instrument};

use molt_hub_core::events::store::{EventStore, EventStoreError};
use molt_hub_core::events::types::{DomainEvent, EventEnvelope};
use molt_hub_core::model::{EventId, Priority, SessionId, TaskId};

use super::github_client::{GitHubClient, GitHubError, GitHubIssue};

const DEFAULT_PROJECT_ID: &str = "default";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from GitHub import operations.
#[derive(Debug, Error)]
pub enum GithubImportError {
    #[error("GitHub API error: {0}")]
    GitHub(#[from] GitHubError),

    #[error("event store error: {0}")]
    EventStore(#[from] EventStoreError),
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// Imports GitHub issues as tasks via [`EventStore`], mirroring the Jira import flow
/// (`TaskCreated` plus integration metadata).
pub struct GithubImportService<S: EventStore + 'static> {
    client: GitHubClient,
    store: Arc<S>,
    session_id: SessionId,
}

impl<S: EventStore + 'static> GithubImportService<S> {
    pub fn new(client: GitHubClient, store: Arc<S>, session_id: SessionId) -> Self {
        Self {
            client,
            store,
            session_id,
        }
    }

    /// Import issue numbers in order. Skips issues already recorded via [`github_external_id`].
    ///
    /// Returns `(new_imports, skipped_duplicates)`.
    #[instrument(skip(self), fields(owner = %owner, repo = %repo, count = issue_numbers.len()))]
    pub async fn import_issues(
        &self,
        owner: &str,
        repo: &str,
        issue_numbers: &[i64],
    ) -> Result<(usize, usize), GithubImportError> {
        let mut seen = load_github_imported_external_ids(self.store.as_ref()).await?;

        let mut new_count = 0usize;
        let mut skipped = 0usize;

        for &number in issue_numbers {
            let ext_id = github_external_id(owner, repo, number);
            if seen.contains(&ext_id) {
                skipped += 1;
                continue;
            }

            let issue = self.client.get_issue(owner, repo, number).await?;
            self.persist_new_issue(owner, repo, &issue, &ext_id).await?;
            seen.insert(ext_id);
            new_count += 1;
        }

        info!(new_count, skipped, "GitHub import batch complete");
        Ok((new_count, skipped))
    }

    async fn persist_new_issue(
        &self,
        _owner: &str,
        _repo: &str,
        issue: &GitHubIssue,
        external_id: &str,
    ) -> Result<(), GithubImportError> {
        let task_id = TaskId::new();

        let created = EventEnvelope {
            id: EventId::new(),
            task_id: Some(task_id.clone()),
            project_id: DEFAULT_PROJECT_ID.to_owned(),
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
            caused_by: None,
            payload: DomainEvent::TaskCreated {
                title: issue.title.clone(),
                description: issue.body.clone().unwrap_or_default(),
                initial_stage: "triage".to_owned(),
                priority: github_priority(issue),
            },
        };

        let task_created_id = created.id.clone();
        let imported = EventEnvelope {
            id: EventId::new(),
            task_id: Some(task_id.clone()),
            project_id: DEFAULT_PROJECT_ID.to_owned(),
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
            caused_by: Some(task_created_id),
            payload: DomainEvent::TaskImported {
                source: "github".to_owned(),
                external_id: external_id.to_owned(),
                external_url: issue.html_url.clone(),
            },
        };

        self.store.append_batch(vec![created, imported]).await?;

        info!(task_id = %task_id, %external_id, "GitHub issue imported");
        Ok(())
    }
}

/// Stable id for idempotency (matches `owner/repo#number`).
pub fn github_external_id(owner: &str, repo: &str, number: i64) -> String {
    format!("{owner}/{repo}#{number}")
}

async fn load_github_imported_external_ids<S: EventStore>(
    store: &S,
) -> Result<HashSet<String>, EventStoreError> {
    let events = store.get_events_for_project(DEFAULT_PROJECT_ID).await?;
    let mut set = HashSet::new();
    for e in events {
        if let DomainEvent::TaskImported {
            source,
            external_id,
            ..
        } = e.payload
        {
            if source == "github" {
                set.insert(external_id);
            }
        }
    }
    Ok(set)
}

fn github_priority(issue: &GitHubIssue) -> Priority {
    let labels: Vec<&str> = issue.labels.iter().map(|l| l.name.as_str()).collect();
    if labels.iter().any(|l| {
        matches!(
            *l,
            "priority:critical" | "priority:blocker" | "P0" | "p0" | "critical" | "blocker"
        )
    }) {
        return Priority::P0;
    }
    if labels
        .iter()
        .any(|l| matches!(*l, "priority:high" | "P1" | "p1" | "high"))
    {
        return Priority::P1;
    }
    if labels
        .iter()
        .any(|l| matches!(*l, "priority:low" | "P3" | "p3" | "low"))
    {
        return Priority::P3;
    }
    Priority::P2
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use molt_hub_core::events::types::EventEnvelope;
    use molt_hub_core::model::EventId;

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

    fn sample_issue(number: i64) -> GitHubIssue {
        GitHubIssue {
            number,
            title: "Fix it".into(),
            state: "open".into(),
            body: Some("Body".into()),
            html_url: format!("https://github.com/o/r/issues/{number}"),
            labels: vec![super::super::github_client::GitHubLabel { name: "bug".into() }],
        }
    }

    #[test]
    fn github_external_id_format() {
        assert_eq!(github_external_id("acme", "app", 7), "acme/app#7");
    }

    #[tokio::test]
    async fn persist_new_issue_writes_two_events() {
        let store = Arc::new(MemoryStore::default());
        let session_id = SessionId::new();
        let client = GitHubClient::new("unused".into());
        let svc = GithubImportService::new(client, Arc::clone(&store), session_id);

        let issue = sample_issue(42);
        let ext = github_external_id("o", "r", 42);
        svc.persist_new_issue("o", "r", &issue, &ext).await.unwrap();

        let all = store.get_events_for_project("default").await.unwrap();
        assert_eq!(all.len(), 2);

        assert!(matches!(
            &all[0].payload,
            DomainEvent::TaskCreated { title, .. } if title == "Fix it"
        ));
        assert!(matches!(
            &all[1].payload,
            DomainEvent::TaskImported {
                source,
                external_id,
                ..
            } if source == "github" && external_id == "o/r#42"
        ));
        assert_eq!(all[1].caused_by, Some(all[0].id.clone()));
    }

    #[tokio::test]
    async fn import_skips_when_task_imported_exists() {
        let store = Arc::new(MemoryStore::default());
        let session_id = SessionId::new();

        let seed = EventEnvelope {
            id: EventId::new(),
            task_id: Some(TaskId::new()),
            project_id: "default".into(),
            session_id: session_id.clone(),
            timestamp: Utc::now(),
            caused_by: None,
            payload: DomainEvent::TaskImported {
                source: "github".into(),
                external_id: "o/r#1".into(),
                external_url: "https://github.com/o/r/issues/1".into(),
            },
        };
        store.append(seed).await.unwrap();

        let client = GitHubClient::new("unused".into());
        let svc = GithubImportService::new(client, Arc::clone(&store), session_id);

        let (new_c, skip_c) = svc.import_issues("o", "r", &[1]).await.unwrap();
        assert_eq!(new_c, 0);
        assert_eq!(skip_c, 1);

        let count = store.get_events_for_project("default").await.unwrap().len();
        assert_eq!(count, 1);
    }

    #[test]
    fn github_priority_from_labels() {
        let mut issue = sample_issue(1);
        issue.labels = vec![super::super::github_client::GitHubLabel { name: "P1".into() }];
        assert_eq!(github_priority(&issue), Priority::P1);
    }
}
