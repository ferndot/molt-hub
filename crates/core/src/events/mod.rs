//! Event sourcing layer — types, store trait, SQLite implementation, and schema.

pub mod schema;
pub mod store;
pub mod types;

// Re-export the most commonly needed items at the module root.
pub use schema::{
    CREATE_EVENTS_TABLE, CREATE_TASK_CURRENT_STATE_TABLE, CREATE_TASK_TIMELINE_TABLE,
    ENABLE_WAL, SET_SYNCHRONOUS,
};
pub use store::{EventStore, EventStoreError, SqliteEventStore};
pub use types::{DomainEvent, EventEnvelope, HumanDecisionKind};
