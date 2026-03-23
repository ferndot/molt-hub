---
name: Small fixes should be direct edits
description: For <10 line fixes touching 1-2 files, edit directly instead of dispatching worktree agents
type: feedback
---

For small fixes (<10 lines, 1-2 files), edit directly in the primary session instead of dispatching worktree agents. The overhead of agent dispatch + worktree creation + merge exceeds the edit time.

**Why:** Two 3-line fixes were dispatched to worktree agents, taking ~60s each when direct edits would have taken ~10s. One was wrong and had to be reverted anyway.

**How to apply:** Reserve worktree dispatch for multi-file changes (3+ files) or changes exceeding ~20 lines. Single-component fixes should be done inline.
