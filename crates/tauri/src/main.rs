//! Molt Hub desktop application — Tauri v2 shell with embedded Axum server.
//!
//! In release mode, spawns the Axum server and opens a webview to 127.0.0.1:13401.
//! In debug mode (`cargo run`), skips the embedded server and points the webview
//! at the Vite dev server (localhost:5173) for hot module reloading.
//!
//! OAuth (HTTPS bridge → `molthub://oauth/{jira|github}?…`): the deep-link plugin
//! forwards `code`/`state` to the local API so PKCE can complete on the same process
//! that started the flow.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use tauri::Manager;
use tauri_plugin_deep_link::DeepLinkExt;
use url::Url;

use molt_hub_server::serve::{build_router, spawn_health_metrics_task};

/// Port for the embedded Axum server.
/// Uncommon port to avoid collisions with common dev servers.
const SERVER_PORT: u16 = 13401;

fn local_api_port() -> u16 {
    std::env::var("MOLTHUB_LOCAL_API_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(SERVER_PORT)
}

/// Forward `molthub://oauth/jira|github?…` to the local HTTP OAuth callback (PKCE verifier lives there).
fn forward_oauth_deep_links(urls: Vec<Url>) {
    let port = local_api_port();
    for u in urls {
        if u.scheme() != "molthub" {
            continue;
        }
        let host = u.host_str().unwrap_or("");
        let path = u.path().trim_start_matches('/');
        let provider = match (host, path) {
            ("oauth", "jira") => "jira",
            ("oauth", "github") => "github",
            _ => {
                tracing::debug!(%u, "deep link ignored (not an OAuth callback path)");
                continue;
            }
        };
        let api_path = match provider {
            "jira" => "/api/integrations/jira/oauth/callback",
            "github" => "/api/integrations/github/oauth/callback",
            _ => continue,
        };
        let query = u.query().unwrap_or("");
        let target = format!("http://127.0.0.1:{port}{api_path}?{query}");
        std::thread::spawn(move || {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_else(|_| reqwest::blocking::Client::new());
            match client.get(&target).send() {
                Ok(resp) => {
                    tracing::info!(status = %resp.status(), url = %target, "oauth callback forwarded from deep link");
                }
                Err(e) => tracing::warn!(
                    error = %e,
                    url = %target,
                    "oauth deep link forward failed — start `molt-hub serve` (or the release app) on this port"
                ),
            }
        });
    }
}

/// URL the webview loads — Vite dev server in debug, embedded server in release.
fn webview_url() -> String {
    if cfg!(debug_assertions) {
        // In dev, use Vite for HMR. Start `npm run dev` in ui/ separately.
        let port = std::env::var("VITE_PORT").unwrap_or_else(|_| "5173".to_string());
        format!("http://localhost:{port}")
    } else {
        // Match the embedded listener (127.0.0.1) so we never rely on `localhost` → ::1
        // resolving while the server is IPv4-only.
        format!("http://127.0.0.1:{}", local_api_port())
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {
            tracing::info!("single-instance handoff (e.g. second open or deep link on Windows/Linux)");
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            if let Err(e) = app.deep_link().register_all() {
                tracing::warn!(error = %e, "deep link register_all");
            }

            let _ = app.deep_link().on_open_url(|event| {
                forward_oauth_deep_links(event.urls());
            });

            if let Ok(Some(urls)) = app.deep_link().get_current() {
                if !urls.is_empty() {
                    forward_oauth_deep_links(urls);
                }
            }

            // In release mode, spawn the embedded Axum server.
            // In debug mode, assume `npm run dev` and `cargo run --bin molt-hub` are
            // running externally for full HMR support.
            if !cfg!(debug_assertions) {
                let bind_port = local_api_port();
                let dist_dir = resolve_embedded_dist_dir(app);
                std::thread::spawn(move || {
                    let rt =
                        tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
                    rt.block_on(async {
                        let (router, manager, supervisor, _audit) = build_router(dist_dir).await;
                        let _metrics_handle = spawn_health_metrics_task(
                            manager,
                            supervisor,
                            std::time::Duration::from_secs(5),
                        );
                        let addr: SocketAddr =
                            format!("127.0.0.1:{bind_port}").parse().unwrap();
                        tracing::info!(address = %addr, "embedded server starting");
                        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
                        axum::serve(listener, router).await.unwrap();
                    });
                });
                // Avoid navigating before `bind`: otherwise `/api/*` can fall through to the
                // SPA static fallback (200 + index.html) and OAuth sees “invalid JSON”.
                wait_for_local_port(bind_port, Duration::from_secs(30));
            } else {
                tracing::info!(
                    "dev mode — webview points to Vite dev server. \
                     Start `npm run dev` in ui/ for HMR."
                );
            }

            // Update the main window URL to match dev/release target.
            let url = webview_url();
            if let Some(window) = app.get_webview_window("main") {
                let parsed: tauri::Url = url.parse().unwrap();
                let _ = window.navigate(parsed);
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}

/// UI `dist/` for the embedded Axum static layer (SPA + fallback).
///
/// Packaged macOS apps place `frontendDist` files at **`Contents/Resources/`** (flat
/// `index.html`), not `ui/dist` next to the binary — use Tauri’s resource dir first.
fn resolve_embedded_dist_dir(app: &tauri::App) -> PathBuf {
    if let Ok(p) = std::env::var("UI_DIST") {
        return PathBuf::from(p);
    }

    if let Ok(res) = app.path().resource_dir() {
        if res.join("index.html").exists() {
            tracing::info!(path = %res.display(), "using Tauri resource dir for UI dist");
            return res;
        }
    }

    resolve_dist_dir_from_exe_or_cwd()
}

/// Fallback when not running from a Tauri bundle (e.g. `cargo run --release` from repo).
///
/// 1. Walk up from the binary looking for `ui/dist`
/// 2. `ui/dist` relative to CWD
fn resolve_dist_dir_from_exe_or_cwd() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent();
        while let Some(d) = dir {
            let candidate = d.join("ui/dist");
            if candidate.exists() {
                return candidate;
            }
            dir = d.parent();
        }
    }

    PathBuf::from("ui/dist")
}

fn wait_for_local_port(port: u16, timeout: Duration) {
    let addr = format!("127.0.0.1:{port}");
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if std::net::TcpStream::connect(&addr).is_ok() {
            return;
        }
        if std::time::Instant::now() > deadline {
            tracing::error!(
                port,
                "embedded server did not accept connections in time — UI may be broken until refresh"
            );
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
