//! Shared router builder for the Molt Hub Axum server.
//!
//! Used by both the standalone CLI binary (`molt-hub serve`) and the Tauri desktop shell.

use std::path::PathBuf;
use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

use crate::ws::{ws_handler, ConnectionManager};

/// Build the Molt Hub Axum router with WebSocket and static file serving.
///
/// The returned router provides:
/// - `GET /ws` — WebSocket upgrade for real-time UI updates
/// - `/*`      — Static files from `dist_dir` with `index.html` fallback (SPA routing)
pub fn build_router(dist_dir: PathBuf) -> Router {
    let manager = Arc::new(ConnectionManager::new());
    let index_html = dist_dir.join("index.html");

    Router::new()
        .route("/ws", get(ws_handler))
        .fallback_service(ServeDir::new(dist_dir).fallback(ServeFile::new(index_html)))
        .with_state(manager)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_router_does_not_panic() {
        let _router = build_router(PathBuf::from("/tmp/nonexistent-dist"));
    }
}
