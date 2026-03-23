---
name: sprint1_workflow
description: Sprint workflow patterns — worktree isolation, parallel dispatch, merge ordering, dependency handling
type: project
---

## Sprint Workflow Patterns

**Parallel worktree dispatch**: 3 concurrent agents is the reliable sweet spot. All Wave 2 and Wave 3 sprints used this pattern with zero merge conflicts requiring manual intervention.

**Merge ordering**: When multiple agents touch the same crate (e.g., T18 + T25 both adding server modules), merge the one with fewer `lib.rs` changes first. Git's ort strategy handles new-file additions cleanly.

**Serial dependencies**: T46 depended on T22 (needed SolidJS scaffold to exist). Correctly sequenced as serial-after-parallel. Don't force-parallelize when there's a real data dependency.

**Pre-dispatch validation**: Always run `cargo check` and `cargo test` before dispatching. Agents that start from a broken baseline waste cycles.

**Agent commit discipline**: All agents committed successfully when given explicit commit trailer instructions. The worktree-commit-merge pattern is reliable.

**Fix rate**: 0% across Wave 3 (4 feat commits, 0 fix commits). Agents using learnings from prior waves produce correct code on first try.

**Tester gap**: Tester agent hasn't been dispatched through 3 waves. Should activate in Wave 4 for integration testing of the UI views.
