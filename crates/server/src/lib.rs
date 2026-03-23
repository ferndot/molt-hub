//! `molt-hub-server` — Axum HTTP/WebSocket server, hook execution, task concurrency, and approvals.
//!
//! This crate wires the domain model from `molt-hub-core` to the outside world: HTTP endpoints,
//! WebSocket streams for the UI, and the hook infrastructure that coordinates agent processes.

pub mod actors;
pub mod approvals;
pub mod attention;
pub mod hooks;
pub mod ws;
