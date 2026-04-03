//! SQL schema definitions and migration runner for the event store.

/// DDL for the primary events table.
///
/// Columns:
/// - `id`           — opaque text primary key (EventId)
/// - `task_id`      — the task this event belongs to (nullable for project-level events)
/// - `project_id`   — the project this event belongs to (defaults to "default")
/// - `session_id`   — the session in which the event occurred
/// - `timestamp`    — ISO-8601 / RFC-3339 wall-clock time
/// - `caused_by`    — optional foreign-key-like link to a parent event
/// - `event_type`   — discriminator string extracted from the serde tag
/// - `payload`      — full JSON of the DomainEvent (including the `type` tag)
pub const CREATE_EVENTS_TABLE: &str = "
CREATE TABLE IF NOT EXISTS events (
    id         TEXT NOT NULL PRIMARY KEY,
    task_id    TEXT,
    project_id TEXT NOT NULL DEFAULT 'default',
    session_id TEXT NOT NULL,
    timestamp  TEXT NOT NULL,
    caused_by  TEXT,
    event_type TEXT NOT NULL,
    payload    TEXT NOT NULL
)";

/// Index on `task_id` for efficient per-task event queries.
pub const CREATE_IDX_EVENTS_TASK_ID: &str = "
CREATE INDEX IF NOT EXISTS idx_events_task_id ON events (task_id)";

/// Index on `timestamp` for time-range queries.
pub const CREATE_IDX_EVENTS_TIMESTAMP: &str = "
CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events (timestamp)";

/// Index on `caused_by` for causal-chain traversal.
pub const CREATE_IDX_EVENTS_CAUSED_BY: &str = "
CREATE INDEX IF NOT EXISTS idx_events_caused_by ON events (caused_by)";

/// Index on `project_id` for efficient per-project event queries.
pub const CREATE_IDX_EVENTS_PROJECT_ID: &str = "
CREATE INDEX IF NOT EXISTS idx_events_project_id ON events (project_id)";

// ---------------------------------------------------------------------------
// Projection tables (read side)
// ---------------------------------------------------------------------------

/// Projection: current state of each task (one row per task).
pub const CREATE_TASK_CURRENT_STATE_TABLE: &str = "
CREATE TABLE IF NOT EXISTS task_current_state (
    task_id         TEXT NOT NULL PRIMARY KEY,
    title           TEXT NOT NULL,
    current_stage   TEXT NOT NULL,
    state           TEXT NOT NULL,
    priority        TEXT NOT NULL,
    last_event_id   TEXT NOT NULL,
    updated_at      TEXT NOT NULL
)";

/// Projection: ordered timeline of stage transitions for each task.
pub const CREATE_TASK_TIMELINE_TABLE: &str = "
CREATE TABLE IF NOT EXISTS task_timeline (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id     TEXT NOT NULL,
    event_id    TEXT NOT NULL,
    from_stage  TEXT,
    to_stage    TEXT NOT NULL,
    state       TEXT NOT NULL,
    occurred_at TEXT NOT NULL
)";

// ---------------------------------------------------------------------------
// Agent output and steer message tables
// ---------------------------------------------------------------------------

/// Persistent ring buffer for agent terminal output lines.
pub const CREATE_AGENT_OUTPUT_TABLE: &str = "
CREATE TABLE IF NOT EXISTS agent_output (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    agent_id   TEXT NOT NULL,
    task_id    TEXT,
    project_id TEXT NOT NULL DEFAULT 'default',
    timestamp  TEXT NOT NULL,
    line       TEXT NOT NULL
)";

/// Index on `agent_id` for efficient per-agent output queries.
pub const CREATE_IDX_AGENT_OUTPUT_AGENT_ID: &str = "
CREATE INDEX IF NOT EXISTS idx_agent_output_agent_id ON agent_output (agent_id)";

/// Persistent store for steering/chat messages sent to agents.
pub const CREATE_STEER_MESSAGES_TABLE: &str = "
CREATE TABLE IF NOT EXISTS steer_messages (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    agent_id   TEXT NOT NULL,
    task_id    TEXT,
    project_id TEXT NOT NULL DEFAULT 'default',
    timestamp  TEXT NOT NULL,
    role       TEXT NOT NULL,
    content    TEXT NOT NULL,
    priority   TEXT
)";

/// Index on `agent_id` for efficient per-agent steer message queries.
pub const CREATE_IDX_STEER_MESSAGES_AGENT_ID: &str = "
CREATE INDEX IF NOT EXISTS idx_steer_messages_agent_id ON steer_messages (agent_id)";

// ---------------------------------------------------------------------------
// Schema version tracking
// ---------------------------------------------------------------------------

/// DDL for the schema version tracking table.
pub const CREATE_SCHEMA_VERSION_TABLE: &str = "
CREATE TABLE IF NOT EXISTS schema_version (
    version    INTEGER NOT NULL,
    applied_at TEXT NOT NULL
)";

// ---------------------------------------------------------------------------
// SQLite tuning
// ---------------------------------------------------------------------------

/// Enable WAL mode for concurrent reads + single-writer throughput.
pub const ENABLE_WAL: &str = "PRAGMA journal_mode=WAL";

/// Relax fsync frequency; WAL mode makes this safe for non-critical data.
pub const SET_SYNCHRONOUS: &str = "PRAGMA synchronous=NORMAL";

// ---------------------------------------------------------------------------
// Migration runner
// ---------------------------------------------------------------------------

/// Apply all pending schema migrations idempotently.
///
/// Each migration is gated on a version check against `schema_version` so it
/// is safe to call on every startup.
pub async fn apply_migrations(conn: &mut sqlx::SqliteConnection) -> Result<(), sqlx::Error> {
    // Ensure schema_version table exists.
    sqlx::query(CREATE_SCHEMA_VERSION_TABLE)
        .execute(&mut *conn)
        .await?;

    // --- Migration 1: add project_id column to events ---
    // SQLite does not support ALTER TABLE ... ADD COLUMN IF NOT EXISTS,
    // so we inspect PRAGMA table_info(events) first.
    let already_applied = migration_applied(conn, 1).await?;
    if !already_applied {
        // Check whether the column already exists (e.g. fresh DB with new DDL).
        let has_column = column_exists(conn, "events", "project_id").await?;
        if !has_column {
            sqlx::query("ALTER TABLE events ADD COLUMN project_id TEXT")
                .execute(&mut *conn)
                .await?;
            sqlx::query("UPDATE events SET project_id = 'default' WHERE project_id IS NULL")
                .execute(&mut *conn)
                .await?;
        }
        record_migration(conn, 1).await?;
    }

    // --- Migration 2: create agent_output and steer_messages tables ---
    let already_applied = migration_applied(conn, 2).await?;
    if !already_applied {
        sqlx::query(CREATE_AGENT_OUTPUT_TABLE)
            .execute(&mut *conn)
            .await?;
        sqlx::query(CREATE_IDX_AGENT_OUTPUT_AGENT_ID)
            .execute(&mut *conn)
            .await?;
        sqlx::query(CREATE_STEER_MESSAGES_TABLE)
            .execute(&mut *conn)
            .await?;
        sqlx::query(CREATE_IDX_STEER_MESSAGES_AGENT_ID)
            .execute(&mut *conn)
            .await?;
        record_migration(conn, 2).await?;
    }

    Ok(())
}

/// Return true if the given migration version has already been recorded.
async fn migration_applied(
    conn: &mut sqlx::SqliteConnection,
    version: i64,
) -> Result<bool, sqlx::Error> {
    use sqlx::Row;
    let row = sqlx::query("SELECT COUNT(*) as cnt FROM schema_version WHERE version = ?1")
        .bind(version)
        .fetch_one(&mut *conn)
        .await?;
    let cnt: i64 = row.get("cnt");
    Ok(cnt > 0)
}

/// Return true if `table` has a column named `column`.
async fn column_exists(
    conn: &mut sqlx::SqliteConnection,
    table: &str,
    column: &str,
) -> Result<bool, sqlx::Error> {
    let rows = sqlx::query("SELECT name FROM pragma_table_info(?1) WHERE name = ?2")
        .bind(table)
        .bind(column)
        .fetch_all(&mut *conn)
        .await?;
    Ok(!rows.is_empty())
}

/// Record a migration version as applied (with current UTC timestamp).
async fn record_migration(
    conn: &mut sqlx::SqliteConnection,
    version: i64,
) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO schema_version (version, applied_at) VALUES (?1, ?2)")
        .bind(version)
        .bind(now)
        .execute(&mut *conn)
        .await?;
    Ok(())
}
