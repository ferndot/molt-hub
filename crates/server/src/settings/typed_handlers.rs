//! Axum HTTP handlers for the typed settings API (JSON-file-backed).
//!
//! Routes:
//!   GET   /                — return current settings
//!   PUT   /                — full-replace settings
//!   PATCH /:section        — update a single section

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, patch, put},
    Json, Router,
};
use tracing::instrument;

use super::file_store::SettingsFileStore;
use super::model::ServerSettings;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// State shared across typed settings handlers.
pub struct TypedSettingsState {
    pub store: Arc<SettingsFileStore>,
}

// ---------------------------------------------------------------------------
// Handler: GET /api/settings
// ---------------------------------------------------------------------------

#[instrument(skip_all)]
pub async fn get_typed_settings(
    State(state): State<Arc<TypedSettingsState>>,
) -> impl IntoResponse {
    let settings = state.store.get().await;
    Json(settings).into_response()
}

// ---------------------------------------------------------------------------
// Handler: PUT /api/settings
// ---------------------------------------------------------------------------

#[instrument(skip_all)]
pub async fn put_typed_settings(
    State(state): State<Arc<TypedSettingsState>>,
    Json(body): Json<ServerSettings>,
) -> impl IntoResponse {
    match state.store.set(body).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Handler: PATCH /api/settings/:section
// ---------------------------------------------------------------------------

#[instrument(skip_all, fields(%section))]
pub async fn patch_settings_section(
    State(state): State<Arc<TypedSettingsState>>,
    Path(section): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    match state.store.patch_section(&section, body).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            let status = if matches!(
                e,
                super::file_store::SettingsPatchError::UnknownSection(_)
            ) {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            };
            (status, Json(serde_json::json!({ "error": e.to_string() }))).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Router builder
// ---------------------------------------------------------------------------

/// Build the typed `/api/settings` sub-router.
pub fn typed_settings_router(state: Arc<TypedSettingsState>) -> Router {
    Router::new()
        .route("/", get(get_typed_settings))
        .route("/", put(put_typed_settings))
        .route("/:section", patch(patch_settings_section))
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
    use tower::util::ServiceExt;

    fn temp_path() -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("molt-typed-settings-test-{}", ulid::Ulid::new()));
        dir.join("settings.json")
    }

    fn test_app(
        path: std::path::PathBuf,
    ) -> impl tower::Service<
        Request<Body>,
        Response = Response<Body>,
        Error = std::convert::Infallible,
        Future: Send,
    > + Clone {
        let store = Arc::new(SettingsFileStore::open(path));
        let state = Arc::new(TypedSettingsState { store });
        Router::new()
            .nest("/api/settings", typed_settings_router(state))
            .into_service::<Body>()
    }

    async fn json_body(resp: Response<Body>) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // ── GET defaults ────────────────────────────────────────────────────

    #[tokio::test]
    async fn get_returns_defaults() {
        let path = temp_path();
        let app = test_app(path.clone());
        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/settings")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["appearance"]["theme"], "system");
        assert_eq!(body["agentDefaults"]["adapter"], "claude-code");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    // ── PUT full replace ────────────────────────────────────────────────

    #[tokio::test]
    async fn put_then_get_round_trips() {
        let path = temp_path();
        let app = test_app(path.clone());

        let mut settings = ServerSettings::default();
        settings.appearance.theme = "dark".to_owned();
        let body = serde_json::to_vec(&settings).unwrap();

        let put_req = Request::builder()
            .method(Method::PUT)
            .uri("/api/settings")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let put_resp = app.clone().oneshot(put_req).await.unwrap();
        assert_eq!(put_resp.status(), StatusCode::NO_CONTENT);

        let get_req = Request::builder()
            .method(Method::GET)
            .uri("/api/settings")
            .body(Body::empty())
            .unwrap();
        let get_resp = app.oneshot(get_req).await.unwrap();
        let val = json_body(get_resp).await;
        assert_eq!(val["appearance"]["theme"], "dark");

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    // ── PATCH section ───────────────────────────────────────────────────

    #[tokio::test]
    async fn patch_appearance() {
        let path = temp_path();
        let app = test_app(path.clone());

        let body =
            serde_json::to_vec(&serde_json::json!({"theme": "light", "colorblindMode": true}))
                .unwrap();
        let req = Request::builder()
            .method(Method::PATCH)
            .uri("/api/settings/appearance")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Verify
        let get_req = Request::builder()
            .method(Method::GET)
            .uri("/api/settings")
            .body(Body::empty())
            .unwrap();
        let get_resp = app.oneshot(get_req).await.unwrap();
        let val = json_body(get_resp).await;
        assert_eq!(val["appearance"]["theme"], "light");
        assert_eq!(val["appearance"]["colorblindMode"], true);

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn patch_unknown_section_returns_400() {
        let path = temp_path();
        let app = test_app(path.clone());

        let req = Request::builder()
            .method(Method::PATCH)
            .uri("/api/settings/bogus")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    // ── Persistence across reloads ──────────────────────────────────────

    #[tokio::test]
    async fn settings_persist_across_store_instances() {
        let path = temp_path();

        // First instance: write
        {
            let store = Arc::new(SettingsFileStore::open(path.clone()));
            let mut s = ServerSettings::default();
            s.notifications.attention_level = "all".to_owned();
            store.set(s).await.unwrap();
        }

        // Second instance: read
        {
            let store = Arc::new(SettingsFileStore::open(path.clone()));
            let loaded = store.get().await;
            assert_eq!(loaded.notifications.attention_level, "all");
        }

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
