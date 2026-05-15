# Di-Core Hook Opportunities — Condensed

**Mapping 18+ systems' hook patterns to your existing Rust engine + Go gateway.**

## Current State (No Hook System)

48 source files, 13 tools, ~3300-line agent loop. Only one formal trait (`ContextDistiller`). Everything else is hardcoded dispatch (`match` on tool name, per-tool match arms in pre-flight firewall).

## 8 Natural Hook Point Groups

| Group | Current Implementation | Hook Pattern (from) |
|-------|----------------------|---------------------|
| **Tool execution** | `run_preflight_firewall()` — hardcoded per-tool checks | `before_tool_call` / `after_tool_call` (Pi) |
| **Agent turn** | `run_turn()` — linear flow, no interception | `before_agent_start` / `after_agent_end` (OpenClaw) |
| **Context/compaction** | `ContextDistiller` trait — one extension point | `before_compaction` / `after_compaction` (Pi) |
| **Session lifecycle** | `SpawnAgent` → `AgentEngine`, no session events | `session_start` / `session_shutdown` (AG2) |
| **Error handling** | `CircuitBreaker` + `ErrorRouter` — fixed enums | `on_error` / `on_recovery` (Cline) |
| **Message/stream** | `StreamingToolAccumulator` — fixed handler | `on_text_delta` / `transform_llm_output` (Hermes) |
| **Approval/policy** | `ApprovalManager` — hardcoded approve lists | `before_approval` / `custom_approval_policy` (OpenClaw) |
| **Multi-agent** | `MultiAgentOrchestrator` — no subagent hooks | `on_handoff` / `subagent_ended` (OpenAI SDK) |

## Architecture Recommendations

1. **Tool dispatch → trait-based** — Replace `match` on tool name with `ToolHandler` trait
2. **Pre-flight checks → hook chain** — Replace per-tool match arms with deny-wins hook chain
3. **Error routing → `on_error` hooks** — Extend `ErrorRouter` with observer hooks
4. **ContextDistiller → compaction hooks** — Add `before_compaction`/`after_compaction` hooks
5. **ApprovalManager → custom policy hooks** — Per-tool approval policies as hooks
6. **Observer → observation hooks** — Extensions react to observer signals

## Cross-Boundary Hook Design

| Location | Pros | Cons | Best For |
|----------|------|------|----------|
| **Rust (di-core)** | Direct access to loop state, tools, context | Recompilation for new hooks | Tool lifecycle, context, recovery |
| **Go (api-gateway)** | Dynamic registration, no recompilation | Limited to request/response | Provider-level hooks, security, observability |
| **Wasm (Extism)** | Language-agnostic, sandboxed, no recompilation | FFI overhead, binary size | Third-party hooks, security-critical policies |

## Rust-Specific Constraints

- Trait objects (`Box<dyn Hook>`) for dynamic dispatch
- Channels for async boundary crossing
- Wasm (via Extism) for hot-reloadable, sandboxed hooks
- Go gateway already has `ModifyRequest`/`ModifyHeaders`/`ModifyMessages` function hooks
