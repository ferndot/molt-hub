//! SQLite-backed key/value store for user settings.
//!
//! Settings are stored as plain text values under arbitrary string keys.
//! The frontend uses a single key (`app_settings`) with a JSON-serialised
//! value, which keeps the server schema stable regardless of what the UI
//! decides to persist.

use std::collections::HashMap;

use sqlx::SqlitePool;

// ---------------------------------------------------------------------------
// SettingsStore
// ---------------------------------------------------------------------------

/// Persistent, SQLite-backed store for application settings.
///
/// All methods are async and take `&self`; interior mutability is handled by
/// the connection pool.
pub struct SettingsStore {
    pool: SqlitePool,
}

impl SettingsStore {
    /// Connect to the given pool and ensure the `settings` table exists.
    ///
    /// Safe to call multiple times — uses `CREATE TABLE IF NOT EXISTS`.
    pub async fn new(pool: SqlitePool) -> Result<Self, sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS settings (
                key        TEXT PRIMARY KEY,
                value      TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }

    /// Return the stored value for `key`, or `None` if it does not exist.
    pub async fn get(&self, key: &str) -> Result<Option<String>, sqlx::Error> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM settings WHERE key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;

        Ok(row.map(|(v,)| v))
    }

    /// Upsert `value` under `key`.
    ///
    /// Updates `updated_at` to the current UTC timestamp on every write.
    pub async fn set(&self, key: &str, value: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at)
             VALUES (?, ?, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET
                 value      = excluded.value,
                 updated_at = excluded.updated_at",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Return every key/value pair in the settings table.
    pub async fn get_all(&self) -> Result<HashMap<String, String>, sqlx::Error> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT key, value FROM settings")
                .fetch_all(&self.pool)
                .await?;

        Ok(rows.into_iter().collect())
    }

    /// Upsert all entries from `settings` atomically within a single transaction.
    pub async fn set_bulk(&self, settings: &HashMap<String, String>) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        for (key, value) in settings {
            sqlx::query(
                "INSERT INTO settings (key, value, updated_at)
                 VALUES (?, ?, datetime('now'))
                 ON CONFLICT(key) DO UPDATE SET
                     value      = excluded.value,
                     updated_at = excluded.updated_at",
            )
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Delete the entry for `key`.
    ///
    /// Returns `Ok(())` even when the key did not exist (idempotent).
    pub async fn delete(&self, key: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM settings WHERE key = ?")
            .bind(key)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn in_memory_store() -> SettingsStore {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        SettingsStore::new(pool).await.expect("store init")
    }

    // ── Table creation ────────────────────────────────────────────────────

    #[tokio::test]
    async fn table_is_created_on_startup() {
        // If the table were not created, any subsequent query would fail.
        let store = in_memory_store().await;
        let all = store.get_all().await.expect("get_all should succeed");
        assert!(all.is_empty());
    }

    #[tokio::test]
    async fn new_is_idempotent() {
        // Calling `new` twice on the same pool must not fail.
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("pool");
        let _s1 = SettingsStore::new(pool.clone()).await.expect("first init");
        let _s2 = SettingsStore::new(pool).await.expect("second init");
    }

    // ── get / set ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn get_returns_none_for_missing_key() {
        let store = in_memory_store().await;
        let val = store.get("missing").await.expect("get");
        assert!(val.is_none());
    }

    #[tokio::test]
    async fn set_then_get_round_trips_value() {
        let store = in_memory_store().await;
        store.set("theme", "dark").await.expect("set");
        let val = store.get("theme").await.expect("get");
        assert_eq!(val, Some("dark".to_owned()));
    }

    #[tokio::test]
    async fn set_overwrites_existing_value() {
        let store = in_memory_store().await;
        store.set("theme", "light").await.expect("first set");
        store.set("theme", "dark").await.expect("second set");
        let val = store.get("theme").await.expect("get");
        assert_eq!(val, Some("dark".to_owned()));
    }

    #[tokio::test]
    async fn set_stores_json_string() {
        let store = in_memory_store().await;
        let json = r#"{"columns":["todo","doing","done"]}"#;
        store.set("app_settings", json).await.expect("set");
        let val = store.get("app_settings").await.expect("get");
        assert_eq!(val.as_deref(), Some(json));
    }

    // ── get_all ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn get_all_returns_all_entries() {
        let store = in_memory_store().await;
        store.set("a", "1").await.expect("set a");
        store.set("b", "2").await.expect("set b");

        let all = store.get_all().await.expect("get_all");
        assert_eq!(all.len(), 2);
        assert_eq!(all.get("a").map(|s| s.as_str()), Some("1"));
        assert_eq!(all.get("b").map(|s| s.as_str()), Some("2"));
    }

    #[tokio::test]
    async fn get_all_empty_when_no_entries() {
        let store = in_memory_store().await;
        let all = store.get_all().await.expect("get_all");
        assert!(all.is_empty());
    }

    // ── set_bulk ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn set_bulk_inserts_multiple_entries() {
        let store = in_memory_store().await;
        let mut batch = HashMap::new();
        batch.insert("x".to_owned(), "10".to_owned());
        batch.insert("y".to_owned(), "20".to_owned());

        store.set_bulk(&batch).await.expect("set_bulk");

        let all = store.get_all().await.expect("get_all");
        assert_eq!(all.len(), 2);
        assert_eq!(all.get("x").map(|s| s.as_str()), Some("10"));
        assert_eq!(all.get("y").map(|s| s.as_str()), Some("20"));
    }

    #[tokio::test]
    async fn set_bulk_upserts_existing_keys() {
        let store = in_memory_store().await;
        store.set("x", "old").await.expect("pre-seed");

        let mut batch = HashMap::new();
        batch.insert("x".to_owned(), "new".to_owned());
        batch.insert("z".to_owned(), "30".to_owned());

        store.set_bulk(&batch).await.expect("set_bulk");

        assert_eq!(
            store.get("x").await.expect("get").as_deref(),
            Some("new")
        );
        assert_eq!(
            store.get("z").await.expect("get").as_deref(),
            Some("30")
        );
    }

    #[tokio::test]
    async fn set_bulk_empty_map_is_noop() {
        let store = in_memory_store().await;
        store.set("k", "v").await.expect("pre-seed");
        store.set_bulk(&HashMap::new()).await.expect("empty bulk");

        let all = store.get_all().await.expect("get_all");
        assert_eq!(all.len(), 1);
    }

    // ── delete ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn delete_removes_existing_key() {
        let store = in_memory_store().await;
        store.set("del_me", "val").await.expect("set");
        store.delete("del_me").await.expect("delete");

        let val = store.get("del_me").await.expect("get");
        assert!(val.is_none());
    }

    #[tokio::test]
    async fn delete_is_idempotent_for_missing_key() {
        let store = in_memory_store().await;
        // Should not error even when the key doesn't exist.
        store.delete("never_existed").await.expect("delete");
    }

    #[tokio::test]
    async fn delete_only_removes_targeted_key() {
        let store = in_memory_store().await;
        store.set("keep", "yes").await.expect("set keep");
        store.set("gone", "no").await.expect("set gone");

        store.delete("gone").await.expect("delete");

        let all = store.get_all().await.expect("get_all");
        assert_eq!(all.len(), 1);
        assert!(all.contains_key("keep"));
    }
}
