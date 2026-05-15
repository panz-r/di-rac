# Extension Authoring Patterns — Condensed

**18 patterns from Pi's community. See individual files for full detail.**

## Tool Patterns

Minimal — `defineTool()` preserves TypeScript inference. Streaming — `onUpdate(partial)` mid-execution. Sequential — `executionMode: "sequential"` prevents race conditions. Custom rendering — `renderCall()` + `renderResult()`. Pluggable — `createBashTool(cwd, { spawnHook })` without full override.

## Security

`tool_call` → confirm dialog → `{ block: true }`. `tool_call` → check path → block. `session_before_switch` → `{ cancel: true }`.

## State

Via `details` (reconstruct on `session_start`). Via `appendEntry()` (LLM-invisible, survives compaction). Via `withFileMutationQueue()` (concurrent-safe).

## Dynamic

Register tools in `session_start` or via command. Custom compaction with different model via `session_before_compact`. Handoff via `newSession()` + `withSession()`. Inter-extension via `pi.events` EventBus. Input transform via `input` hook → `{ action: "transform" }`.

## Anti-Patterns

`Type.Union(Type.Literal)` → breaks Google, use `StringEnum`. Awaiting in `session_start` → blocks startup, use `void`. In-memory only state → lost on reload. Stale context after `newSession()` → use `withSession()`. Deep merging `content`/`details` → not supported, return full arrays.
