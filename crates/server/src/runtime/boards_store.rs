//! SQLite-backed store for board index records.
//!
//! Each project owns zero or more boards. A `BoardRecord` maps a `board_id` to
//! the on-disk `config_path` for that board's YAML configuration file.
//!
//! This replaces the old `boards-index.yaml` file approach with a proper
//! relational store, keyed by `(project_id, board_id)`.

use sqlx::SqlitePool;

// ---------------------------------------------------------------------------
// BoardRecord
// ---------------------------------------------------------------------------

/// A single board row as returned by [`BoardsStore::list_boards`].
#[derive(Debug, Clone, PartialEq)]
pub struct BoardRecord {
    pub board_id: String,
    pub project_id: String,
    pub config_path: String,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// BoardsStore
// ---------------------------------------------------------------------------

/// SQLite-backed store for board index records.
///
/// All methods are async and take `&self`; interior mutability is handled by
/// the connection pool.
pub struct BoardsStore {
    pool: SqlitePool,
}

impl BoardsStore {
    /// Connect to the given pool and ensure the `boards` table exists.
    ///
    /// Safe to call multiple times — uses `CREATE TABLE IF NOT EXISTS`.
    pub async fn new(pool: SqlitePool) -> Result<Self, sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS boards (
                board_id    TEXT NOT NULL,
                project_id  TEXT NOT NULL,
                config_path TEXT NOT NULL,
                created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                PRIMARY KEY (project_id, board_id)
            )",
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }

    /// Insert a board record.
    ///
    /// If a record with the same `(project_id, board_id)` already exists, this
    /// is a no-op (idempotent). The existing `config_path` and `created_at` are
    /// preserved.
    pub async fn add_board(
        &self,
        project_id: &str,
        board_id: &str,
        config_path: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT OR IGNORE INTO boards (board_id, project_id, config_path)
             VALUES (?1, ?2, ?3)",
        )
        .bind(board_id)
        .bind(project_id)
        .bind(config_path)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Return all boards for the given `project_id`, ordered by `created_at`.
    pub async fn list_boards(&self, project_id: &str) -> Result<Vec<BoardRecord>, sqlx::Error> {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT board_id, project_id, config_path, created_at
             FROM boards
             WHERE project_id = ?1
             ORDER BY created_at ASC",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(board_id, project_id, config_path, created_at)| BoardRecord {
                board_id,
                project_id,
                config_path,
                created_at,
            })
            .collect())
    }

    /// Delete the board record for `(project_id, board_id)`.
    ///
    /// Returns `Ok(())` even when the record did not exist (idempotent).
    pub async fn remove_board(
        &self,
        project_id: &str,
        board_id: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "DELETE FROM boards WHERE project_id = ?1 AND board_id = ?2",
        )
        .bind(project_id)
        .bind(board_id)
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

    async fn in_memory_store() -> BoardsStore {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        BoardsStore::new(pool).await.expect("store init")
    }

    // ── Table creation ────────────────────────────────────────────────────

    #[tokio::test]
    async fn table_is_created_on_startup() {
        let store = in_memory_store().await;
        let boards = store.list_boards("any-project").await.expect("list");
        assert!(boards.is_empty());
    }

    #[tokio::test]
    async fn new_is_idempotent() {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("pool");
        let _s1 = BoardsStore::new(pool.clone()).await.expect("first init");
        let _s2 = BoardsStore::new(pool).await.expect("second init");
    }

    // ── Happy path ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn add_board_then_list_returns_it() {
        let store = in_memory_store().await;
        store
            .add_board("proj-1", "board-abc", "/path/to/board-abc.yaml")
            .await
            .expect("add_board");

        let boards = store.list_boards("proj-1").await.expect("list");
        assert_eq!(boards.len(), 1);
        assert_eq!(boards[0].board_id, "board-abc");
        assert_eq!(boards[0].project_id, "proj-1");
        assert_eq!(boards[0].config_path, "/path/to/board-abc.yaml");
    }

    // ── Empty list ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_boards_returns_empty_for_project_with_no_boards() {
        let store = in_memory_store().await;
        let boards = store.list_boards("empty-project").await.expect("list");
        assert!(boards.is_empty());
    }

    // ── Idempotent add ────────────────────────────────────────────────────

    #[tokio::test]
    async fn add_board_twice_is_idempotent() {
        // INSERT OR IGNORE: second add is silently skipped; no error is returned
        // and the list still contains exactly one record.
        let store = in_memory_store().await;
        store
            .add_board("proj-1", "board-dup", "/path/dup.yaml")
            .await
            .expect("first add");
        store
            .add_board("proj-1", "board-dup", "/path/dup-v2.yaml")
            .await
            .expect("second add should not error");

        let boards = store.list_boards("proj-1").await.expect("list");
        assert_eq!(boards.len(), 1, "duplicate insert must be ignored");
        // Original config_path is preserved (INSERT OR IGNORE keeps original row).
        assert_eq!(boards[0].config_path, "/path/dup.yaml");
    }

    // ── Remove ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn remove_board_removes_only_that_board() {
        let store = in_memory_store().await;
        store
            .add_board("proj-1", "keep-me", "/keep.yaml")
            .await
            .expect("add keep-me");
        store
            .add_board("proj-1", "delete-me", "/delete.yaml")
            .await
            .expect("add delete-me");

        store
            .remove_board("proj-1", "delete-me")
            .await
            .expect("remove");

        let boards = store.list_boards("proj-1").await.expect("list");
        assert_eq!(boards.len(), 1);
        assert_eq!(boards[0].board_id, "keep-me");
    }

    #[tokio::test]
    async fn remove_board_is_idempotent_for_missing_record() {
        let store = in_memory_store().await;
        // Should not error when the record does not exist.
        store
            .remove_board("proj-1", "never-existed")
            .await
            .expect("remove missing");
    }

    // ── Project isolation ─────────────────────────────────────────────────

    #[tokio::test]
    async fn list_boards_is_scoped_to_project_id() {
        let store = in_memory_store().await;
        store
            .add_board("proj-a", "board-1", "/a/board-1.yaml")
            .await
            .expect("add proj-a board");
        store
            .add_board("proj-b", "board-2", "/b/board-2.yaml")
            .await
            .expect("add proj-b board");

        let a_boards = store.list_boards("proj-a").await.expect("list a");
        let b_boards = store.list_boards("proj-b").await.expect("list b");

        assert_eq!(a_boards.len(), 1);
        assert_eq!(a_boards[0].board_id, "board-1");

        assert_eq!(b_boards.len(), 1);
        assert_eq!(b_boards[0].board_id, "board-2");
    }

    // ── config_path round-trip ─────────────────────────────────────────────

    #[tokio::test]
    async fn config_path_is_stored_and_returned_correctly() {
        let store = in_memory_store().await;
        let path = "/Users/alice/.config/molt-hub/boards/proj-x/01ABCDEF.yaml";
        store
            .add_board("proj-x", "01ABCDEF", path)
            .await
            .expect("add");

        let boards = store.list_boards("proj-x").await.expect("list");
        assert_eq!(boards.len(), 1);
        assert_eq!(boards[0].config_path, path);
    }
}
