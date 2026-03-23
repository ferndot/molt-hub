//! Pipeline configuration API — exposes stage definitions to the UI.
//!
//! # Endpoints
//!
//! | Method | Path                      | Description                               |
//! |--------|---------------------------|-------------------------------------------|
//! | GET    | `/api/pipeline/stages`    | Return configured pipeline stages as JSON |

pub mod handlers;

pub use handlers::pipeline_router;
