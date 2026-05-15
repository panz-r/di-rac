# Use Cases — Quick Reference

**Across all systems. Organized by capability.**

## Security & Safety (7 patterns)

Block dangerous commands — `tool_call` check + return block. Protected paths — override `read` tool. Approval gate — `requireApproval` with severity/timeout. Rate limiting — track frequency per tool. Tool filtering — `before_tool_execute(tools=['email'])`. Session switch guard — `session_before_switch` → `{ cancel: true }`.

## Monitoring (5)

Tool call logging — pre/post tool hooks. Cost tracking — `after_llm_call` accumulate tokens. Iteration limiting — check count, block. Silent observer — track workflow signals end-to-end. Sanitized telemetry — `model_call_started/ended` without prompt content.

## State (6)

LLM-visible — `sendMessage({customType})`. LLM-invisible — `appendEntry(type, data)` (survives compaction). Plugin session — `registerSessionExtension()`. Next-turn — `enqueueNextTurnInjection()` with TTL. Per-run scratch — `runContext.setRunContext()`. Tool metadata — `details` field (stripped before replay).

## Tool Customization (5)

Tool override — register same name as built-in. Pluggable operations — `createBashTool(cwd, { spawnHook })`. Dynamic registration — register at `session_start`. Sequential mode — `executionMode: "sequential"`. Truncation — 50KB/2000 line limit.

## Background & Scheduled (4)

Cron — `--session isolated` for fresh context. Heartbeat — `heartbeat_prompt_contribution` for monitor-only context. Background subagents — `async: true` with completion notifications. Webhook — `POST /hooks/agent` for external triggers.

## Multi-Agent (5)

Subagent spawning — child Pi process with JSON mode. Handoff tracking — `on_handoff` hook. Agent orchestration — crew-scoped hooks. Parallel subagents — `Promise.all` with concurrency limit. Worktree isolation — git worktrees for parallel edits.
