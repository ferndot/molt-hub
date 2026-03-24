//! Axum HTTP handlers for the audit log API.
//!
//! Routes:
//!   GET /api/audit             — return recent audit entries
//!   GET /api/audit?action=Spawn&limit=50  — filter by action type

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use tracing::instrument;

use super::writer::{AuditAction, AuditHandle};

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// State shared across audit handlers.
pub struct AuditState {
    pub handle: AuditHandle,
}

// ---------------------------------------------------------------------------
// Query parameters
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    /// Filter by action type (case-insensitive: "spawn", "send", "terminate", "import").
    pub action: Option<String>,
    /// Maximum number of entries to return (default: 100).
    pub limit: Option<usize>,
}

// ---------------------------------------------------------------------------
// Handler: GET /api/audit
// ---------------------------------------------------------------------------

#[instrument(skip_all)]
pub async fn get_audit_entries(
    State(state): State<Arc<AuditState>>,
    Query(params): Query<AuditQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(100).min(1000);
    let action_filter = params
        .action
        .as_deref()
        .and_then(AuditAction::from_str_loose);
    let entries = state.handle.recent(limit, action_filter).await;
    Json(entries).into_response()
}

// ---------------------------------------------------------------------------
// Router builder
// ---------------------------------------------------------------------------

/// Build the `/api/audit` sub-router.
pub fn audit_router(state: Arc<AuditState>) -> Router {
    Router::new()
        .route("/", get(get_audit_entries))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::start_audit_writer;
    use axum::{
        body::Body,
        http::{Method, Request, Response, StatusCode},
    };
    use tower::util::ServiceExt;

    fn test_app(
        handle: AuditHandle,
    ) -> impl tower::Service<
        Request<Body>,
        Response = Response<Body>,
        Error = std::convert::Infallible,
        Future: Send,
    > + Clone {
        let state = Arc::new(AuditState { handle });
        Router::new()
            .nest("/api/audit", audit_router(state))
            .into_service::<Body>()
    }

    async fn json_body(resp: Response<Body>) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn get_audit_returns_empty_array_initially() {
        let handle = start_audit_writer();
        let app = test_app(handle);
        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/audit")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert!(body.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn get_audit_returns_logged_entries() {
        let handle = start_audit_writer();
        handle.log_spawn("agent-1", serde_json::json!({"pid": 100}));
        handle.log_send("agent-1", serde_json::json!({"msg": "hi"}));
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let app = test_app(handle);
        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/audit")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = json_body(resp).await;
        let arr = body.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // Newest first
        assert_eq!(arr[0]["action"], "send");
    }

    #[tokio::test]
    async fn get_audit_filters_by_action() {
        let handle = start_audit_writer();
        handle.log_spawn("s1", serde_json::json!({}));
        handle.log_send("d1", serde_json::json!({}));
        handle.log_spawn("s2", serde_json::json!({}));
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let app = test_app(handle);
        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/audit?action=Spawn")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = json_body(resp).await;
        let arr = body.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        for entry in arr {
            assert_eq!(entry["action"], "spawn");
        }
    }

    #[tokio::test]
    async fn get_audit_respects_limit() {
        let handle = start_audit_writer();
        for i in 0..10 {
            handle.log_spawn(format!("a{i}"), serde_json::json!({}));
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let app = test_app(handle);
        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/audit?limit=3")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = json_body(resp).await;
        assert_eq!(body.as_array().unwrap().len(), 3);
    }
}
