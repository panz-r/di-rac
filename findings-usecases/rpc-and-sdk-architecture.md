# RPC Mode & SDK Architecture — Condensed

**How Pi's extension system works across interactive, headless, and embedded modes.**

## Three Execution Modes

| Mode | Flag | Primary Use | UI Available? |
|------|------|-------------|---------------|
| **Interactive** | (default) | Full TUI | Full (`ctx.ui.*`) |
| **Print** | `-p` / `--print` | CLI piping, single-shot | **No** (`ctx.hasUI = false`) |
| **RPC** | `--mode rpc` | IDE integration, custom UIs | JSON protocol (proxied) |

## UI Context Behavior Per Mode

| Method | Interactive | RPC | Print |
|--------|-------------|-----|-------|
| `select`, `confirm`, `input`, `editor` | Full TUI | JSON request/response | Returns undefined/false |
| `setStatus`, `notify`, `setTitle` | TUI update | Fire-and-forget JSON | No-op |
| `setFooter`, `setHeader`, `custom`, `setEditorComponent` | Full TUI | **Not supported** | No-op |
| `onTerminalInput` | Supported | **Not supported** | No-op |
| Widgets (component) | Full TUI | String arrays only | No-op |

**Rule**: Every UI-calling hook must guard with `ctx.hasUI` or provide a fallback.

## RPC Protocol

JSON-over-stdio, one line per message:

```json
// Request
{"id": 1, "method": "prompt", "params": {"message": "Hello"}}

// Streaming event
{"id": 1, "type": "event", "event": {"type": "message_update", "delta": "Hello..."}}

// Completion
{"id": 1, "type": "result", "result": {"status": "ok"}}
```

### RPC UI Protocol

```typescript
// Extension requests user input via RPC:
→ {"type": "rpc_ui_request", "id": "uuid", "method": "confirm",
   "params": {"title": "Proceed?", "message": "..."}}

// Client responds:
→ {"type": "rpc_ui_response", "id": "uuid", "result": true}
```

## SDK Architecture

| Class | Role |
|-------|------|
| `AgentSession` | Single conversation lifecycle |
| `AgentSessionRuntime` | Session replacement (new, fork, switch) |
| `DefaultResourceLoader` | Resource discovery (skills, prompts, themes) |
| `DefaultPackageManager` | Package resolution from multiple sources |

### Session vs Runtime

- `AgentSession`: Current active conversation. Subscribe to events, send prompts, manage tools.
- `AgentSessionRuntime`: Manages session transitions. `newSession()`, `fork()`, `switchSession()` create new `AgentSession` instances.

When `newSession()` is called, the old `AgentSession` is replaced. Hooks registered on the old session must be re-registered on the new one. In Pi, `bindExtensions()` handles this.

### SDK Initialization

```typescript
const session = await createAgentSession({
    model: { provider: "openai", model: "gpt-4" },
    resourceLoader: new DefaultResourceLoader({
        skills: [...],
        extensions: [...],
    }),
    settings: { autoCompactThreshold: 0.75 },
});

session.subscribe(event => {
    // Handle streaming events
});

await session.prompt("Hello");
```

## Cross-System Comparison

| Feature | Pi | Hermes | CrewAI | OpenAI SDK |
|---------|----|--------|--------|-----------|
| **Interactive** | TUI | TUI | CLI | N/A |
| **Headless** | RPC/Print | N/A | Programmatic | `Runner.run()` |
| **UI Fallback** | Returns undefined | No-op | N/A | N/A |
| **Protocol** | JSONL over stdio | JSON subprocess | Direct import | Python API |
| **Session Mgmt** | `AgentSessionRuntime` | `AIAgent` class | `Crew` class | `Runner` class |

## Key Design Lessons

1. **`ctx.hasUI` guard is essential** — all hooks that interact with the user must handle headless mode. Provide a boolean flag for extensions to check.
2. **Mode-agnostic extensions** — the same hook code works in all modes, but behaviors differ. Design hooks to be mode-aware.
3. **Session re-registration** — session transitions invalidate old contexts. Provide `withSession()` callbacks for post-transition work.
4. **Protocol for RPC UI** — `rpc_ui_request`/`rpc_ui_response` pattern enables interactive dialogs in headless integrations.
