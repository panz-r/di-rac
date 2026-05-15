# Genkit & OpenAI Agents SDK — Condensed

## Genkit (Google): 3-Level Middleware

`model` (LLM call), `tool` (tool execution), `generate` (generation loop). Registered via `generateMiddleware()` + `use: [middleware]`. Onion-layer composition.

5 built-in: `filesystem` (inject file tools), `skills` (inject SKILL.md), `toolApproval` (restrict tools → interrupt → resume), `retry` (exponential backoff), `fallback` (alternative model on error).

**Tool approval via interrupts**: Unapproved tool throws `ToolInterruptError` → caller resumes with `restartTool()` → generation continues. Unique pattern: pause + user prompt + resume mid-turn.

## OpenAI Agents SDK: 2 Scopes, Observation-Only

| Scope | Configured On | Events (7) |
|-------|-------------|------------|
| `RunHooks` | `Runner.run()` | `on_agent_start/end`, `on_llm_start/end`, `on_tool_start/end`, `on_handoff` |
| `AgentHooks` | `Agent(hooks=...)` | Same, scoped to one agent |

Key constraints:
- Tool hooks fire only for **local** tools (not hosted: WebSearchTool, FileSearchTool, CodeInterpreterTool)
- On LLM failure: `on_llm_start` fires but `on_llm_end`/`on_agent_end` are skipped
- **Observation only** — cannot block or modify
- `context.usage` updated before `on_llm_end`/`on_agent_end`

## Lessons

1. **Interrupt-based approval** (Genkit) — pause → user prompt → resume. More natural than binary block/allow.
2. **Observation-only hooks** (OpenAI) — design some hooks as read-only by default. Makes them safe for observability.
3. **Hosted vs local tool distinction** — Server-side execution doesn't trigger local hooks. Important constraint for your gateway-based hooks.
4. **Error gaps** — `on_llm_start` without matching `on_llm_end` signals failure. Design your hook lifecycle to account for errors at every stage.
