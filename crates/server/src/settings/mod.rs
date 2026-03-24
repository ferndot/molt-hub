//! User settings persistence — SQLite-backed key/value store with REST API,
//! plus a typed JSON-file-backed settings model.
//!
//! # Endpoints
//!
//! | Method | Path                         | Description                          |
//! |--------|------------------------------|--------------------------------------|
//! | GET    | `/api/settings`              | Return current typed settings as JSON |
//! | PUT    | `/api/settings`              | Full-replace typed settings           |
//! | PATCH  | `/api/settings/:section`     | Update a single section               |
//! | GET    | `/api/settings/kv`           | Return all KV settings                |
//! | PUT    | `/api/settings/kv`           | Upsert KV settings                    |
//! | DELETE | `/api/settings/kv/:key`      | Delete a specific KV key              |

pub mod file_store;
pub mod handlers;
pub mod model;
pub mod store;
pub mod typed_handlers;

pub use file_store::SettingsFileStore;
pub use handlers::{settings_router, SettingsState};
pub use model::ServerSettings;
pub use store::SettingsStore;
pub use typed_handlers::{typed_settings_router, TypedSettingsState};
