//! Pipeline configuration — stage definitions stored per board.
//!
//! The production HTTP API is [`crate::projects::handlers`] under
//! `/api/projects/:pid/boards/:bid/stages`. [`handlers::pipeline_router`] is
//! retained for unit tests and ad-hoc mounting, not exposed from [`crate::serve::build_router`].

pub mod handlers;

pub use handlers::pipeline_router;
