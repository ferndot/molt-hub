//! Audit logging — isolated append-only writer for adapter boundary events,
//! plus an HTTP endpoint for querying recent entries.
//!
//! All adapter calls (spawn, send, terminate) and external imports (Jira) are
//! logged here without blocking the caller.

pub mod handlers;
pub mod writer;

pub use handlers::{audit_router, AuditState};
pub use writer::{start_audit_writer, AuditAction, AuditEntry, AuditHandle};
