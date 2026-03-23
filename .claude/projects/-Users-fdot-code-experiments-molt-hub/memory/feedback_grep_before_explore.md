---
name: Grep exact UI text before exploring
description: When user reports a UI bug with specific visible text, grep for that text first before dispatching explore agents
type: feedback
---

When the user describes a UI bug with specific visible text (like "in-progress pill"), grep for that exact string in the UI source before dispatching explore agents. Exact text match finds the right component instantly.

**Why:** Dispatched explore agents looking for "status pill" which found the wrong component (TaskCard status dot instead of UnifiedCard stage chip). The user's text "in-progress" was a stage name, not a status name — a simple grep would have found UnifiedCard.tsx line 120 immediately.

**How to apply:** For any UI bug report containing quoted text or specific labels, run `grep -r "<exact text>"` in the UI source directory as the first step. Only fall back to semantic exploration if the exact text isn't found.
