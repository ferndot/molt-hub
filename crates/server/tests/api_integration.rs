//! Integration tests for the Molt Hub HTTP API.
//!
//! These tests build the real Axum router (with a temporary dist directory)
//! and exercise API endpoints using `tower::ServiceExt::oneshot`.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use molt_hub_server::serve::build_router;

/// Helper: build the router with a throwaway dist dir.
async fn app() -> axum::Router {
    let dist = PathBuf::from("/tmp/molt-hub-test-dist");
    std::fs::create_dir_all(&dist).ok();
    // Create a minimal index.html so the fallback service doesn't fail.
    std::fs::write(dist.join("index.html"), "<html></html>").ok();
    let (router, _mgr, _supervisor, _audit) = build_router(dist).await;
    router
}

// ---------------------------------------------------------------------------
// Pipeline stages API
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_pipeline_stages_returns_default_stages() {
    let app = app().await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/pipeline/stages")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
        .await
        .unwrap();
    let wrapper: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Response is { "stages": [...] }.
    assert!(wrapper["stages"].is_array(), "expected stages array in response");
    let arr = wrapper["stages"].as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one default stage");

    // Each stage should have an id and label.
    let first = &arr[0];
    assert!(first.get("id").is_some(), "stage missing 'id'");
    assert!(first.get("label").is_some(), "stage missing 'label'");
}

#[tokio::test]
async fn project_scoped_pipeline_uses_same_runtime_registry_as_build_router() {
    let app = app().await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/projects/default/pipeline/stages")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "default project runtime must be registered; empty Extension registry would return 404"
    );

    let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
        .await
        .unwrap();
    let wrapper: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        wrapper["stages"].is_array() && !wrapper["stages"].as_array().unwrap().is_empty(),
        "expected non-empty stages from default project pipeline"
    );
}

// ---------------------------------------------------------------------------
// Settings API round-trip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn settings_put_then_get_round_trip() {
    let app = app().await;

    // PUT a custom settings payload.
    let settings_json = serde_json::json!({
        "appearance": { "theme": "dark", "colorblindMode": true },
        "notifications": { "attentionLevel": "all" },
        "agentDefaults": { "timeoutMinutes": 60, "adapter": "claude-code" },
        "kanban_columns": []
    });

    let put_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/settings")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&settings_json).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(put_resp.status(), StatusCode::NO_CONTENT);

    // GET the settings back.
    let get_resp = app
        .oneshot(
            Request::builder()
                .uri("/api/settings")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(get_resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(get_resp.into_body(), 1_000_000)
        .await
        .unwrap();
    let returned: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(returned["appearance"]["theme"], "dark");
    assert_eq!(returned["appearance"]["colorblindMode"], true);
    assert_eq!(returned["notifications"]["attentionLevel"], "all");
    assert_eq!(returned["agentDefaults"]["timeoutMinutes"], 60);
}

#[tokio::test]
async fn jira_oauth_auth_returns_json_with_authorization_url() {
    let app = app().await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/integrations/jira/auth")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let url = v["url"].as_str().expect("expected url string");
    assert!(
        url.starts_with("https://"),
        "expected absolute https authorize URL, got {url:?}"
    );
    assert!(v["state"].as_str().is_some(), "expected state string");
}

#[tokio::test]
async fn jira_search_returns_unauthorized_without_oauth() {
    let app = app().await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/integrations/jira/search?jql=project%20%3D%20FOO")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Jira REST must be mounted and require the same OAuth session as /auth"
    );
}

#[tokio::test]
async fn github_repos_returns_unauthorized_without_oauth() {
    let app = app().await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/integrations/github/repos")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "GitHub REST must be mounted and require OAuth like /status"
    );
}

#[tokio::test]
async fn github_issues_endpoint_is_mounted() {
    let app = app().await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/integrations/github/issues?owner=o&repo=r&state=open")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
