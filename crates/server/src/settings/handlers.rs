//! Axum HTTP handlers for the settings API.
//!
//! Routes:
//!   GET    /api/settings        — return all settings as a flat JSON object
//!   PUT    /api/settings        — upsert all settings from request body
//!   DELETE /api/settings/:key   — delete a specific setting key

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use tracing::instrument;

use super::store::SettingsStore;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// State shared across all settings handlers.
pub struct SettingsState {
    pub store: Arc<SettingsStore>,
}

// ---------------------------------------------------------------------------
// Handler: GET /api/settings
// ---------------------------------------------------------------------------

/// Return all settings as a flat JSON object: `{ "key": "value", ... }`.
#[instrument(skip_all)]
pub async fn get_settings(State(state): State<Arc<SettingsState>>) -> impl IntoResponse {
    match state.store.get_all().await {
        Ok(map) => Json(map).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Handler: PUT /api/settings
// ---------------------------------------------------------------------------

/// Upsert settings from the request body.
///
/// Body must be a flat JSON object: `{ "app_settings": "{...json...}" }`.
/// All entries are upserted atomically.
#[instrument(skip_all)]
pub async fn put_settings(
    State(state): State<Arc<SettingsState>>,
    Json(body): Json<HashMap<String, String>>,
) -> impl IntoResponse {
    match state.store.set_bulk(&body).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Handler: DELETE /api/settings/:key
// ---------------------------------------------------------------------------

/// Delete the setting entry identified by `key`.
///
/// Responds with 204 whether or not the key existed (idempotent).
#[instrument(skip_all, fields(%key))]
pub async fn delete_setting(
    State(state): State<Arc<SettingsState>>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    match state.store.delete(&key).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Router builder
// ---------------------------------------------------------------------------

use axum::{
    routing::{delete, get, put},
    Router,
};

/// Build the `/api/settings` sub-router.
pub fn settings_router(state: Arc<SettingsState>) -> Router {
    Router::new()
        .route("/", get(get_settings))
        .route("/", put(put_settings))
        .route("/:key", delete(delete_setting))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Method, Request, Response},
    };
    use sqlx::sqlite::SqlitePoolOptions;
    use tower::util::ServiceExt; // for `oneshot`

    // Build an in-memory-backed app and convert it to a `Service` so the
    // compiler can infer the request/response body types.
    async fn test_app() -> impl tower::Service<
        Request<Body>,
        Response = Response<Body>,
        Error = std::convert::Infallible,
        Future: Send,
    > + Clone {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("pool");
        let store = Arc::new(SettingsStore::new(pool).await.expect("store"));
        let state = Arc::new(SettingsState { store });
        Router::new()
            .nest("/api/settings", settings_router(state))
            .into_service::<Body>()
    }

    async fn json_body(resp: Response<Body>) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // ── GET /api/settings ─────────────────────────────────────────────────

    #[tokio::test]
    async fn get_settings_empty() {
        let app = test_app().await;
        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/settings")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(json_body(resp).await, serde_json::json!({}));
    }

    #[tokio::test]
    async fn get_settings_returns_stored_values() {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("pool");
        let store = Arc::new(SettingsStore::new(pool).await.expect("store"));
        store
            .set("app_settings", r#"{"theme":"dark"}"#)
            .await
            .unwrap();

        let state = Arc::new(SettingsState { store });
        let app = Router::new()
            .nest("/api/settings", settings_router(state))
            .into_service::<Body>();

        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/settings")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let val = json_body(resp).await;
        assert_eq!(val["app_settings"], r#"{"theme":"dark"}"#);
    }

    // ── PUT /api/settings ─────────────────────────────────────────────────

    #[tokio::test]
    async fn put_settings_returns_no_content() {
        let app = test_app().await;
        let body = serde_json::to_vec(&serde_json::json!({"app_settings": "{}"})).unwrap();
        let req = Request::builder()
            .method(Method::PUT)
            .uri("/api/settings")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn put_then_get_round_trips() {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("pool");
        let store = Arc::new(SettingsStore::new(pool).await.expect("store"));
        let state = Arc::new(SettingsState { store });
        let app = Router::new()
            .nest("/api/settings", settings_router(state))
            .into_service::<Body>();

        // PUT
        let body = serde_json::to_vec(
            &serde_json::json!({"app_settings": r#"{"columns":["todo","done"]}"#}),
        )
        .unwrap();
        let put_req = Request::builder()
            .method(Method::PUT)
            .uri("/api/settings")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let put_resp = app.clone().oneshot(put_req).await.unwrap();
        assert_eq!(put_resp.status(), StatusCode::NO_CONTENT);

        // GET
        let get_req = Request::builder()
            .method(Method::GET)
            .uri("/api/settings")
            .body(Body::empty())
            .unwrap();
        let get_resp = app.oneshot(get_req).await.unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);
        let val = json_body(get_resp).await;
        assert_eq!(val["app_settings"], r#"{"columns":["todo","done"]}"#);
    }

    // ── DELETE /api/settings/:key ─────────────────────────────────────────

    #[tokio::test]
    async fn delete_setting_returns_no_content() {
        let app = test_app().await;
        let req = Request::builder()
            .method(Method::DELETE)
            .uri("/api/settings/app_settings")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn delete_removes_key() {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("pool");
        let store = Arc::new(SettingsStore::new(pool).await.expect("store"));
        store.set("to_delete", "bye").await.unwrap();

        let state = Arc::new(SettingsState { store });
        let app = Router::new()
            .nest("/api/settings", settings_router(state))
            .into_service::<Body>();

        // DELETE
        let del_req = Request::builder()
            .method(Method::DELETE)
            .uri("/api/settings/to_delete")
            .body(Body::empty())
            .unwrap();
        app.clone().oneshot(del_req).await.unwrap();

        // GET — should now be empty
        let get_req = Request::builder()
            .method(Method::GET)
            .uri("/api/settings")
            .body(Body::empty())
            .unwrap();
        let get_resp = app.oneshot(get_req).await.unwrap();
        let val = json_body(get_resp).await;
        assert_eq!(val, serde_json::json!({}));
    }
}
