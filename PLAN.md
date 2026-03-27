# Plan: AI Tutor Chat — Initial Suggestions

**Ticket**: [Shaping needed] AI Tutor chat - initial suggestions
**Priority**: P2
**Complexity**: Medium (new component required)
**Status**: Architecture decided — ready for implementation

---

## Context

The codebase has a `SteerChat` component (`ui/src/views/AgentDetail/SteerChat.tsx`) that already
implements the `complete` / `partial` suggestion pattern described in the ticket:

- `SuggestionKind = "complete" | "partial"` types exist
- `Suggestion` interface (`{ kind, text, value? }`) exists
- Suggestion click logic works: complete → auto-send, partial → populate input + focus
- `FaSolidCommentAlt` icon imported and used
- Suggestion button styling (`suggestionBtn`, `suggestionIcon`) in `SteerChat.module.css`

**Confirmed**: No `AiTutor` directory or `AiTutorChat` component exists anywhere in `ui/src`.
This means a new component must be created — we cannot simply pass a `suggestions` prop to an
existing AI Tutor chat. The work is a new component, scaffolded from SteerChat patterns.

`SteerChat` is also agent-steering–specific (priority levels, "urgent" mode, Shift+Enter shortcut)
and should not be repurposed directly for student-facing educational chat.

---

## Suggestion Architecture (Decided)

Suggestions work in two modes:

### Mode 1 — Initial state (predefined)
When the chat has no messages, a static list of predefined suggestions is shown. These are
constant, require no backend, and guide the student toward the kinds of questions they can ask.

**Predefined list (mix of partial and complete):**
- `{ kind: "partial",   text: "Find the lesson that covers…" }`
- `{ kind: "partial",   text: "Can you explain…" }`
- `{ kind: "partial",   text: "What is the difference between…" }`
- `{ kind: "partial",   text: "Help me understand why…" }`
- `{ kind: "complete",  text: "What topics should I study next?" }`
- `{ kind: "complete",  text: "Give me a quiz on what I just learned." }`

### Mode 2 — Tool-call-driven (agent-surfaced)
After any tutor response, the agent may optionally call a `SuggestFollowups` tool to surface
contextually relevant follow-up suggestions. Suggestions are NOT shown after every message —
only when the agent explicitly calls the tool.

**Tool call format** (matches existing Claude Code text output format parsed by `AgentChat`):
```
⏺ SuggestFollowups({"suggestions": [{"kind": "complete", "text": "..."}, ...]})
  ⎿  (no result / null)
```

**Frontend behaviour:**
1. Parse each output line for the `⏺ SuggestFollowups(...)` pattern (same regex as AgentChat)
2. Extract and parse the JSON argument
3. Attach the resulting `Suggestion[]` to the triggering tutor message in the store
4. Suppress the normal collapsible tool block rendering for this specific tool call
5. Render suggestion buttons below the tutor message instead

---

## Implementation Steps

### Step 1 — Extract shared suggestion types

Move `SuggestionKind` and `Suggestion` from `SteerChat.tsx` to a shared module:

- Create `ui/src/types/chat.ts` with the exported types
- Update `SteerChat.tsx` to import from there (re-export for backwards compat)
- New `AiTutorChat` will import from the same shared file

### Step 2 — `tutorStore.ts`

Create `ui/src/views/AiTutor/tutorStore.ts`:

```typescript
export interface TutorMessage {
  id: string;
  role: "student" | "tutor";
  content: string;
  timestamp: string;        // ISO string
  suggestions?: Suggestion[]; // populated when agent calls SuggestFollowups
}

export interface TutorState {
  messages: Record<string, TutorMessage[]>; // keyed by sessionId
  sending: Record<string, boolean>;
}
```

Actions:
- `sendMessage(sessionId, content)` — POST to `/api/tutor/sessions/{id}/messages`, add student message optimistically
- `addTutorMessage(sessionId, content)` — append tutor text response (called from WS output)
- `attachSuggestions(sessionId, messageId, suggestions)` — attach tool-call suggestions to a message
- `getMessages(sessionId)`, `isSending(sessionId)`

### Step 3 — `AiTutorChat.tsx` component

Create `ui/src/views/AiTutor/AiTutorChat.tsx`:

**Props:**
```typescript
interface Props {
  sessionId: string;
  /** Overrides the default predefined initial suggestions. Pass [] to hide. */
  initialSuggestions?: Suggestion[];
}
```

**Behaviour:**
- Empty state: show `initialSuggestions` (default predefined list from Step above)
- Message list: render student messages right, tutor messages left
- After each tutor message: if `msg.suggestions` is non-empty, render suggestion buttons below it
  (only the most recent tutor message's suggestions are interactive; older ones are greyed out)
- Clicking a suggestion: same `handleSuggestionClick` logic as SteerChat (complete → send, partial → populate + focus)
- Input: auto-resizing textarea, Enter to send
- Output line parsing: scan incoming lines for `⏺ SuggestFollowups(...)`, call `attachSuggestions()` and skip normal rendering

**No** priority/urgent mode. **No** Shift+Enter shortcut.

### Step 4 — `AiTutorChat.module.css`

Create co-located styles reusing the same CSS variables as SteerChat:
- `--bg-elevated`, `--border`, `--chrome-accent` for suggestion buttons
- `--text-primary`, `--text-secondary`, `--text-subtle` for text/icons
- Verify spacing against Figma (ticket links Figma flows for both suggestion kinds)
- Note: must match "suggested content links" styling — check if that component exists in the codebase first

### Step 5 — `AiTutorView.tsx` + route

Create `ui/src/views/AiTutor/AiTutorView.tsx` as a placeholder page mounting `AiTutorChat`.
Add `/tutor` route in the app router.

### Step 6 — Tests

Unit tests in `ui/src/views/AiTutor/__tests__/aiTutorChat.test.ts`:

- Empty state renders predefined suggestion buttons
- Complete suggestion click → `sendMessage()` called, input stays empty
- Partial suggestion click → input populated with text (minus trailing `…`), textarea focused
- `SuggestFollowups` tool call in output → suggestions attached to triggering message
- Suggestion buttons appear below triggering message
- Only most recent suggestions are interactive (older ones disabled)
- Disabled state during send

---

## Files Expected to Change / Create

| File | Change |
|------|--------|
| `ui/src/types/chat.ts` | **New** — shared `Suggestion` / `SuggestionKind` types |
| `ui/src/views/AgentDetail/SteerChat.tsx` | Import types from `chat.ts` instead of local |
| `ui/src/views/AiTutor/tutorStore.ts` | **New** — message state for tutor sessions |
| `ui/src/views/AiTutor/AiTutorChat.tsx` | **New** — AI Tutor chat component |
| `ui/src/views/AiTutor/AiTutorChat.module.css` | **New** — co-located styles |
| `ui/src/views/AiTutor/AiTutorView.tsx` | **New** — page wrapper |
| `ui/src/views/AiTutor/__tests__/aiTutorChat.test.ts` | **New** — unit tests |
| Router config | Add `/tutor` route |

---

## Risks & Unknowns

| # | Risk | Impact | Mitigation |
|---|------|--------|------------|
| 1 | Backend API endpoint for tutor messages unknown | Blocks tutorStore send | Stub with `POST /api/tutor/sessions/{id}/messages`; can be corrected when backend exists |
| 2 | "Suggested content links" component — may be an existing UI component to match | Wrong styles | Search codebase for this component before Step 4 |
| 3 | Figma designs not reviewed | Spacing/icon mismatch | Check Figma before finalizing CSS |
| 4 | Partial suggestion `…` trimming edge cases | Minor UX bug | Trim both `…` (U+2026) and `...`; SteerChat already does this |
| 5 | SuggestFollowups tool call must not appear in normal tool block rendering | UI clutter | Filter by tool name before delegating to AgentChat parser |

---

## Complexity Assessment

**Medium** — The suggestion logic is proven (copied from SteerChat), but the tool-call parsing,
message attachment, and "only latest suggestions are interactive" behaviour add non-trivial logic.
Full scaffold (component + store + view + route + tests) is required.

---

## Out of Scope (separate tickets)

- Backend AI Tutor API and agent spawn
- Backend `SuggestFollowups` tool definition / system prompt
- Figma review and pixel-perfect spacing pass
