//! `molt-hub-harness` — agent adapter trait, process supervisor, and git worktree lifecycle.
//!
//! This crate provides the runtime substrate for running agents: it abstracts over different
//! agent backends (ACP-compatible agents), manages OS-level processes, and handles the
//! git worktree isolation model used for concurrent agent tasks.

pub mod acp;
pub mod adapter;
pub mod health;
pub mod supervisor;
pub mod worktree;
