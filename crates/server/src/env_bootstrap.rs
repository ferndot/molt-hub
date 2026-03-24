//! Load optional `.env` before OAuth and other `std::env` reads.

use std::path::{Path, PathBuf};

/// Find and load the first `.env` in the current directory or a parent (up to 12 levels).
///
/// Variables already set in the process environment are **not** overridden ([`dotenvy`] default).
/// Missing file is ignored.
pub fn load_dotenv_files() {
    let mut dir: Option<PathBuf> = std::env::current_dir().ok();
    let mut env_path: Option<PathBuf> = None;
    for _ in 0..12 {
        let Some(ref d) = dir else {
            break;
        };
        let candidate = d.join(".env");
        if candidate.is_file() {
            env_path = Some(candidate);
            break;
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    if let Some(path) = env_path {
        let _ = dotenvy::from_path(path);
    }
}
