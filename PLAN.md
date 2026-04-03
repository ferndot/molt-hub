# Plan: Test Search Web and Read Tools

## Objective

Verify that the `WebSearch` and `WebFetch` deferred tools are available and functional in the current agent environment.

## Background

These tools appear in the deferred tool list at session start:
- `WebSearch` — queries the web and returns results
- `WebFetch` — fetches and reads the content of a URL

This is a smoke-test to confirm both tools activate and return usable results.

## Implementation Steps

1. **Fetch the WebSearch and WebFetch tool schemas** via `ToolSearch` with `select:WebSearch,WebFetch` to confirm they are resolvable.

2. **Call WebSearch** with a simple, low-ambiguity query (e.g., `"Rust programming language"`) and verify:
   - Tool activates without error
   - Response contains at least one result with a title and URL

3. **Call WebFetch** on a stable, well-known URL (e.g., `https://www.rust-lang.org`) and verify:
   - Tool activates without error
   - Response contains readable HTML/text content

4. **Report results** — summarize what each tool returned, note any errors or unexpected behavior.

## Risks / Unknowns

- Tools may be gated behind user permission approval at runtime.
- Network access may be restricted in the current environment.
- No changes to the codebase are required; this is purely an agent-environment smoke test.

## Complexity

**Small** — two tool calls plus reporting. No code changes.
