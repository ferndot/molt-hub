---
name: feedback_native_ui
description: User wants platform-native UI with system accent color, dark/light theme, system fonts
type: feedback
---

UI should look as platform-native as possible.

**Why:** User values native feel over custom design. Developer tool aesthetic — information-dense, not flashy.

**How to apply:** Use system font stack (-apple-system, system-ui), CSS prefers-color-scheme for auto dark/light, accent-color for form controls, color-scheme: light dark on :root. No manual theme toggle. Priority colors (P0-P3) stay constant across themes.
