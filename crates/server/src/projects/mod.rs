//! Project runtime — board management, pipeline stores, and runtime registry.

pub mod boards_store;
pub mod handlers;
pub mod runtime;

pub use boards_store::{BoardRecord, BoardsStore};
pub use runtime::{BoardSummary, MultiBoardPipelineStore, ProjectRuntime, ProjectRuntimeRegistry};
