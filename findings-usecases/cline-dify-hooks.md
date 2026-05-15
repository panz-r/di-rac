# Cline SDK & Dify — Two More Architectures

Two additional systems: Cline (TypeScript SDK with behavioral hooks) and Dify (Python platform with 6 plugin types).

## Part 1: Cline SDK Hooks (TypeScript)

Cline's SDK provides plugins with lifecycle hooks defined in the `hooks` object.

### 6 Core Hook Stages

```typescript
const myPlugin: AgentPlugin = {
    name: "my-plugin",
    setup(api, ctx) {
        // Register tools, commands, providers
    },
    hooks: {
        beforeRun(context) { /* Run start: logging, timers, rate limits */ },
        afterRun(context) { /* Run end: metrics, notifications, cleanup */ },
        beforeModel(context) { /* Before LLM call: modify prompt */ },
        afterModel(context) { /* After LLM response */ },
        beforeTool(context) { /* Before tool: audit, block */ },
        afterTool(context) { /* After tool: log, side effects */ },
        onEvent(context) { /* Custom events */ },
    },
};
```

### 16 Hook Stages (Full)

```
input → runtime_event → session_start → run_start → iteration_start →
turn_start → before_agent_start → tool_call_before → tool_call_after →
turn_end → stop_error → iteration_end → run_end → session_shutdown → error
```

| Stage | Use For |
|-------|---------|
| `run_start` / `run_end` | Logging, timers, metrics, notifications |
| `before_agent_start` | Inject context, modify prompt/messages |
| `tool_call_before` / `tool_call_after` | Audit, block, log, side effects |
| `error` | Error reporting |
| `session_start` / `session_shutdown` | Session lifecycle |

### Hook Policies

```typescript
hooks: {
    beforeTool: {
        handler: async (context) => { /* block or allow */ },
        policy: {
            mode: "blocking",          // or "async"
            timeoutMs: 5000,
            retries: 2,
            retryDelayMs: 1000,
            failureMode: "fail_closed", // or "fail_open"
            maxConcurrency: 5,
            queueLimit: 100,
        },
    },
}
```

`fail_closed` = deny on error (for policy enforcement). `fail_open` = allow on error (for observability).

### Plugin Registration

```typescript
import { type AgentPlugin } from "@cline/sdk";

await cline.start({
    prompt: "Analyze data",
    config: {
        extensions: [myPlugin],          // Direct reference
        pluginPaths: ["/path/to/plugin.ts"],  // File path
    },
});
```

### Plugin Manifest & Capabilities

```typescript
const plugin: AgentPlugin = {
    name: "my-plugin",
    manifest: {
        capabilities: ["tools", "hooks", "commands"],
    },
    setup(api, ctx) {
        api.registerTool({ name: "query_db", handler });
        api.registerCommand({ name: "db-status", handler });
    },
};
```

### Extension Points

| Point | What It Does |
|-------|-------------|
| **Tool** | Model-callable action |
| **Command** | User slash command |
| **Hook** | Lifecycle handler with policies |
| **Rules** | Prompt guidance included in every session |
| **Events** | External triggers (new PR, Slack message) |
| **Plugin** | Packages tools, hooks, commands, rules, events |

---

## Part 2: Dify Plugin System (Python)

Dify provides 6 plugin types for extending its LLM application platform.

### 6 Plugin Types

| Type | Purpose | Selection |
|------|---------|-----------|
| **Tool** | External service integration (Google Search, Stable Diffusion) | Multi-select |
| **Model** | Model providers (OpenAI, Anthropic) | Config-based |
| **Datasource** | Data source connectors | Config-based |
| **Trigger** | Event-triggered automations | Multi-select |
| **Agent Strategy** | Agent orchestration logic | Single-select |
| **Endpoint** | HTTP API endpoints | Multi-select |

### Plugin Architecture

```
plugin.zip → Plugin Daemon → Isolated Container
```

Plugins run in **isolated containers** managed by a Plugin Daemon. Communication happens via a reverse-invocation mechanism (the plugin calls back into the host).

### Plugin Manifest (plugin.yaml)

```yaml
name: my-tool-plugin
version: 1.0.0
type: tool
author: ...
plugins:
  tools:
    - name: my_tool
      endpoint: ...
```

### Reverse Invocation

Dify plugins use a unique "reverse invocation" pattern — plugins are called by the host but can also invoke host APIs. This is implemented via a daemon process that manages plugin lifecycle in isolated containers.

### Hook Points via Extensions

Dify's code extensions (`api/core/extension/`) provide integration points:

| Extension | Purpose |
|-----------|---------|
| Before/after model invocation | Modify request/response |
| Prompt transformation | Modify prompts before send |
| Memory management | Conversation history handling |
| Output moderation | Filter responses |

### Comparison

| Feature | Cline | Dify |
|---------|-------|------|
| **Language** | TypeScript | Python |
| **Hook style** | Behavioral hooks with policies | Platform extensions |
| **Hook count** | 6 core + 16 stages | ~5 extension points |
| **Blocking** | `fail_closed` policy mode | Via extension code |
| **Plugin isolation** | In-process TypeScript | Container-based Python |
| **Hook policies** | Yes (timeout, retry, failureMode) | No |
| **Marketplace** | npm + git | Dify Marketplace |

## Key Design Lessons

### Cline
1. **Hook policies** — `mode`, `timeoutMs`, `retries`, `failureMode`, `maxConcurrency`, `queueLimit` — per-hook configuration for production hardening
2. **`fail_closed` vs `fail_open`** — explicit failure mode: `fail_closed` for policy hooks (deny on error), `fail_open` for observability hooks (allow on error)
3. **16-stage hook lifecycle** — covers every phase from input through run, iteration, turn, tool, session, and error
4. **Plugin as extension container** — single `AgentPlugin` bundles tools, hooks, commands, rules, and events

### Dify
5. **6 plugin types** — separates concerns (tools vs models vs datasources vs triggers vs agent strategies vs endpoints)
6. **Container isolation** — plugins run in isolated containers via Plugin Daemon
7. **Reverse invocation** — plugins can call back into the host platform
8. **Marketplace distribution** — dedicated marketplace for plugin discovery and installation
