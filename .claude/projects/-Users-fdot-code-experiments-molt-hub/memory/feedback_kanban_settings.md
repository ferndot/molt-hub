---
name: feedback_kanban_settings
description: Kanban column config belongs on the board view, not main settings. Columns need behavior + hook configuration.
type: feedback
---

Kanban column configuration should live with the Board view, not in the main Settings page.

**Why:** Settings page is for global config (integrations, appearance). Column config is board-specific — it's part of the board's workflow, not app preferences. User wants to configure column behavior and hooks alongside the column definition.

**How to apply:** Move the column editor out of Settings > General and into the Board view (e.g., a gear icon or "Configure Columns" panel on the board itself). Each column should support: title, stage matching, color, ordering, AND behavior config (e.g., WIP limits, auto-transitions) and hook bindings (on_enter, on_exit hooks per column/stage).
