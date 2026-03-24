//! Pipeline configuration API — exposes stage definitions to the UI.
//!
//! # Endpoints
//!
//! | Method | Path                        | Description                               |
//! |--------|-----------------------------|-------------------------------------------|
//! | GET    | `/api/pipeline/stages`      | Return configured pipeline stages as JSON |
//! | PUT    | `/api/pipeline/stages`      | Replace all stages                        |
//! | POST   | `/api/pipeline/stages`      | Add a single new stage                    |
//! | PATCH  | `/api/pipeline/stages/:id`  | Partially update a single stage           |
//! | DELETE | `/api/pipeline/stages/:id`  | Remove a stage by ID                      |

pub mod handlers;

pub use handlers::pipeline_router;
