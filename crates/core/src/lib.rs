//! `molt-hub-core` — domain model, event sourcing, state machines, and configuration schema.
//!
//! This crate contains the foundational types and logic shared across the Molt Hub system.
//! It has no network or I/O dependencies; all persistence and transport concerns live in
//! upstream crates.

pub mod config;
pub mod events;
pub mod integrations;
pub mod machine;
pub mod model;
pub mod project;
pub mod summaries;
pub mod templates;
pub mod transitions;
