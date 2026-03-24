//! Project management API — CRUD for monitored codebases.
//!
//! # Endpoints
//!
//! | Method | Path                  | Description              |
//! |--------|-----------------------|--------------------------|
//! | GET    | `/api/projects`       | List all projects        |
//! | POST   | `/api/projects`       | Create a project         |
//! | GET    | `/api/projects/:id`   | Get project details      |
//! | PATCH  | `/api/projects/:id`   | Update project name      |
//! | DELETE | `/api/projects/:id`   | Archive (soft-delete)    |

pub mod handlers;
pub mod runtime;

pub use handlers::project_router;
pub use runtime::{BoardSummary, MultiBoardPipelineStore, ProjectRuntime, ProjectRuntimeRegistry};
