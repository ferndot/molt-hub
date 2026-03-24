//! Molt Hub desktop application — Tauri v2 shell with embedded Axum server.
//!
//! In release mode, spawns the Axum server and opens a webview to localhost:13401.
//! In debug mode (`cargo run`), skips the embedded server and points the webview
//! at the Vite dev server (localhost:5173) for hot module reloading.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::net::SocketAddr;
use std::path::PathBuf;

use tauri::Manager;
use molt_hub_server::serve::{build_router, spawn_health_metrics_task};

/// Port for the embedded Axum server.
/// Uncommon port to avoid collisions with common dev servers.
const SERVER_PORT: u16 = 13401;

/// URL the webview loads — Vite dev server in debug, embedded server in release.
fn webview_url() -> String {
    if cfg!(debug_assertions) {
        // In dev, use Vite for HMR. Start `npm run dev` in ui/ separately.
        let port = std::env::var("VITE_PORT").unwrap_or_else(|_| "5173".to_string());
        format!("http://localhost:{port}")
    } else {
        format!("http://localhost:{SERVER_PORT}")
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
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // In release mode, spawn the embedded Axum server.
            // In debug mode, assume `npm run dev` and `cargo run --bin molt-hub` are
            // running externally for full HMR support.
            if !cfg!(debug_assertions) {
                std::thread::spawn(|| {
                    let rt =
                        tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
                    rt.block_on(async {
                        let dist_dir = resolve_dist_dir();
                        let (router, manager, supervisor, _audit) = build_router(dist_dir).await;
                        let _metrics_handle = spawn_health_metrics_task(
                            manager,
                            supervisor,
                            std::time::Duration::from_secs(5),
                        );
                        let addr: SocketAddr =
                            format!("127.0.0.1:{SERVER_PORT}").parse().unwrap();
                        tracing::info!(address = %addr, "embedded server starting");
                        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
                        axum::serve(listener, router).await.unwrap();
                    });
                });
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
        .expect("error while running tauri application");
}

/// Resolve the UI dist directory for the embedded server.
///
/// Resolution order:
/// 1. `UI_DIST` environment variable
/// 2. Walk up from the binary location looking for `ui/dist`
/// 3. Fall back to `ui/dist` relative to CWD
fn resolve_dist_dir() -> PathBuf {
    if let Ok(p) = std::env::var("UI_DIST") {
        return PathBuf::from(p);
    }

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
