//! Event store API — exposes task events and task listings.
//!
//! # Endpoints
//!
//! | Method | Path                          | Description                        |
//! |--------|-------------------------------|------------------------------------|
//! | GET    | `/api/events`                 | List events (filter by task/since) |
//! | GET    | `/api/events/:id`             | Get a single event by ID           |
//! | POST   | `/api/events`                 | Append a new event                 |
//! | GET    | `/api/tasks`                  | List tasks derived from events     |

pub mod handlers;

pub use handlers::{events_router, tasks_router, EventStoreState};
