//! Shared router builder for the Molt Hub Axum server.
//!
//! Used by both the standalone CLI binary (`molt-hub serve`) and the Tauri desktop shell.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::routing::get;
use axum::Router;
use tokio::task::JoinHandle;
use tower_http::services::{ServeDir, ServeFile};
use tracing::debug;

use crate::pipeline::handlers::{pipeline_router, PipelineState};
use crate::ws::{ws_handler, ConnectionManager};
use crate::ws_broadcast::{broadcast_metrics, MetricsPayload};

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the Molt Hub Axum router with WebSocket and static file serving.
///
/// Returns both the router and a shared `ConnectionManager` that callers can
/// use to broadcast events to connected clients.
///
/// The returned router provides:
/// - `GET /ws` — WebSocket upgrade for real-time UI updates
/// - `/*`      — Static files from `dist_dir` with `index.html` fallback (SPA routing)
pub fn build_router(dist_dir: PathBuf) -> (Router, Arc<ConnectionManager>) {
    let manager = Arc::new(ConnectionManager::new());
    let index_html = dist_dir.join("index.html");

    // Pipeline stages API state
    let pipeline_state = Arc::new(PipelineState::default_stages());

    // Pipeline sub-router has its own state, so we build it independently
    // and nest it as a service to avoid state type mismatches.
    let pipeline = pipeline_router(pipeline_state);

    let router = Router::new()
        .route("/ws", get(ws_handler))
        .nest_service("/api/pipeline", pipeline)
        .fallback_service(ServeDir::new(dist_dir).fallback(ServeFile::new(index_html)))
        .with_state(Arc::clone(&manager));

    (router, manager)
}

// ---------------------------------------------------------------------------
// Periodic health metrics broadcast
// ---------------------------------------------------------------------------

/// Spawn a background task that periodically broadcasts system health metrics
/// to all connected WebSocket clients every `interval`.
///
/// Metrics include:
/// - CPU usage (approximated from system load average)
/// - Memory usage (from process RSS via libc)
/// - Active connection count (as a proxy for active agents until the real
///   agent registry is wired)
///
/// Returns a `JoinHandle` that can be used to abort the task on shutdown.
pub fn spawn_health_metrics_task(
    manager: Arc<ConnectionManager>,
    interval: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;

            let (cpu_usage, memory_bytes) = collect_system_metrics();
            let active_connections = manager.connection_count() as u32;

            let payload = MetricsPayload {
                active_agent_count: Some(active_connections),
                cpu_usage: Some(cpu_usage),
                memory_bytes: Some(memory_bytes),
            };

            debug!(
                cpu = cpu_usage,
                mem_bytes = memory_bytes,
                connections = active_connections,
                "broadcasting health metrics"
            );

            broadcast_metrics(&manager, &payload);
        }
    })
}

/// Collect basic system metrics without external crate dependencies.
///
/// - **CPU**: Uses libc `getloadavg` on Unix; returns a normalised 0-100 value
///   based on 1-minute load average divided by available CPUs.
/// - **Memory**: Reads process RSS from `/proc/self/statm` on Linux; falls
///   back to a reasonable estimate on other platforms.
fn collect_system_metrics() -> (f64, u64) {
    let cpu_usage = collect_cpu_usage();
    let memory_bytes = collect_memory_bytes();
    (cpu_usage, memory_bytes)
}

#[cfg(unix)]
fn collect_cpu_usage() -> f64 {
    let mut loadavg: [f64; 3] = [0.0; 3];
    // SAFETY: getloadavg writes up to `nelem` doubles into the provided buffer.
    let ret = unsafe { libc::getloadavg(loadavg.as_mut_ptr(), 1) };
    if ret < 1 {
        return 0.0;
    }
    let ncpus = std::thread::available_parallelism()
        .map(|n| n.get() as f64)
        .unwrap_or(1.0);
    // Normalise to 0-100 range (capped).
    (loadavg[0] / ncpus * 100.0).min(100.0).max(0.0)
}

#[cfg(not(unix))]
fn collect_cpu_usage() -> f64 {
    // Fallback: return a modest constant on non-Unix.
    15.0
}

fn collect_memory_bytes() -> u64 {
    // Try reading from /proc/self/statm (Linux).
    if let Ok(contents) = std::fs::read_to_string("/proc/self/statm") {
        // Second field is RSS in pages.
        if let Some(rss_pages_str) = contents.split_whitespace().nth(1) {
            if let Ok(rss_pages) = rss_pages_str.parse::<u64>() {
                let page_size = 4096u64; // typical page size
                return rss_pages * page_size;
            }
        }
    }

    // macOS: use mach task_info to get RSS.
    #[cfg(target_os = "macos")]
    {
        if let Some(rss) = macos_rss() {
            return rss;
        }
    }

    // Fallback: estimate ~50 MB.
    50 * 1024 * 1024
}

#[cfg(target_os = "macos")]
fn macos_rss() -> Option<u64> {
    use std::mem;
    // Use rusage to get RSS on macOS — avoids deprecated mach_task_self.
    // SAFETY: getrusage is a standard POSIX call reading our own process stats.
    unsafe {
        let mut usage: libc::rusage = mem::zeroed();
        let ret = libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        if ret == 0 {
            // On macOS, ru_maxrss is in bytes (unlike Linux where it's in KB).
            Some(usage.ru_maxrss as u64)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_router_does_not_panic() {
        let (_router, _manager) = build_router(PathBuf::from("/tmp/nonexistent-dist"));
    }

    #[test]
    fn build_router_returns_shared_manager() {
        let (_router, manager) = build_router(PathBuf::from("/tmp/nonexistent-dist"));
        assert_eq!(manager.connection_count(), 0);
    }

    #[test]
    fn collect_system_metrics_returns_sane_values() {
        let (cpu, mem) = collect_system_metrics();
        assert!(cpu >= 0.0 && cpu <= 100.0, "cpu out of range: {cpu}");
        assert!(mem > 0, "memory should be > 0, got {mem}");
    }
}
