//! SQLite-backed persistence for agent output lines and steer messages.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::Row;

// ---------------------------------------------------------------------------
// Agent output
// ---------------------------------------------------------------------------

/// Persist a single output line for an agent.
pub async fn insert_output_line(
    pool: &sqlx::SqlitePool,
    agent_id: &str,
    task_id: Option<&str>,
    project_id: &str,
    line: &str,
    timestamp: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let ts = timestamp.to_rfc3339();
    sqlx::query(
        "INSERT INTO agent_output (agent_id, task_id, project_id, timestamp, line) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(agent_id)
    .bind(task_id)
    .bind(project_id)
    .bind(ts)
    .bind(line)
    .execute(pool)
    .await?;
    Ok(())
}

/// Retrieve output lines for an agent, oldest first.
///
/// `limit`: cap at this many rows (capped at 1000 if `None`).
pub async fn get_output_lines(
    pool: &sqlx::SqlitePool,
    agent_id: &str,
    limit: Option<i64>,
) -> Result<Vec<(DateTime<Utc>, String)>, sqlx::Error> {
    let cap = limit.unwrap_or(1000).min(1000);
    let rows = sqlx::query(
        "SELECT timestamp, line FROM agent_output \
         WHERE agent_id = ?1 \
         ORDER BY id ASC \
         LIMIT ?2",
    )
    .bind(agent_id)
    .bind(cap)
    .fetch_all(pool)
    .await?;

    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let ts_str: String = row.get("timestamp");
        let line: String = row.get("line");
        let ts = ts_str
            .parse::<DateTime<Utc>>()
            .unwrap_or_else(|_| Utc::now());
        result.push((ts, line));
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Steer messages
// ---------------------------------------------------------------------------

/// A single steer message row as returned from the DB.
#[derive(Debug, Clone, Serialize)]
pub struct SteerMessageRow {
    pub id: i64,
    pub agent_id: String,
    pub timestamp: String,
    pub role: String,
    pub content: String,
    pub priority: Option<String>,
}

/// Persist a steer message (human or agent).
pub async fn insert_steer_message(
    pool: &sqlx::SqlitePool,
    agent_id: &str,
    task_id: Option<&str>,
    project_id: &str,
    role: &str,
    content: &str,
    priority: Option<&str>,
) -> Result<(), sqlx::Error> {
    let ts = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO steer_messages \
         (agent_id, task_id, project_id, timestamp, role, content, priority) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .bind(agent_id)
    .bind(task_id)
    .bind(project_id)
    .bind(ts)
    .bind(role)
    .bind(content)
    .bind(priority)
    .execute(pool)
    .await?;
    Ok(())
}

/// Retrieve all steer messages for an agent, oldest first.
pub async fn get_steer_messages(
    pool: &sqlx::SqlitePool,
    agent_id: &str,
) -> Result<Vec<SteerMessageRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, agent_id, timestamp, role, content, priority \
         FROM steer_messages \
         WHERE agent_id = ?1 \
         ORDER BY id ASC",
    )
    .bind(agent_id)
    .fetch_all(pool)
    .await?;

    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        result.push(SteerMessageRow {
            id: row.get("id"),
            agent_id: row.get("agent_id"),
            timestamp: row.get("timestamp"),
            role: row.get("role"),
            content: row.get("content"),
            priority: row.get("priority"),
        });
    }
    Ok(result)
}
