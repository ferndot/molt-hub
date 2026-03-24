//! Git worktree lifecycle — create, mount, and clean up isolated worktrees per agent task.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use thiserror::Error;
use tokio::process::Command;

use molt_hub_core::model::{AgentId, TaskId};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum WorktreeError {
    #[error("path is not a git repository")]
    NotAGitRepo,

    #[error("worktree already exists for task {0}")]
    WorktreeAlreadyExists(TaskId),

    #[error("worktree not found for task {0}")]
    WorktreeNotFound(TaskId),

    #[error("worktree already exists for agent {0}")]
    AgentWorktreeAlreadyExists(AgentId),

    #[error("worktree not found for agent {0}")]
    AgentWorktreeNotFound(AgentId),

    #[error("git command `{command}` failed: {stderr}")]
    GitCommandFailed { command: String, stderr: String },

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// WorktreeConfig
// ---------------------------------------------------------------------------

/// Configuration for the worktree manager.
#[derive(Debug, Clone)]
pub struct WorktreeConfig {
    /// Directory under which worktrees will be created (e.g. `.molt/worktrees/`).
    pub base_dir: PathBuf,
    /// The main branch from which new task branches are cut.
    pub main_branch: String,
    /// Prefix applied to every generated branch name.
    pub branch_prefix: String,
}

impl Default for WorktreeConfig {
    fn default() -> Self {
        Self {
            base_dir: PathBuf::from(".molt/worktrees"),
            main_branch: "main".to_string(),
            branch_prefix: "molt/".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// WorktreeInfo
// ---------------------------------------------------------------------------

/// Describes an active worktree that was created for a task.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub task_id: TaskId,
    /// Absolute path to the worktree on disk.
    pub path: PathBuf,
    /// The git branch created for this worktree.
    pub branch_name: String,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// AgentWorktreeInfo
// ---------------------------------------------------------------------------

/// Describes an active worktree created for an agent process.
#[derive(Debug, Clone)]
pub struct AgentWorktreeInfo {
    pub agent_id: AgentId,
    /// Absolute path to the worktree on disk.
    pub path: PathBuf,
    /// The git branch created for this worktree.
    pub branch_name: String,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// WorktreeManager
// ---------------------------------------------------------------------------

/// Manages git worktrees for concurrent agent tasks.
pub struct WorktreeManager {
    pub config: WorktreeConfig,
    /// Absolute path to the main repository root.
    pub repo_root: PathBuf,
    active: DashMap<TaskId, WorktreeInfo>,
    /// Agent-keyed worktrees (parallel to `active` for agent-based API).
    agent_worktrees: DashMap<AgentId, AgentWorktreeInfo>,
}

impl WorktreeManager {
    /// Create a new manager. Returns `WorktreeError::NotAGitRepo` if `repo_root`
    /// does not contain a `.git` directory or file.
    pub fn new(repo_root: PathBuf, config: WorktreeConfig) -> Result<Self, WorktreeError> {
        // Validate synchronously — we just check for `.git` presence.
        let git_path = repo_root.join(".git");
        if !git_path.exists() {
            return Err(WorktreeError::NotAGitRepo);
        }
        Ok(Self {
            config,
            repo_root,
            active: DashMap::new(),
            agent_worktrees: DashMap::new(),
        })
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Create a new worktree for `task_id`.
    ///
    /// This:
    /// 1. Generates a branch name from the task id.
    /// 2. Creates a new branch from `main_branch`.
    /// 3. Creates the worktree at `{base_dir}/{task_id}/`.
    /// 4. Inserts the entry in the active map.
    pub async fn create(&self, task_id: TaskId) -> Result<WorktreeInfo, WorktreeError> {
        if self.active.contains_key(&task_id) {
            return Err(WorktreeError::WorktreeAlreadyExists(task_id));
        }

        let branch_name = self.branch_name_for(&task_id);
        let worktree_path = self
            .config
            .base_dir
            .join(sanitize_task_id(&task_id.to_string()));

        // Make the path absolute relative to repo_root when it is not already.
        let worktree_path = if worktree_path.is_absolute() {
            worktree_path
        } else {
            self.repo_root.join(&worktree_path)
        };

        // Create the branch from main.
        run_git(
            &self.repo_root,
            &["branch", &branch_name, &self.config.main_branch],
        )
        .await?;

        // Create the worktree.
        run_git(
            &self.repo_root,
            &[
                "worktree",
                "add",
                worktree_path.to_str().unwrap_or_default(),
                &branch_name,
            ],
        )
        .await?;

        let info = WorktreeInfo {
            task_id: task_id.clone(),
            path: worktree_path,
            branch_name,
            created_at: Utc::now(),
        };

        self.active.insert(task_id, info.clone());
        Ok(info)
    }

    /// Look up the worktree info for a task.
    pub fn get(&self, task_id: &TaskId) -> Option<WorktreeInfo> {
        self.active.get(task_id).map(|r| r.clone())
    }

    /// Remove the worktree and optionally delete the branch.
    ///
    /// Runs `git worktree remove <path>` and `git branch -D <branch>`.
    pub async fn remove(&self, task_id: &TaskId) -> Result<(), WorktreeError> {
        let info = self
            .active
            .get(task_id)
            .map(|r| r.clone())
            .ok_or_else(|| WorktreeError::WorktreeNotFound(task_id.clone()))?;

        // Remove the worktree (force to handle dirty state).
        run_git(
            &self.repo_root,
            &[
                "worktree",
                "remove",
                "--force",
                info.path.to_str().unwrap_or_default(),
            ],
        )
        .await?;

        // Delete the branch.
        run_git(&self.repo_root, &["branch", "-D", &info.branch_name]).await?;

        self.active.remove(task_id);
        Ok(())
    }

    /// Return a snapshot of all active worktrees.
    pub fn list(&self) -> Vec<WorktreeInfo> {
        self.active.iter().map(|r| r.clone()).collect()
    }

    /// Prune stale worktrees (directories removed from disk) and return the
    /// task IDs that were cleaned up.
    pub async fn cleanup_stale(&self) -> Result<Vec<TaskId>, WorktreeError> {
        run_git(&self.repo_root, &["worktree", "prune"]).await?;

        let mut removed = Vec::new();
        let stale_ids: Vec<TaskId> = self
            .active
            .iter()
            .filter(|r| !r.path.exists())
            .map(|r| r.task_id.clone())
            .collect();

        for id in stale_ids {
            self.active.remove(&id);
            removed.push(id);
        }
        Ok(removed)
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn branch_name_for(&self, task_id: &TaskId) -> String {
        format!(
            "{}{}",
            self.config.branch_prefix,
            sanitize_task_id(&task_id.to_string())
        )
    }

    // -----------------------------------------------------------------------
    // Agent-based API
    // -----------------------------------------------------------------------

    /// Create a new worktree for an agent process.
    ///
    /// - Path: `{base_dir}/{agent_id}/`
    /// - Branch: `agent/{agent_id}`
    /// - Based on `base_branch` (defaults to `main_branch` from config).
    pub async fn create_for_agent(
        &self,
        agent_id: AgentId,
        base_branch: Option<&str>,
    ) -> Result<AgentWorktreeInfo, WorktreeError> {
        if self.agent_worktrees.contains_key(&agent_id) {
            return Err(WorktreeError::AgentWorktreeAlreadyExists(agent_id));
        }

        let id_str = sanitize_task_id(&agent_id.to_string());
        let branch_name = format!("agent/{}", id_str);
        let base = base_branch.unwrap_or(&self.config.main_branch);

        let worktree_path = self.config.base_dir.join(&id_str);
        let worktree_path = if worktree_path.is_absolute() {
            worktree_path
        } else {
            self.repo_root.join(&worktree_path)
        };

        // Create the branch from base.
        run_git(&self.repo_root, &["branch", &branch_name, base]).await?;

        // Create the worktree.
        run_git(
            &self.repo_root,
            &[
                "worktree",
                "add",
                worktree_path.to_str().unwrap_or_default(),
                &branch_name,
            ],
        )
        .await?;

        let info = AgentWorktreeInfo {
            agent_id: agent_id.clone(),
            path: worktree_path,
            branch_name,
            created_at: Utc::now(),
        };

        self.agent_worktrees.insert(agent_id, info.clone());
        Ok(info)
    }

    /// Look up the worktree info for an agent.
    pub fn get_agent_worktree(&self, agent_id: &AgentId) -> Option<AgentWorktreeInfo> {
        self.agent_worktrees.get(agent_id).map(|r| r.clone())
    }

    /// Remove the worktree and branch for an agent.
    pub async fn remove_agent_worktree(&self, agent_id: &AgentId) -> Result<(), WorktreeError> {
        let info = self
            .agent_worktrees
            .get(agent_id)
            .map(|r| r.clone())
            .ok_or_else(|| WorktreeError::AgentWorktreeNotFound(agent_id.clone()))?;

        // Remove the worktree (force to handle dirty state).
        run_git(
            &self.repo_root,
            &[
                "worktree",
                "remove",
                "--force",
                info.path.to_str().unwrap_or_default(),
            ],
        )
        .await?;

        // Delete the branch.
        run_git(&self.repo_root, &["branch", "-D", &info.branch_name]).await?;

        self.agent_worktrees.remove(agent_id);
        Ok(())
    }

    /// Return a snapshot of all active agent worktrees.
    pub fn list_agent_worktrees(&self) -> Vec<AgentWorktreeInfo> {
        self.agent_worktrees.iter().map(|r| r.clone()).collect()
    }
}

// ---------------------------------------------------------------------------
// Standalone validation
// ---------------------------------------------------------------------------

/// Check whether `path` is a valid git repository.
///
/// A path is considered valid when it contains a `.git` entry (either a
/// directory for regular repos or a file for worktrees / submodules).
///
/// Returns `Ok(())` on success, or [`WorktreeError::NotAGitRepo`] when the
/// check fails.
pub fn validate_repo(path: &std::path::Path) -> Result<(), WorktreeError> {
    let git_path = path.join(".git");
    if git_path.exists() {
        Ok(())
    } else {
        Err(WorktreeError::NotAGitRepo)
    }
}

// ---------------------------------------------------------------------------
// Git helper
// ---------------------------------------------------------------------------

/// Run a git command in `repo_root`. Returns trimmed stdout on success,
/// or `WorktreeError::GitCommandFailed` on a non-zero exit code.
pub async fn run_git(repo_root: &Path, args: &[&str]) -> Result<String, WorktreeError> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(args)
        .output()
        .await?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(WorktreeError::GitCommandFailed {
            command: format!("git {}", args.join(" ")),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Sanitization
// ---------------------------------------------------------------------------

/// Replace any character that is not alphanumeric, `-`, or `_` with `-`.
/// Used to turn a ULID string (which may contain only valid chars already)
/// into a valid git branch-name component.
fn sanitize_task_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Create a temporary directory with a real git repo inside.
    fn make_temp_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        // Init the repo.
        let status = StdCommand::new("git")
            .args(["init", "-b", "main"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(status.success(), "git init failed");

        // Configure user (required for commits in CI environments).
        StdCommand::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        StdCommand::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .status()
            .unwrap();

        // Make an initial commit so that branches can be cut.
        let readme = dir.path().join("README.md");
        std::fs::write(&readme, "# test\n").unwrap();
        StdCommand::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .status()
            .unwrap();
        StdCommand::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir.path())
            .status()
            .unwrap();

        dir
    }

    fn config_for(base: &Path) -> WorktreeConfig {
        WorktreeConfig {
            base_dir: base.join("worktrees"),
            main_branch: "main".to_string(),
            branch_prefix: "molt/".to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // sanitize_task_id
    // -----------------------------------------------------------------------

    #[test]
    fn sanitize_preserves_valid_chars() {
        assert_eq!(sanitize_task_id("abc-123_XYZ"), "abc-123_XYZ");
    }

    #[test]
    fn sanitize_replaces_invalid_chars() {
        assert_eq!(sanitize_task_id("task/1 2.3"), "task-1-2-3");
    }

    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    #[test]
    fn new_rejects_non_git_dir() {
        let dir = TempDir::new().unwrap();
        let cfg = config_for(dir.path());
        let result = WorktreeManager::new(dir.path().to_path_buf(), cfg);
        assert!(matches!(result, Err(WorktreeError::NotAGitRepo)));
    }

    #[test]
    fn new_accepts_git_repo() {
        let repo = make_temp_repo();
        let cfg = config_for(repo.path());
        let result = WorktreeManager::new(repo.path().to_path_buf(), cfg);
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // Lifecycle: create / get / list / remove
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn create_get_remove_lifecycle() {
        let repo = make_temp_repo();
        let cfg = config_for(repo.path());
        let mgr = WorktreeManager::new(repo.path().to_path_buf(), cfg).unwrap();

        let task_id = TaskId::new();

        // Create
        let info = mgr.create(task_id.clone()).await.unwrap();
        assert!(info.path.exists(), "worktree directory should exist");
        assert!(info.branch_name.starts_with("molt/"));

        // Get
        let fetched = mgr.get(&task_id).unwrap();
        assert_eq!(fetched.branch_name, info.branch_name);

        // List
        let list = mgr.list();
        assert_eq!(list.len(), 1);

        // Remove
        mgr.remove(&task_id).await.unwrap();
        assert!(!info.path.exists(), "worktree directory should be gone");
        assert!(mgr.get(&task_id).is_none());
        assert!(mgr.list().is_empty());
    }

    // -----------------------------------------------------------------------
    // Duplicate creation
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn create_duplicate_returns_error() {
        let repo = make_temp_repo();
        let cfg = config_for(repo.path());
        let mgr = WorktreeManager::new(repo.path().to_path_buf(), cfg).unwrap();

        let task_id = TaskId::new();
        mgr.create(task_id.clone()).await.unwrap();

        let second = mgr.create(task_id.clone()).await;
        assert!(
            matches!(second, Err(WorktreeError::WorktreeAlreadyExists(_))),
            "expected WorktreeAlreadyExists"
        );

        // Cleanup
        mgr.remove(&task_id).await.unwrap();
    }

    // -----------------------------------------------------------------------
    // cleanup_stale
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Agent-based lifecycle: create / get / list / remove
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn agent_create_get_remove_lifecycle() {
        let repo = make_temp_repo();
        let cfg = config_for(repo.path());
        let mgr = WorktreeManager::new(repo.path().to_path_buf(), cfg).unwrap();

        let agent_id = AgentId::new();

        // Create
        let info = mgr.create_for_agent(agent_id.clone(), None).await.unwrap();
        assert!(info.path.exists(), "agent worktree directory should exist");
        assert!(info.branch_name.starts_with("agent/"));

        // Get
        let fetched = mgr.get_agent_worktree(&agent_id).unwrap();
        assert_eq!(fetched.branch_name, info.branch_name);

        // List
        let list = mgr.list_agent_worktrees();
        assert_eq!(list.len(), 1);

        // Remove
        mgr.remove_agent_worktree(&agent_id).await.unwrap();
        assert!(
            !info.path.exists(),
            "agent worktree directory should be gone"
        );
        assert!(mgr.get_agent_worktree(&agent_id).is_none());
        assert!(mgr.list_agent_worktrees().is_empty());
    }

    #[tokio::test]
    async fn agent_create_duplicate_returns_error() {
        let repo = make_temp_repo();
        let cfg = config_for(repo.path());
        let mgr = WorktreeManager::new(repo.path().to_path_buf(), cfg).unwrap();

        let agent_id = AgentId::new();
        mgr.create_for_agent(agent_id.clone(), None).await.unwrap();

        let second = mgr.create_for_agent(agent_id.clone(), None).await;
        assert!(
            matches!(second, Err(WorktreeError::AgentWorktreeAlreadyExists(_))),
            "expected AgentWorktreeAlreadyExists"
        );

        // Cleanup
        mgr.remove_agent_worktree(&agent_id).await.unwrap();
    }

    #[tokio::test]
    async fn agent_create_with_custom_base_branch() {
        let repo = make_temp_repo();
        let cfg = config_for(repo.path());
        let mgr = WorktreeManager::new(repo.path().to_path_buf(), cfg).unwrap();

        let agent_id = AgentId::new();
        // Use "main" explicitly as base_branch.
        let info = mgr
            .create_for_agent(agent_id.clone(), Some("main"))
            .await
            .unwrap();
        assert!(info.path.exists());

        mgr.remove_agent_worktree(&agent_id).await.unwrap();
    }

    #[tokio::test]
    async fn agent_remove_nonexistent_returns_error() {
        let repo = make_temp_repo();
        let cfg = config_for(repo.path());
        let mgr = WorktreeManager::new(repo.path().to_path_buf(), cfg).unwrap();

        let agent_id = AgentId::new();
        let err = mgr.remove_agent_worktree(&agent_id).await;
        assert!(
            matches!(err, Err(WorktreeError::AgentWorktreeNotFound(_))),
            "expected AgentWorktreeNotFound"
        );
    }

    // -----------------------------------------------------------------------
    // cleanup_stale (task-based)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn cleanup_stale_removes_missing_paths() {
        let repo = make_temp_repo();
        let cfg = config_for(repo.path());
        let mgr = WorktreeManager::new(repo.path().to_path_buf(), cfg).unwrap();

        let task_id = TaskId::new();
        let info = mgr.create(task_id.clone()).await.unwrap();

        // Manually remove the directory to simulate a stale worktree.
        // First remove it from git, then delete directory.
        run_git(
            repo.path(),
            &["worktree", "remove", "--force", info.path.to_str().unwrap()],
        )
        .await
        .unwrap();

        // The active map still thinks it's there; cleanup_stale should fix it.
        let cleaned = mgr.cleanup_stale().await.unwrap();
        assert!(cleaned.contains(&task_id));
        assert!(mgr.get(&task_id).is_none());

        // Branch cleanup (best-effort, ignore error).
        let _ = run_git(repo.path(), &["branch", "-D", &info.branch_name]).await;
    }
}
