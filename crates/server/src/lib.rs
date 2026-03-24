//! `molt-hub-server` — Axum HTTP/WebSocket server, hook execution, task concurrency, and approvals.
//!
//! This crate wires the domain model from `molt-hub-core` to the outside world: HTTP endpoints,
//! WebSocket streams for the UI, and the hook infrastructure that coordinates agent processes.

pub mod actors;
pub mod agents;
pub mod approvals;
pub mod attention;
pub mod audit;
pub mod credentials;
pub mod events;
pub mod hooks;
pub mod integrations;
pub mod pipeline;
pub mod projects;
pub mod scheduler;
pub mod serve;
pub mod settings;
pub mod summarizer;
pub mod ws;
pub mod ws_broadcast;

mod env_bootstrap;
pub use env_bootstrap::load_dotenv_files;
