# Pi vs OpenClaw — 15-Dimension Comparison

**Condensed — see ecosystem-comparison.md for all 12 systems.**

## Philosophy

| Dimension | Pi | OpenClaw |
|-----------|----|----------|
| **Role** | Standalone coding agent CLI | Multi-channel platform (Chat, API, Cron) |
| **Core size** | Minimal (4 tools, short prompt) | Full-featured (many built-in) |
| **Extension language** | TypeScript (jiti, no compile) | TypeScript (compiled plugins) |
| **Hook count** | ~25 events | 40+ typed hooks |

## Hook System

| Aspect | Pi | OpenClaw |
|--------|----|----------|
| **Registration** | `pi.on(event, handler)` | `api.on(event, handler, { priority?, timeoutMs? })` |
| **Ordering** | Registration order | Numeric priority (higher first) |
| **Block** | `{ block: true }` | `{ block: true }` + `requireApproval` |
| **Timeout** | None | Per-hook configurable |

## Agent Turn Lifecycle

| Phase | Pi | OpenClaw |
|-------|----|----------|
| Model resolution | Not in hooks | `before_model_resolve` |
| Turn preparation | Not in hooks | `agent_turn_prepare` |
| Prompt building | `before_agent_start` | `before_prompt_build` + `heartbeat_prompt_contribution` |
| Pre-submission guard | Not in hooks | `before_agent_run` (block before model reads prompt) |
| Synthetic reply | Not in hooks | `before_agent_reply` (short-circuit turn) |
| Finalization control | Not in hooks | `before_agent_finalize` (request revision) |

## Tool Lifecycle

| Aspect | Pi | OpenClaw |
|--------|----|----------|
| Pre-execution | `tool_call` (event.input mutable) | `before_tool_call` (rewrite params) |
| Post-execution | `tool_result` (field merge) | `after_tool_call` (observe) |
| Persistence | Not in hooks | `tool_result_persist` (rewrite before storage) |
| Write guard | Not in hooks | `before_message_write` |

## Session Lifecycle

| Phase | Pi | OpenClaw |
|-------|----|----------|
| Start | `session_start` | `session_start` (with reason) |
| Compaction | `session_before_compact`, `session_compact` | `before_compaction`, `after_compaction` |
| Reset | Not in hooks | `before_reset` |
| End | `session_shutdown` | `session_end` (with reason) |
| Switch/Fork | `session_before_switch`, `session_before_fork` (cancellable) | Not in hooks |

## Message & Subagent Lifecycle

| Phase | Pi | OpenClaw |
|-------|----|----------|
| Message received | Not in hooks | `message_received` + `inbound_claim` |
| Message sending | Not in hooks | `message_sending` (cancel/rewrite) |
| Dispatch | Not in hooks | `before_dispatch`, `reply_dispatch` |
| Subagent events | Not in hooks | `subagent_spawning`, `subagent_ended` |
| Gateway lifecycle | Not in hooks | `gateway_start/stop`, `cron_changed` |

## What to Steal from Pi

| Decision | Why |
|----------|-----|
| Extension = code, prompts = markdown | Clean separation |
| `block: true` terminal semantics | Simple, unambiguous |
| Tool override via name collision | Powerful without extra API |
| Mutable event input (`event.input`) | Intuitive for argument patching |
| Serialized event queue | Safety without complexity |
| `defineTool()` type inference | Preserves TypeScript types |
| Self-describing tools (`promptSnippet` + `promptGuidelines`) | Reduces hook boilerplate |

## What to Steal from OpenClaw

| Decision | Why |
|----------|-----|
| Phase separation (resolve → prepare → build → run) | Clear contracts per phase |
| Numeric priority | Explicit ordering control |
| Per-hook timeout budgets | Prevents wedged plugins |
| `requireApproval` with resolution | Rich approval workflows |
| Next-turn injection (`enqueueNextTurnInjection`) | Clean exactly-once context |
| Session extension state (`registerSessionExtension`) | Plugin isolation from transcript |
| Observation-only hooks (`registerAgentEventSubscription`) | Safe observability by default |
