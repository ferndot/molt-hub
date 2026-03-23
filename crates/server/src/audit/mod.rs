//! Audit logging — isolated append-only writer for adapter boundary events.
//!
//! All adapter calls (spawn, send, terminate) and external imports (Jira) are
//! logged here without blocking the caller.

pub mod writer;

pub use writer::{start_audit_writer, AuditAction, AuditEntry, AuditHandle};
