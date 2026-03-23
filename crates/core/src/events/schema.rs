//! SQL schema definitions for the event store.

/// DDL for the primary events table.
///
/// Columns:
/// - `id`           — opaque text primary key (EventId)
/// - `task_id`      — the task this event belongs to
/// - `session_id`   — the session in which the event occurred
/// - `timestamp`    — ISO-8601 / RFC-3339 wall-clock time
/// - `caused_by`    — optional foreign-key-like link to a parent event
/// - `event_type`   — discriminator string extracted from the serde tag
/// - `payload`      — full JSON of the DomainEvent (including the `type` tag)
pub const CREATE_EVENTS_TABLE: &str = "
CREATE TABLE IF NOT EXISTS events (
    id         TEXT NOT NULL PRIMARY KEY,
    task_id    TEXT NOT NULL,
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
// SQLite tuning
// ---------------------------------------------------------------------------

/// Enable WAL mode for concurrent reads + single-writer throughput.
pub const ENABLE_WAL: &str = "PRAGMA journal_mode=WAL";

/// Relax fsync frequency; WAL mode makes this safe for non-critical data.
pub const SET_SYNCHRONOUS: &str = "PRAGMA synchronous=NORMAL";
