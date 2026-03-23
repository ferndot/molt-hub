//! User settings persistence — SQLite-backed key/value store with REST API.
//!
//! The settings model is intentionally schema-free: the server stores arbitrary
//! string values under string keys.  The frontend uses a single key
//! (`app_settings`) whose value is a JSON-serialised settings object.  This
//! keeps the server decoupled from UI schema changes.
//!
//! # Endpoints
//!
//! | Method | Path                    | Description                          |
//! |--------|-------------------------|--------------------------------------|
//! | GET    | `/api/settings`         | Return all settings as a JSON object |
//! | PUT    | `/api/settings`         | Upsert settings from a JSON object   |
//! | DELETE | `/api/settings/:key`    | Delete a specific setting key        |

pub mod handlers;
pub mod store;

pub use handlers::{settings_router, SettingsState};
pub use store::SettingsStore;
