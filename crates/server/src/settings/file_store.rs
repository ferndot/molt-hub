//! JSON-file-backed persistence for [`ServerSettings`].
//!
//! Settings are stored at `~/.config/molt-hub/settings.json` (or the
//! platform-appropriate config directory).  All reads and writes go through
//! a `tokio::sync::RwLock` so concurrent HTTP handlers are safe.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{info, warn};

use super::model::ServerSettings;

// ---------------------------------------------------------------------------
// SettingsFileStore
// ---------------------------------------------------------------------------

/// Thread-safe, file-backed settings store.
///
/// On construction, the store loads existing settings from disk (or falls
/// back to defaults).  Every mutation is flushed to disk before the lock
/// is released.
pub struct SettingsFileStore {
    path: PathBuf,
    inner: Arc<RwLock<ServerSettings>>,
}

impl SettingsFileStore {
    /// Open (or create) a settings file at the given path.
    pub fn open(path: PathBuf) -> Self {
        let settings = Self::load_from_disk(&path);
        Self {
            path,
            inner: Arc::new(RwLock::new(settings)),
        }
    }

    /// Open the default settings path: `~/.config/molt-hub/settings.json`.
    pub fn open_default() -> Self {
        Self::open(Self::default_path())
    }

    /// Return the platform-conventional config path.
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("molt-hub")
            .join("settings.json")
    }

    // -- reads ---------------------------------------------------------------

    /// Return a snapshot of the current settings.
    pub async fn get(&self) -> ServerSettings {
        self.inner.read().await.clone()
    }

    // -- writes --------------------------------------------------------------

    /// Full replace of all settings.
    pub async fn set(&self, settings: ServerSettings) -> Result<(), std::io::Error> {
        let mut guard = self.inner.write().await;
        *guard = settings;
        Self::flush_to_disk(&self.path, &guard)
    }

    /// Patch a single section by name.  Returns `Ok(())` on success, or an
    /// error if the section name is unknown or the value fails to deserialise.
    pub async fn patch_section(
        &self,
        section: &str,
        value: serde_json::Value,
    ) -> Result<(), SettingsPatchError> {
        let mut guard = self.inner.write().await;
        match section {
            "appearance" => {
                guard.appearance = serde_json::from_value(value)
                    .map_err(|e| SettingsPatchError::Deserialize(e.to_string()))?;
            }
            "notifications" => {
                guard.notifications = serde_json::from_value(value)
                    .map_err(|e| SettingsPatchError::Deserialize(e.to_string()))?;
            }
            "agent_defaults" | "agentDefaults" => {
                guard.agent_defaults = serde_json::from_value(value)
                    .map_err(|e| SettingsPatchError::Deserialize(e.to_string()))?;
            }
            "kanban_columns" | "kanbanColumns" => {
                guard.kanban_columns = serde_json::from_value(value)
                    .map_err(|e| SettingsPatchError::Deserialize(e.to_string()))?;
            }
            "sidebar_widths" | "sidebarWidths" => {
                guard.sidebar_widths = Some(
                    serde_json::from_value(value)
                        .map_err(|e| SettingsPatchError::Deserialize(e.to_string()))?,
                );
            }
            "jira_config" | "jiraConfig" => {
                guard.jira_config = Some(
                    serde_json::from_value(value)
                        .map_err(|e| SettingsPatchError::Deserialize(e.to_string()))?,
                );
            }
            "github_config" | "githubConfig" => {
                guard.github_config = Some(
                    serde_json::from_value(value)
                        .map_err(|e| SettingsPatchError::Deserialize(e.to_string()))?,
                );
            }
            other => return Err(SettingsPatchError::UnknownSection(other.to_owned())),
        }
        Self::flush_to_disk(&self.path, &guard)
            .map_err(|e| SettingsPatchError::Io(e.to_string()))?;
        Ok(())
    }

    // -- disk I/O ------------------------------------------------------------

    fn load_from_disk(path: &PathBuf) -> ServerSettings {
        match std::fs::read_to_string(path) {
            Ok(contents) => match serde_json::from_str(&contents) {
                Ok(s) => {
                    info!(path = %path.display(), "loaded settings from disk");
                    s
                }
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "corrupt settings file; using defaults");
                    ServerSettings::default()
                }
            },
            Err(_) => {
                info!(path = %path.display(), "no settings file found; using defaults");
                ServerSettings::default()
            }
        }
    }

    fn flush_to_disk(
        path: &PathBuf,
        settings: &ServerSettings,
    ) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(settings)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)?;
        info!(path = %path.display(), "settings flushed to disk");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Error type for patch operations
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum SettingsPatchError {
    #[error("unknown section: {0}")]
    UnknownSection(String),
    #[error("deserialization error: {0}")]
    Deserialize(String),
    #[error("I/O error: {0}")]
    Io(String),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_path() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("molt-settings-test-{}", ulid::Ulid::new()));
        dir.join("settings.json")
    }

    #[tokio::test]
    async fn open_creates_defaults_when_no_file() {
        let path = temp_path();
        let store = SettingsFileStore::open(path.clone());
        let s = store.get().await;
        assert_eq!(s, ServerSettings::default());
        // cleanup
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn set_persists_to_disk() {
        let path = temp_path();
        let store = SettingsFileStore::open(path.clone());

        let mut custom = ServerSettings::default();
        custom.appearance.theme = "dark".to_owned();
        store.set(custom.clone()).await.unwrap();

        // Read back from disk
        let contents = fs::read_to_string(&path).unwrap();
        let on_disk: ServerSettings = serde_json::from_str(&contents).unwrap();
        assert_eq!(on_disk.appearance.theme, "dark");

        // cleanup
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn reload_from_disk_after_set() {
        let path = temp_path();

        // Write settings
        let store = SettingsFileStore::open(path.clone());
        let mut custom = ServerSettings::default();
        custom.notifications.attention_level = "all".to_owned();
        store.set(custom).await.unwrap();

        // Open a new store from the same file
        let store2 = SettingsFileStore::open(path.clone());
        let loaded = store2.get().await;
        assert_eq!(loaded.notifications.attention_level, "all");

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn patch_appearance_section() {
        let path = temp_path();
        let store = SettingsFileStore::open(path.clone());

        store
            .patch_section(
                "appearance",
                serde_json::json!({ "theme": "light", "colorblindMode": true }),
            )
            .await
            .unwrap();

        let s = store.get().await;
        assert_eq!(s.appearance.theme, "light");
        assert!(s.appearance.colorblind_mode);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn patch_unknown_section_returns_error() {
        let path = temp_path();
        let store = SettingsFileStore::open(path.clone());
        let result = store
            .patch_section("nonexistent", serde_json::json!({}))
            .await;
        assert!(result.is_err());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn patch_agent_defaults_camel_case() {
        let path = temp_path();
        let store = SettingsFileStore::open(path.clone());
        store
            .patch_section(
                "agentDefaults",
                serde_json::json!({ "timeoutMinutes": 60, "adapter": "claude-code" }),
            )
            .await
            .unwrap();
        let s = store.get().await;
        assert_eq!(s.agent_defaults.timeout_minutes, 60);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn patch_sidebar_widths_camel_case() {
        let path = temp_path();
        let store = SettingsFileStore::open(path.clone());
        store
            .patch_section(
                "sidebarWidths",
                serde_json::json!({ "navSidebar": 280, "inboxSidebar": 320 }),
            )
            .await
            .unwrap();
        let s = store.get().await;
        let sw = s.sidebar_widths.expect("sidebar_widths should be Some");
        assert_eq!(sw.nav_sidebar, Some(280));
        assert_eq!(sw.inbox_sidebar, Some(320));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn patch_jira_config_camel_case() {
        let path = temp_path();
        let store = SettingsFileStore::open(path.clone());
        store
            .patch_section(
                "jiraConfig",
                serde_json::json!({
                    "connected": true,
                    "baseUrl": "https://mysite.atlassian.net",
                    "siteName": "My Site",
                    "cloudId": "cloud-abc"
                }),
            )
            .await
            .unwrap();
        let s = store.get().await;
        let jc = s.jira_config.expect("jira_config should be Some");
        assert!(jc.connected);
        assert_eq!(jc.base_url.as_deref(), Some("https://mysite.atlassian.net"));
        assert_eq!(jc.site_name.as_deref(), Some("My Site"));
        assert_eq!(jc.cloud_id.as_deref(), Some("cloud-abc"));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn patch_github_config_camel_case() {
        let path = temp_path();
        let store = SettingsFileStore::open(path.clone());
        store
            .patch_section(
                "githubConfig",
                serde_json::json!({ "connected": true, "owner": "octocat" }),
            )
            .await
            .unwrap();
        let s = store.get().await;
        let gc = s.github_config.expect("github_config should be Some");
        assert!(gc.connected);
        assert_eq!(gc.owner.as_deref(), Some("octocat"));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn new_sections_persist_across_reloads() {
        let path = temp_path();

        // Write with new sections
        {
            let store = SettingsFileStore::open(path.clone());
            store
                .patch_section(
                    "sidebarWidths",
                    serde_json::json!({ "navSidebar": 300 }),
                )
                .await
                .unwrap();
            store
                .patch_section(
                    "jiraConfig",
                    serde_json::json!({ "connected": false }),
                )
                .await
                .unwrap();
            store
                .patch_section(
                    "githubConfig",
                    serde_json::json!({ "connected": true, "owner": "test-org" }),
                )
                .await
                .unwrap();
        }

        // Re-open and verify
        {
            let store = SettingsFileStore::open(path.clone());
            let s = store.get().await;
            assert_eq!(
                s.sidebar_widths.as_ref().and_then(|w| w.nav_sidebar),
                Some(300)
            );
            assert_eq!(s.jira_config.as_ref().map(|j| j.connected), Some(false));
            assert_eq!(
                s.github_config.as_ref().and_then(|g| g.owner.as_deref()),
                Some("test-org")
            );
        }

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
