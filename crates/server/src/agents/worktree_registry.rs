//! Typed wrappers for worktree bookkeeping (agent spawn / terminate).

use std::path::PathBuf;
use std::sync::Arc;

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use molt_hub_core::model::AgentId;
use molt_hub_harness::worktree::{WorktreeError, WorktreeManager};

/// Maps each agent that used a git worktree to its repository root (for manager lookup).
#[derive(Debug, Default)]
pub struct WorktreeRegistry {
    agent_repo: DashMap<AgentId, PathBuf>,
}

impl WorktreeRegistry {
    pub fn new() -> Self {
        Self {
            agent_repo: DashMap::new(),
        }
    }

    pub fn record(&self, agent_id: AgentId, repo_root: PathBuf) {
        self.agent_repo.insert(agent_id, repo_root);
    }

    pub fn take_repo_for_agent(&self, agent_id: &AgentId) -> Option<PathBuf> {
        self.agent_repo.remove(agent_id).map(|(_, p)| p)
    }
}

/// One [`WorktreeManager`] per canonical repository root.
#[derive(Default)]
pub struct WorktreeManagerCache {
    inner: DashMap<PathBuf, Arc<WorktreeManager>>,
}

impl WorktreeManagerCache {
    pub fn new() -> Self {
        Self {
            inner: DashMap::new(),
        }
    }

    /// Returns the manager for `repo_root`, creating it if missing.
    pub fn get_or_insert(
        &self,
        repo_root: PathBuf,
        build: impl FnOnce() -> Result<WorktreeManager, WorktreeError>,
    ) -> Result<Arc<WorktreeManager>, WorktreeError> {
        match self.inner.entry(repo_root) {
            Entry::Occupied(o) => Ok(Arc::clone(o.get())),
            Entry::Vacant(v) => {
                let mgr = build()?;
                Ok(Arc::clone(&*v.insert(Arc::new(mgr))))
            }
        }
    }

    pub fn get(&self, repo_root: &PathBuf) -> Option<Arc<WorktreeManager>> {
        self.inner.get(repo_root).map(|r| Arc::clone(&*r))
    }
}
