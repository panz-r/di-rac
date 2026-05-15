# Extension Runner Internals — Event Dispatch & Runtime Architecture

Analysis of the actual `ExtensionRunner` and type system source code to understand how events are dispatched, how results are merged, and how the runtime manages context lifecycle.

## Event Dispatch Architecture

### Generic `emit()` vs Dedicated Emitters

The runner uses **two patterns** for event dispatch:

| Pattern | Used For | Behavior |
|---------|----------|----------|
| **Generic `emit()`** | Observation events, session lifecycle | Iterates all handlers, catches errors, returns merged result for "before" events |
| **Dedicated emitters** | `tool_call`, `tool_result`, `input`, `context`, `user_bash`, `before_provider_request`, `before_agent_start`, `resources_discover` | Custom merge/chain/block logic |

### Dedicated Emitter Behaviors

| Emitter | Merge Strategy | Block Support |
|---------|---------------|---------------|
| `emitToolCall(event)` | Returns first `{ block: true, reason }` result | Yes — short-circuits on first `block: true` |
| `emitToolResult(event)` | Field-by-field merge: `content`, `details`, `isError` from any handler. Later handlers override earlier. | No |
| `emitInput(text, images, source)` | Chains transforms: each handler gets current `text`/`images`, can return `"transform"` (update) or `"handled"` (short-circuit) | Yes — `"handled"` stops chain |
| `emitContext(messages)` | `structuredClone` deep copy → each handler gets fresh copy → returns modified array | No |
| `emitBeforeAgentStart(prompt, images, systemPrompt, opts)` | Multiple handlers can each return `systemPrompt` (chained), multiple `message` objects collected | No |
| `emitUserBash(event)` | Returns first handler's result | No — first return wins |
| `emitBeforeProviderRequest(payload)` | Chains: each handler sees previous handler's modified payload | No |
| `emitResourcesDiscover(cwd, reason)` | Collects `skillPaths`, `promptPaths`, `themePaths` from all handlers | No |

### Handler Execution Order

```
For each extension (registration order):
  For each handler for this event type (registration order):
    try { result = await handler(event, ctx); }
    catch(err) { emitError(err); continue; }
```

Key fact: **Pi has no priority system for handlers**. Execution order is extension registration order + handler registration order within each extension. This is a notable difference from OpenClaw's numeric priority system.

## Tool Call Event Mutable Input

The `tool_call` event's `event.input` is **mutable in place**:

```typescript
// From types.ts: "event.input is mutable. Mutate it in place to patch
// tool arguments before execution. Later tool_call handlers see earlier
// mutations. No re-validation is performed after mutation."
```

This means a hook can modify tool args by side-effect without returning anything:

```typescript
pi.on("tool_call", async (event) => {
    if (event.toolName === "bash") {
        event.input.command = `cd /safe/path && ${event.input.command}`;
        // No return needed — mutation is in-place
    }
});
```

Combined with the block pattern:
```typescript
pi.on("tool_call", async (event) => {
    if (event.toolName === "bash" && isDangerous(event.input.command)) {
        return { block: true, reason: "Dangerous command blocked" };
    }
    // No return → continue execution
});
```

## Error Handling Strategy

```typescript
// Individual handler errors are caught, reported, and DO NOT stop other handlers
try {
    const handlerResult = await handler(event, ctx);
    // process result
} catch (err) {
    this.emitError({
        extensionPath: ext.path,
        event: event.type,
        error: err instanceof Error ? err.message : String(err),
        stack: err instanceof Error ? err.stack : undefined,
    });
    // continue to next handler
}
```

- One failing handler does not block others
- Errors are reported via `emitError()` to registered error listeners
- Error events include the extension path, event type, error message, and stack trace

## Context Lifecycle & Staleness

### The `createContext()` Pattern

```typescript
createContext(): ExtensionContext {
    return {
        get ui() { runner.assertActive(); return runner.uiContext; },
        get hasUI() { runner.assertActive(); return runner.hasUI(); },
        get cwd() { runner.assertActive(); return runner.cwd; },
        isIdle: () => { runner.assertActive(); return runner.isIdleFn(); },
        // ... all getters and methods call assertActive()
    };
}
```

Key design decisions:
- **Lazy getters**: All context properties use getters, not stored values. This means they reflect the most current state even if the runner's internal state changes between `createContext()` and use.
- **Staleness checks**: Every access calls `assertActive()` which throws if the runtime has been invalidated (after session replacement or reload).
- **Per-call contexts**: `createContext()` is called fresh for each event emission. This means each handler invocation gets a fresh context object.

### When Context Becomes Stale

The runtime invalidates after:
1. `newSession()` — session replaced
2. `fork()` — session replaced  
3. `switchSession()` — different session file
4. `reload()` — extensions/skills/prompts reloaded

After invalidation, any access to `ctx.*` properties or methods throws with a descriptive message directing the developer to use `withSession()` callbacks.

### Command Context Reuse

`createCommandContext()` uses `Object.defineProperties` + `Object.getOwnPropertyDescriptors` to copy the lazy getters from `createContext()` rather than evaluating them immediately:

```typescript
createCommandContext(): ExtensionCommandContext {
    const context = Object.defineProperties(
        {},
        Object.getOwnPropertyDescriptors(this.createContext()),
    ) as ExtensionCommandContext;
    context.waitForIdle = () => { this.assertActive(); return this.waitForIdleFn(); };
    // ... add command-specific methods
    return context;
}
```

This ensures command contexts have the same lazy/staleness-check behavior as event contexts.

## Session Lifecycle Events with Reasons

Each session lifecycle event includes a `reason` field explaining *why* it fired:

### `session_start` reasons
| Reason | Meaning |
|--------|---------|
| `startup` | Initial session creation |
| `reload` | After `/reload` command |
| `new` | After `newSession()` |
| `resume` | After `switchSession()` |
| `fork` | After `fork()` |

### `session_shutdown` reasons
| Reason | Meaning |
|--------|---------|
| `quit` | Application quitting |
| `reload` | Extensions/skills being reloaded |
| `new` | Being replaced by new session |
| `resume` | Being replaced by resumed session |
| `fork` | Being forked into new session |

## Extension Registry Details

### Tool Registration (First-Wins)

```typescript
getAllRegisteredTools(): RegisteredTool[] {
    const toolsByName = new Map<string, RegisteredTool>();
    for (const ext of this.extensions) {
        for (const tool of ext.tools.values()) {
            if (!toolsByName.has(tool.definition.name)) {
                toolsByName.set(tool.definition.name, tool);
            }
        }
    }
    return Array.from(toolsByName.values());
}
```

- **First registration per name wins** across all extensions
- This means a later extension cannot override a tool from an earlier extension (unlike tool override of built-ins which is handled differently at the AgentSession level)

### Shortcut Conflict Resolution

```typescript
Priority:
1. Reserved built-in keybindings → ALWAYS win (cannot be overridden)
2. Non-reserved built-in keybindings → CAN be overridden by extensions
3. Extension shortcuts → last-registered wins among extensions
```

Reserved keybindings (cannot be overridden):
- `app.interrupt`, `app.clear`, `app.exit`, `app.suspend`
- `app.thinking.cycle`, `app.model.cycleForward`, `app.model.cycleBackward`, `app.model.select`
- `app.tools.expand`, `app.thinking.toggle`, `app.editor.external`, `app.message.followUp`
- `tui.input.submit`, `tui.select.confirm`, `tui.select.cancel`, `tui.input.copy`, `tui.editor.deleteToLineEnd`

### Command Conflict Resolution

Duplicate command names get `:N` suffixes for unique invocation:

```
Two extensions both register "review" command:
  → Extension A: "/review" (invocationName: "review")
  → Extension B: "/review" (invocationName: "review:2")
```

This means users can access both with explicit suffixes. The base name goes to whichever extension's command was registered first.

## Provider Registration Lifecycle

```typescript
// During extension loading: registrations are QUEUED
this.runtime.pendingProviderRegistrations.push({ name, config, extensionPath });

// After bindCore(): queued registrations are FLUSHED
for (const { name, config } of this.runtime.pendingProviderRegistrations) {
    this.modelRegistry.registerProvider(name, config);
}
this.runtime.pendingProviderRegistrations = [];

// After bindCore(): registrations take IMMEDIATE effect
this.runtime.registerProvider = (name, config) => {
    this.modelRegistry.registerProvider(name, config);
};
```

This two-phase approach allows providers to be registered during extension loading (before the model registry is ready), then flushed once binding completes.

## Implications for Hook DSL Design

| Pattern | Pi Implementation | DSL Design Consideration |
|---------|------------------|--------------------------|
| **Error isolation** | Individual handler errors caught, don't stop others | Essential: one broken hook shouldn't crash the loop |
| **Lazy context** | Getters, not stored values | Context should reflect current state, not snapshot at creation |
| **Staleness** | `assertActive()` on every access | Session transitions must invalidate stale contexts |
| **Mutable input** | `event.input` mutated in place | Clear API for argument modification without return values |
| **Field-by-field merge** | Tool results merged per-field | No deep merge — whole-field replacement only |
| **Handler ordering** | Registration-order only | Consider whether priority system is needed |
| **Block semantics** | First `block: true` wins, no override | Simple, predictable |
