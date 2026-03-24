//! Load optional `.env` files before OAuth and other `std::env` reads.
//!
//! Order (each step only sets variables **not** already in the process environment):
//! 1. **User config** — [`dirs::config_dir`]`/molt-hub/.env` (stable for desktop apps regardless of cwd).
//! 2. **Working tree** — first `.env` found walking up from [`std::env::current_dir`] (developer convenience).
//!
//! OAuth **app** secrets can also live in `molt-hub/oauth-clients.json` next to that `.env`; the
//! server loads that file when the router is built (see `integrations::oauth_clients`).

use std::path::{Path, PathBuf};

fn load_env_path(path: &Path) {
    if path.is_file() {
        let _ = dotenvy::from_path(path);
    }
}

/// Load optional `.env` files in precedence order.
///
/// Real environment variables (shell, launchd, CI) always win. Files are never bundled inside the
/// app binary; keep user secrets in config-dir `.env` or the OS keychain (tokens after OAuth).
pub fn load_dotenv_files() {
    // 1. Per-user location — recommended for installed desktop builds.
    if let Some(base) = dirs::config_dir() {
        let user_env = base.join("molt-hub").join(".env");
        load_env_path(&user_env);
    }

    // 2. Repo / cwd — typical for `cargo run` / `./dev.sh`.
    let mut dir: Option<PathBuf> = std::env::current_dir().ok();
    for _ in 0..12 {
        let Some(ref d) = dir else {
            break;
        };
        let candidate = d.join(".env");
        if candidate.is_file() {
            load_env_path(&candidate);
            break;
        }
        dir = d.parent().map(Path::to_path_buf);
    }
}
