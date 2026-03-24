//! EventStore trait and SQLite-backed implementation.

use chrono::{DateTime, Utc};
use serde::de::Error as _;
use sqlx::SqlitePool;
use thiserror::Error;
use tracing::instrument;

use crate::model::{EventId, SessionId, TaskId};

use super::schema::{
    apply_migrations, CREATE_EVENTS_TABLE, CREATE_IDX_EVENTS_CAUSED_BY,
    CREATE_IDX_EVENTS_PROJECT_ID, CREATE_IDX_EVENTS_TASK_ID, CREATE_IDX_EVENTS_TIMESTAMP,
    CREATE_TASK_CURRENT_STATE_TABLE, CREATE_TASK_TIMELINE_TABLE, ENABLE_WAL, SET_SYNCHRONOUS,
};
use super::types::EventEnvelope;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur when interacting with the event store.
#[derive(Debug, Error)]
pub enum EventStoreError {
    /// A SQLite / sqlx-level failure.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    /// JSON serialization or deserialization failed.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// The requested event does not exist in the store.
    #[error("event not found: {0}")]
    EventNotFound(EventId),
}

// ---------------------------------------------------------------------------
// EventStore trait
// ---------------------------------------------------------------------------

/// Append-only log of domain events.
///
/// Implementations must be `Send + Sync` so they can be shared across async
/// tasks without additional locking.
///
/// Uses native async-fn-in-trait (stable since Rust 1.75 / edition 2021).
pub trait EventStore: Send + Sync {
    /// Append a single event to the store.
    fn append(
        &self,
        envelope: EventEnvelope,
    ) -> impl std::future::Future<Output = Result<(), EventStoreError>> + Send;

    /// Append multiple events atomically in a single transaction.
    fn append_batch(
        &self,
        envelopes: Vec<EventEnvelope>,
    ) -> impl std::future::Future<Output = Result<(), EventStoreError>> + Send;

    /// Retrieve all events for a given task, ordered by timestamp ascending.
    fn get_events_for_task(
        &self,
        task_id: &TaskId,
    ) -> impl std::future::Future<Output = Result<Vec<EventEnvelope>, EventStoreError>> + Send;

    /// Retrieve all events recorded at or after `since`, ordered by timestamp ascending.
    fn get_events_since(
        &self,
        since: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<Vec<EventEnvelope>, EventStoreError>> + Send;

    /// Look up a single event by its unique identifier.
    fn get_event_by_id(
        &self,
        id: &EventId,
    ) -> impl std::future::Future<Output = Result<Option<EventEnvelope>, EventStoreError>> + Send;

    /// Walk the `caused_by` chain starting from `event_id` and return all
    /// ancestors (inclusive), ordered from root to the given event.
    fn get_causal_chain(
        &self,
        event_id: &EventId,
    ) -> impl std::future::Future<Output = Result<Vec<EventEnvelope>, EventStoreError>> + Send;

    /// Retrieve all events for a given project, ordered by timestamp ascending.
    fn get_events_for_project(
        &self,
        project_id: &str,
    ) -> impl std::future::Future<Output = Result<Vec<EventEnvelope>, EventStoreError>> + Send;
}

// ---------------------------------------------------------------------------
// Row → EventEnvelope helper
// ---------------------------------------------------------------------------

fn parse_ulid(s: &str) -> Result<ulid::Ulid, EventStoreError> {
    use std::str::FromStr;
    ulid::Ulid::from_str(s)
        .map_err(|e| serde_json::Error::custom(format!("invalid ULID '{s}': {e}")).into())
}

fn row_to_envelope(
    id: String,
    task_id: Option<String>,
    project_id: Option<String>,
    session_id: String,
    timestamp: String,
    caused_by: Option<String>,
    payload: String,
) -> Result<EventEnvelope, EventStoreError> {
    let payload_val = serde_json::from_str(&payload)?;
    let ts = timestamp
        .parse::<DateTime<Utc>>()
        .map_err(|e| serde_json::Error::custom(e.to_string()))?;
    let cb = caused_by
        .map(|s| parse_ulid(&s).map(EventId))
        .transpose()?;
    let tid = task_id
        .filter(|s| !s.is_empty())
        .map(|s| parse_ulid(&s).map(TaskId))
        .transpose()?;
    let pid = project_id.unwrap_or_else(|| "default".to_owned());

    Ok(EventEnvelope {
        id: EventId(parse_ulid(&id)?),
        task_id: tid,
        project_id: pid,
        session_id: SessionId(parse_ulid(&session_id)?),
        timestamp: ts,
        caused_by: cb,
        payload: payload_val,
    })
}

// ---------------------------------------------------------------------------
// SqliteEventStore
// ---------------------------------------------------------------------------

/// SQLite-backed event store using WAL mode for concurrent read performance.
pub struct SqliteEventStore {
    pool: SqlitePool,
}

impl SqliteEventStore {
    /// Create a new store backed by `pool`, running schema initialisation
    /// before returning.
    pub async fn new(pool: SqlitePool) -> Result<Self, EventStoreError> {
        Self::initialize(&pool).await?;
        Ok(Self { pool })
    }

    /// Create tables, indexes, and apply WAL / synchronous pragmas.
    async fn initialize(pool: &SqlitePool) -> Result<(), EventStoreError> {
        let mut conn = pool.acquire().await?;

        // Tuning pragmas — must run before DDL so WAL is active from the start.
        sqlx::query(ENABLE_WAL).execute(&mut *conn).await?;
        sqlx::query(SET_SYNCHRONOUS).execute(&mut *conn).await?;

        // Tables
        sqlx::query(CREATE_EVENTS_TABLE).execute(&mut *conn).await?;
        sqlx::query(CREATE_TASK_CURRENT_STATE_TABLE)
            .execute(&mut *conn)
            .await?;
        sqlx::query(CREATE_TASK_TIMELINE_TABLE)
            .execute(&mut *conn)
            .await?;

        // Indexes
        sqlx::query(CREATE_IDX_EVENTS_TASK_ID)
            .execute(&mut *conn)
            .await?;
        sqlx::query(CREATE_IDX_EVENTS_TIMESTAMP)
            .execute(&mut *conn)
            .await?;
        sqlx::query(CREATE_IDX_EVENTS_CAUSED_BY)
            .execute(&mut *conn)
            .await?;
        sqlx::query(CREATE_IDX_EVENTS_PROJECT_ID)
            .execute(&mut *conn)
            .await?;

        // Run schema migrations (idempotent, safe on every startup).
        apply_migrations(&mut *conn).await?;

        Ok(())
    }

    /// Serialise an envelope to the column values expected by the schema.
    /// Returns: (id, task_id, project_id, session_id, timestamp, caused_by, event_type, payload)
    fn to_row(
        envelope: &EventEnvelope,
    ) -> Result<(String, Option<String>, String, String, String, Option<String>, String, String), EventStoreError>
    {
        let payload_json = serde_json::to_string(&envelope.payload)?;
        let event_type = {
            // Parse once to get the type tag cleanly.
            let v: serde_json::Value = serde_json::from_str(&payload_json)?;
            v.get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("unknown")
                .to_owned()
        };
        let timestamp = envelope.timestamp.to_rfc3339();
        let caused_by = envelope.caused_by.as_ref().map(|e| e.0.to_string());
        let task_id = envelope.task_id.as_ref().map(|t| t.0.to_string());

        Ok((
            envelope.id.0.to_string(),
            task_id,
            envelope.project_id.clone(),
            envelope.session_id.0.to_string(),
            timestamp,
            caused_by,
            event_type,
            payload_json,
        ))
    }
}

impl EventStore for SqliteEventStore {
    #[instrument(skip(self, envelope), fields(event_id = %envelope.id.0))]
    async fn append(&self, envelope: EventEnvelope) -> Result<(), EventStoreError> {
        let (id, task_id, project_id, session_id, timestamp, caused_by, event_type, payload) =
            Self::to_row(&envelope)?;

        sqlx::query(
            "INSERT INTO events (id, task_id, project_id, session_id, timestamp, caused_by, event_type, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(id)
        .bind(task_id)
        .bind(project_id)
        .bind(session_id)
        .bind(timestamp)
        .bind(caused_by)
        .bind(event_type)
        .bind(payload)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    #[instrument(skip(self, envelopes), fields(count = envelopes.len()))]
    async fn append_batch(&self, envelopes: Vec<EventEnvelope>) -> Result<(), EventStoreError> {
        if envelopes.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

        for envelope in &envelopes {
            let (id, task_id, project_id, session_id, timestamp, caused_by, event_type, payload) =
                Self::to_row(envelope)?;

            sqlx::query(
                "INSERT INTO events (id, task_id, project_id, session_id, timestamp, caused_by, event_type, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            .bind(id)
            .bind(task_id)
            .bind(project_id)
            .bind(session_id)
            .bind(timestamp)
            .bind(caused_by)
            .bind(event_type)
            .bind(payload)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    #[instrument(skip(self), fields(task_id = %task_id.0))]
    async fn get_events_for_task(
        &self,
        task_id: &TaskId,
    ) -> Result<Vec<EventEnvelope>, EventStoreError> {
        let id_str = task_id.0.to_string();
        let rows = sqlx::query(
            "SELECT id, task_id, COALESCE(project_id, 'default') as project_id, session_id, timestamp, caused_by, payload
             FROM events
             WHERE task_id = ?1
             ORDER BY timestamp ASC",
        )
        .bind(id_str)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|r| {
                use sqlx::Row;
                row_to_envelope(
                    r.get("id"),
                    r.try_get("task_id").ok(),
                    r.try_get("project_id").ok(),
                    r.get("session_id"),
                    r.get("timestamp"),
                    r.get("caused_by"),
                    r.get("payload"),
                )
            })
            .collect()
    }

    #[instrument(skip(self))]
    async fn get_events_since(
        &self,
        since: DateTime<Utc>,
    ) -> Result<Vec<EventEnvelope>, EventStoreError> {
        let since_str = since.to_rfc3339();
        let rows = sqlx::query(
            "SELECT id, task_id, COALESCE(project_id, 'default') as project_id, session_id, timestamp, caused_by, payload
             FROM events
             WHERE timestamp >= ?1
             ORDER BY timestamp ASC",
        )
        .bind(since_str)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|r| {
                use sqlx::Row;
                row_to_envelope(
                    r.get("id"),
                    r.try_get("task_id").ok(),
                    r.try_get("project_id").ok(),
                    r.get("session_id"),
                    r.get("timestamp"),
                    r.get("caused_by"),
                    r.get("payload"),
                )
            })
            .collect()
    }

    #[instrument(skip(self), fields(event_id = %id.0))]
    async fn get_event_by_id(
        &self,
        id: &EventId,
    ) -> Result<Option<EventEnvelope>, EventStoreError> {
        let id_str = id.0.to_string();
        let row = sqlx::query(
            "SELECT id, task_id, COALESCE(project_id, 'default') as project_id, session_id, timestamp, caused_by, payload
             FROM events
             WHERE id = ?1",
        )
        .bind(id_str)
        .fetch_optional(&self.pool)
        .await?;

        row.map(|r| {
            use sqlx::Row;
            row_to_envelope(
                r.get("id"),
                r.try_get("task_id").ok(),
                r.try_get("project_id").ok(),
                r.get("session_id"),
                r.get("timestamp"),
                r.get("caused_by"),
                r.get("payload"),
            )
        })
        .transpose()
    }

    #[instrument(skip(self), fields(root_event_id = %event_id.0))]
    async fn get_causal_chain(
        &self,
        event_id: &EventId,
    ) -> Result<Vec<EventEnvelope>, EventStoreError> {
        // Walk the caused_by chain from the given event back to the root,
        // collecting in reverse order, then reverse at the end so the result
        // is root-first.
        let mut chain: Vec<EventEnvelope> = Vec::new();
        let mut current_id = event_id.clone();

        loop {
            let envelope = self
                .get_event_by_id(&current_id)
                .await?
                .ok_or_else(|| EventStoreError::EventNotFound(current_id.clone()))?;

            let next = envelope.caused_by.clone();
            chain.push(envelope);

            match next {
                Some(parent_id) => current_id = parent_id,
                None => break,
            }
        }

        chain.reverse();
        Ok(chain)
    }

    #[instrument(skip(self), fields(project_id = %project_id))]
    async fn get_events_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<EventEnvelope>, EventStoreError> {
        let rows = sqlx::query(
            "SELECT id, task_id, COALESCE(project_id, 'default') as project_id, session_id, timestamp, caused_by, payload
             FROM events
             WHERE COALESCE(project_id, 'default') = ?1
             ORDER BY timestamp ASC",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|r| {
                use sqlx::Row;
                row_to_envelope(
                    r.get("id"),
                    r.try_get("task_id").ok(),
                    r.try_get("project_id").ok(),
                    r.get("session_id"),
                    r.get("timestamp"),
                    r.get("caused_by"),
                    r.get("payload"),
                )
            })
            .collect()
    }
}
