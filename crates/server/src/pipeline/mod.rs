//! Pipeline configuration — stage definitions stored per board.
//!
//! The production HTTP API is [`crate::boards::handlers`] under `/api/boards/:bid/stages`.
//! [`handlers::pipeline_router`] is retained for unit tests and ad-hoc mounting.

pub mod handlers;
pub mod pipeline_config_store;

pub use handlers::pipeline_router;
pub use pipeline_config_store::PipelineConfigSqliteStore;
