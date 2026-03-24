//! `molt-hub` server binary entry point.
//!
//! # Usage
//!
//! ```text
//! molt-hub serve [--port <PORT>] [--host <HOST>] [--no-open]
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::EnvFilter;

use molt_hub_server::serve::{build_router, spawn_health_metrics_task};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

/// Molt Hub — AI agent orchestration server.
#[derive(Debug, Parser)]
#[command(name = "molt-hub", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start the Molt Hub server and open the web UI.
    Serve(ServeArgs),
}

#[derive(Debug, Parser)]
struct ServeArgs {
    /// Port to listen on.
    #[arg(long, default_value_t = 3001)]
    port: u16,

    /// Host address to bind to.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Do not automatically open the browser.
    #[arg(long = "no-open")]
    no_open: bool,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    // Initialise tracing. RUST_LOG overrides; default to info.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    let cli = Cli::parse();

    // `serve` is the only command; treat bare invocation as `serve` with defaults.
    let args = match cli.command {
        Some(Command::Serve(a)) => a,
        None => ServeArgs {
            port: 3001,
            host: "127.0.0.1".to_owned(),
            no_open: false,
        },
    };

    run_serve(args).await;
}

// ---------------------------------------------------------------------------
// Server logic
// ---------------------------------------------------------------------------

async fn run_serve(args: ServeArgs) {
    let dist_dir = locate_dist_dir();

    // Warn if ui/dist is missing; we continue anyway (WebSocket still works).
    if !dist_dir.exists() {
        eprintln!(
            "Warning: frontend build not found at {dist}.\n\
             Run `npm run build` in ui/ first, or use `npm run dev` for development.",
            dist = dist_dir.display()
        );
    }

    let (app, manager, supervisor, _audit) = build_router(dist_dir).await;

    // Spawn periodic health metrics broadcast (every 5 seconds).
    let _metrics_handle = spawn_health_metrics_task(
        manager,
        supervisor,
        std::time::Duration::from_secs(5),
    );

    let addr: SocketAddr = format!("{}:{}", args.host, args.port)
        .parse()
        .expect("invalid host:port");

    let url = format!("http://{}:{}", args.host, args.port);
    println!("Molt Hub listening on {url}");
    info!(address = %addr, "server starting");

    // Optionally open browser.
    if !args.no_open {
        let url_clone = url.clone();
        tokio::spawn(async move {
            // Small delay so the server is ready before the browser opens.
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            if let Err(e) = open::that(&url_clone) {
                tracing::warn!(error = %e, "failed to open browser");
            }
        });
    }

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");

    info!("server shut down");
}

/// Locate the `ui/dist` directory relative to the binary or workspace root.
///
/// Resolution order:
/// 1. `UI_DIST` environment variable (useful for packaging / CI)
/// 2. `./ui/dist` relative to the current working directory
/// 3. Walk up from the binary location looking for `ui/dist`
fn locate_dist_dir() -> PathBuf {
    if let Ok(env_path) = std::env::var("UI_DIST") {
        return PathBuf::from(env_path);
    }

    // CWD-relative (works when running `cargo run` from workspace root).
    let cwd_relative = PathBuf::from("ui/dist");
    if cwd_relative.exists() {
        return cwd_relative;
    }

    // Walk up from binary location.
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

    // Fall back to CWD-relative (will display the missing warning).
    cwd_relative
}

/// Resolves when Ctrl+C is received.
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
    info!("received Ctrl+C, shutting down");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// Clap's built-in debug_assert validates the argument configuration.
    #[test]
    fn cli_config_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn serve_defaults() {
        let args = ServeArgs::parse_from(["molt-hub"]);
        assert_eq!(args.port, 3001);
        assert_eq!(args.host, "127.0.0.1");
        assert!(!args.no_open);
    }

    #[test]
    fn serve_custom_port() {
        let args = ServeArgs::parse_from(["molt-hub", "--port", "8080"]);
        assert_eq!(args.port, 8080);
    }

    #[test]
    fn serve_no_open_flag() {
        let args = ServeArgs::parse_from(["molt-hub", "--no-open"]);
        assert!(args.no_open);
    }

    #[test]
    fn serve_custom_host() {
        let args = ServeArgs::parse_from(["molt-hub", "--host", "0.0.0.0"]);
        assert_eq!(args.host, "0.0.0.0");
    }

    #[test]
    fn locate_dist_dir_uses_env_var() {
        std::env::set_var("UI_DIST", "/tmp/custom-dist");
        let path = locate_dist_dir();
        std::env::remove_var("UI_DIST");
        assert_eq!(path, PathBuf::from("/tmp/custom-dist"));
    }

    /// Verify the router compiles with both /ws and static fallback.
    #[tokio::test]
    async fn router_builds_without_panic() {
        let dist = PathBuf::from("/tmp/nonexistent-dist");
        let (_app, _mgr, _sup, _audit) = build_router(dist).await;
        // If we got here, the router compiled and wired correctly.
    }
}
