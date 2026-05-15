# Layering & Tool Wrapping — Condensed

**How Pi's 4-layer architecture bridges extension hooks to core runtime.**

## Architecture

```
Extension API: ToolDefinition (rich: schema, snippets, renderers)
      ↓ wrapToolDefinition() — context factory is LAZY (execution time)
AgentSession: AgentTool (lean: name, execute, schema)
      ↓
Agent Core: beforeToolCall / afterToolCall
      ↓
LLM Provider: tool schema
```

**Key**: `ctxFactory` is called at **execution time**, not registration. Context is always fresh.

## Dual Hook System

| Layer | Hooks | When |
|-------|-------|------|
| **Extension** | `pi.on("tool_call")` / `pi.on("tool_result")` | AgentSession event processing |
| **Core** | `beforeToolCall` / `afterToolCall` | Inside agent-loop.ts |

Extension hooks fire first (typed events), then core hooks finalize (low-level enforcement).

## System Prompt Assembly (Pure Function)

```
Identity → Available tools (promptSnippet only) → Guidelines (dynamic per tool)
→ [appendSystemPrompt] → Project Context → Skills (XML) → Date + CWD
```

## Pluggable Operations Pattern

```typescript
const bashTool = createBashTool(cwd, {
    spawnHook: ({ command, cwd, env }) => ({
        command: `source ~/.profile\n${command}`,
        cwd, env: { ...env, PI_SPAWN_HOOK: "1" },
    }),
});
```

Tool factories accept hooks for customization without full reimplementation.

## 7 Lessons

1. **Bidirectional bridge** — `ToolDefinition ↔ AgentTool` should be explicit
2. **Lazy context factory** — never capture context at registration
3. **Hook layering** — extension hooks first (typed), core hooks after (enforcement)
4. **Pure function prompts** — testable, predictable, composable
5. **Self-describing tools** — `promptSnippet` + `promptGuidelines` reduce hook boilerplate
6. **Pluggable operations** — tool factories accept hooks for customization
7. **Multiple prompt extension points** — better than one generic "modify prompt" hook
