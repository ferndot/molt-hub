//! SQLite-backed store for per-board pipeline configuration.
//!
//! Each board owns exactly one pipeline config, stored as a JSON blob.
//! The store is keyed by `(project_id, board_id)` and replaces the old
//! per-board `{board_id}.yaml` file approach.

use molt_hub_core::config::PipelineConfig;
use sqlx::SqlitePool;
use tracing::warn;

// ---------------------------------------------------------------------------
// PipelineConfigStore
// ---------------------------------------------------------------------------

/// SQLite-backed store for per-board [`PipelineConfig`] records.
///
/// All methods are async and take `&self`; interior mutability is handled by
/// the connection pool.
pub struct PipelineConfigSqliteStore {
    pool: SqlitePool,
}

impl PipelineConfigSqliteStore {
    /// Connect to the given pool and ensure the `pipeline_configs` table exists.
    ///
    /// Safe to call multiple times — uses `CREATE TABLE IF NOT EXISTS`.
    pub async fn new(pool: SqlitePool) -> Result<Self, sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS pipeline_configs (
                board_id    TEXT NOT NULL,
                project_id  TEXT NOT NULL,
                config_json TEXT NOT NULL,
                updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                PRIMARY KEY (project_id, board_id)
            )",
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }

    /// Retrieve the pipeline config for `(project_id, board_id)`.
    ///
    /// Returns `None` when no record exists for the given key pair.
    pub async fn get(
        &self,
        project_id: &str,
        board_id: &str,
    ) -> Option<PipelineConfig> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT config_json FROM pipeline_configs
             WHERE project_id = ?1 AND board_id = ?2",
        )
        .bind(project_id)
        .bind(board_id)
        .fetch_optional(&self.pool)
        .await
        .ok()?;

        let (json,) = row?;
        match serde_json::from_str::<PipelineConfig>(&json) {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                warn!(
                    project_id = %project_id,
                    board_id = %board_id,
                    error = %e,
                    "failed to deserialize pipeline config JSON"
                );
                None
            }
        }
    }

    /// Persist or replace the pipeline config for `(project_id, board_id)`.
    ///
    /// Uses `INSERT OR REPLACE` so a second call overwrites the first without error.
    pub async fn set(
        &self,
        project_id: &str,
        board_id: &str,
        config: &PipelineConfig,
    ) -> Result<(), sqlx::Error> {
        let json = serde_json::to_string(config)
            .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;

        sqlx::query(
            "INSERT OR REPLACE INTO pipeline_configs (board_id, project_id, config_json, updated_at)
             VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
        )
        .bind(board_id)
        .bind(project_id)
        .bind(json)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Delete the pipeline config for `(project_id, board_id)`.
    ///
    /// Returns `Ok(())` even when the record did not exist (idempotent).
    pub async fn delete(
        &self,
        project_id: &str,
        board_id: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "DELETE FROM pipeline_configs WHERE project_id = ?1 AND board_id = ?2",
        )
        .bind(project_id)
        .bind(board_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Startup migration: for every stored board config, copy hooks from `board_defaults()`
    /// into any stage whose hook list is empty, matched by stage name.
    ///
    /// This upgrades configs that were created before default hooks were introduced.
    pub async fn migrate_default_hooks(&self) {
        use molt_hub_core::config::PipelineConfig;

        let defaults = PipelineConfig::board_defaults();

        // Fetch all (project_id, board_id, config_json) rows
        let rows: Vec<(String, String, String)> = match sqlx::query_as(
            "SELECT project_id, board_id, config_json FROM pipeline_configs",
        )
        .fetch_all(&self.pool)
        .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("migrate_default_hooks: failed to fetch rows: {e}");
                return;
            }
        };

        for (project_id, board_id, json) in rows {
            let mut cfg = match serde_json::from_str::<PipelineConfig>(&json) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(
                        project_id = %project_id,
                        board_id = %board_id,
                        error = %e,
                        "migrate_default_hooks: skipping board with unparseable config"
                    );
                    continue;
                }
            };

            let mut changed = false;
            for stage in &mut cfg.stages {
                if stage.hooks.is_empty() {
                    if let Some(default_stage) = defaults.stages.iter().find(|s| s.name == stage.name) {
                        if !default_stage.hooks.is_empty() {
                            stage.hooks = default_stage.hooks.clone();
                            changed = true;
                            tracing::info!(
                                project_id = %project_id,
                                board_id = %board_id,
                                stage = %stage.name,
                                "migrate_default_hooks: patched {} hook(s) into stage",
                                stage.hooks.len()
                            );
                        }
                    }
                }
            }

            if changed {
                if let Err(e) = self.set(&project_id, &board_id, &cfg).await {
                    tracing::warn!(
                        project_id = %project_id,
                        board_id = %board_id,
                        error = %e,
                        "migrate_default_hooks: failed to save patched config"
                    );
                }
            }
        }
    }

    /// Migration shim: if no record exists in SQLite for `(project_id, board_id)`,
    /// attempt to load from the legacy YAML file path and seed the DB.
    ///
    /// Returns the loaded config, or `None` if the YAML file does not exist or
    /// cannot be parsed.
    pub async fn migrate_from_yaml_if_absent(
        &self,
        project_id: &str,
        board_id: &str,
    ) -> Option<PipelineConfig> {
        // Only attempt migration when SQLite has no record.
        if self.get(project_id, board_id).await.is_some() {
            return None; // already in SQLite, no migration needed
        }

        let yaml_path = dirs::config_dir()?
            .join("molt-hub")
            .join("boards")
            .join(project_id)
            .join(format!("{board_id}.yaml"));

        let contents = std::fs::read_to_string(&yaml_path).ok()?;
        let cfg = PipelineConfig::from_yaml(&contents)
            .map_err(|e| {
                warn!(
                    path = %yaml_path.display(),
                    error = %e,
                    "migration: failed to parse legacy YAML"
                );
            })
            .ok()?;

        if let Err(e) = self.set(project_id, board_id, &cfg).await {
            warn!(
                project_id = %project_id,
                board_id = %board_id,
                error = %e,
                "migration: failed to seed SQLite from YAML"
            );
        }

        Some(cfg)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use molt_hub_core::config::{PipelineConfig, StageDefinition};
    use sqlx::sqlite::SqlitePoolOptions;

    async fn in_memory_store() -> PipelineConfigSqliteStore {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        PipelineConfigSqliteStore::new(pool).await.expect("store init")
    }

    fn minimal_config(name: &str) -> PipelineConfig {
        PipelineConfig {
            name: name.to_string(),
            description: None,
            version: 1,
            stages: vec![StageDefinition {
                name: "backlog".into(),
                label: Some("Backlog".into()),
                instructions: None,
                instructions_template: None,
                requires_approval: false,
                approvers: vec![],
                timeout_seconds: None,
                terminal: false,
                hooks: vec![],
                transition_rules: vec![],
                color: Some("#94a3b8".into()),
                order: 0,
                wip_limit: None,
            }],
            integrations: vec![],
            columns: vec![],
        }
    }

    // ── Test 1: set then get returns the same config ──────────────────────

    #[tokio::test]
    async fn set_then_get_returns_same_config() {
        let store = in_memory_store().await;
        let cfg = minimal_config("My Pipeline");

        store.set("proj-1", "board-1", &cfg).await.expect("set");

        let result = store.get("proj-1", "board-1").await;
        assert!(result.is_some());
        let got = result.unwrap();
        assert_eq!(got.name, "My Pipeline");
        assert_eq!(got.version, 1);
        assert_eq!(got.stages.len(), 1);
        assert_eq!(got.stages[0].name, "backlog");
    }

    // ── Test 2: get for unknown board_id returns None ─────────────────────

    #[tokio::test]
    async fn get_unknown_board_returns_none() {
        let store = in_memory_store().await;
        let result = store.get("proj-1", "nonexistent-board").await;
        assert!(result.is_none());
    }

    // ── Test 3: set is idempotent (second write overwrites, no error) ─────

    #[tokio::test]
    async fn set_is_idempotent_second_write_overwrites() {
        let store = in_memory_store().await;
        let cfg_v1 = minimal_config("Version One");
        let cfg_v2 = minimal_config("Version Two");

        store.set("proj-1", "board-1", &cfg_v1).await.expect("first set");
        store.set("proj-1", "board-1", &cfg_v2).await.expect("second set should not error");

        let result = store.get("proj-1", "board-1").await.expect("should exist");
        assert_eq!(result.name, "Version Two", "second write should overwrite first");
    }

    // ── Test 4: delete removes the record; subsequent get returns None ────

    #[tokio::test]
    async fn delete_removes_record() {
        let store = in_memory_store().await;
        let cfg = minimal_config("To Delete");

        store.set("proj-1", "board-del", &cfg).await.expect("set");
        assert!(store.get("proj-1", "board-del").await.is_some());

        store.delete("proj-1", "board-del").await.expect("delete");
        assert!(store.get("proj-1", "board-del").await.is_none());
    }

    // ── Test 5: delete on non-existent board is a no-op (no error) ────────

    #[tokio::test]
    async fn delete_nonexistent_is_noop() {
        let store = in_memory_store().await;
        // Should not return an error when the record does not exist.
        store
            .delete("proj-1", "never-existed")
            .await
            .expect("delete missing should not error");
    }

    // ── Test 6: config round-trips cleanly (all fields preserved) ─────────

    #[tokio::test]
    async fn config_round_trips_all_fields() {
        let store = in_memory_store().await;
        let cfg = PipelineConfig::board_defaults();

        store.set("proj-rt", "board-rt", &cfg).await.expect("set");
        let got = store.get("proj-rt", "board-rt").await.expect("get");

        assert_eq!(got.name, cfg.name);
        assert_eq!(got.version, cfg.version);
        assert_eq!(got.stages.len(), cfg.stages.len());
        assert_eq!(got.columns.len(), cfg.columns.len());

        // Spot-check a few stage fields
        let orig_stage = &cfg.stages[2]; // in-progress
        let got_stage = &got.stages[2];
        assert_eq!(got_stage.name, orig_stage.name);
        assert_eq!(got_stage.wip_limit, orig_stage.wip_limit);
        assert_eq!(got_stage.color, orig_stage.color);
        assert_eq!(got_stage.order, orig_stage.order);

        // Spot-check a column
        let orig_col = &cfg.columns[0];
        let got_col = &got.columns[0];
        assert_eq!(got_col.id, orig_col.id);
        assert_eq!(got_col.title, orig_col.title);
        assert_eq!(got_col.color, orig_col.color);
    }

    // ── Test 7: get is scoped to project_id ──────────────────────────────

    #[tokio::test]
    async fn get_is_scoped_to_project_id() {
        let store = in_memory_store().await;
        let cfg_a = minimal_config("Project A Config");
        let cfg_b = minimal_config("Project B Config");

        store.set("proj-a", "shared-board-id", &cfg_a).await.expect("set proj-a");
        store.set("proj-b", "shared-board-id", &cfg_b).await.expect("set proj-b");

        let result_a = store.get("proj-a", "shared-board-id").await.expect("get proj-a");
        let result_b = store.get("proj-b", "shared-board-id").await.expect("get proj-b");
        let result_c = store.get("proj-c", "shared-board-id").await;

        assert_eq!(result_a.name, "Project A Config");
        assert_eq!(result_b.name, "Project B Config");
        assert!(result_c.is_none(), "unknown project_id should return None");
    }

    // ── Bonus: table creation is idempotent ────────────────────────────────

    #[tokio::test]
    async fn new_is_idempotent() {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("pool");
        let _s1 = PipelineConfigSqliteStore::new(pool.clone()).await.expect("first init");
        let _s2 = PipelineConfigSqliteStore::new(pool).await.expect("second init should not error");
    }
}
