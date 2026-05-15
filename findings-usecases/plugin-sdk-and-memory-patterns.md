# Plugin SDK & Memory Patterns — Condensed Reference

## Namespace Architecture

OpenClaw uses grouped namespaces vs Pi's flat API:

```typescript
// OpenClaw (namespaced)
api.session.state.registerSessionExtension(...)
api.session.workflow.enqueueNextTurnInjection(...)
api.agent.events.registerAgentEventSubscription(...)
api.lifecycle.registerRuntimeLifecycle(...)

// Pi (flat)
pi.sendMessage(content, opts)
pi.appendEntry(type, data)
```

Namespaces scale better as APIs grow. They group related functionality for discoverability and allow scope-based authorization.

## State Hierarchy (5 Lifetime Scopes)

| Scope | Pi | OpenClaw | Lifetime |
|-------|----|----------|----------|
| **Plugin config** | — | `api.pluginConfig` | Config lifetime |
| **Session state** | `appendEntry("custom")` | `registerSessionExtension()` | Session lifetime |
| **Next-turn** | `sendMessage({deliverAs: "nextTurn"})` | `enqueueNextTurnInjection()` | One turn (or TTL) |
| **Per-run** | Extension context (lazy) | `runContext.setRunContext()` | One agent turn |
| **Tool details** | Tool result `details` | Tool result `details` | Until compaction/pruning |

OpenClaw's explicit state hierarchy is cleaner than Pi's mixed approach. Each scope has clear lifetime and access rules.

## Session Extension State

```typescript
// Register plugin-owned state alongside session rows
api.session.state.registerSessionExtension("my-plugin", { status: "active" });
// Accessible via Gateway sessions.pluginPatch
// JSON-compatible, survives restarts
// UI can render plugin data without learning plugin internals
```

## Next-Turn Injection

```typescript
// Durable context that reaches model on next turn exactly once
api.session.workflow.enqueueNextTurnInjection({
    content: "User approved deletion.",
    idempotencyKey: "approval-del-123",
    ttlMs: 300_000,  // auto-expire
});
// Drained before prompt hooks, deduplicated by idempotencyKey
```

## Plugin Lifecycle

```typescript
api.lifecycle.registerRuntimeLifecycle({
    onReset(session), onDelete(session),   // Removes persistent state
    onDisable(),                             // Plugin disabled
    onRestart(),                             // Keeps durable state, releases resources
});
```

Cleanup semantics: reset/delete/disable clears extension state and pending injections. Restart keeps state.

## Exclusive Slot vs Additive Registration

| Pattern | Methods | Use Case |
|---------|---------|----------|
| **Exclusive** | `registerContextEngine()`, `registerMemoryCapability()` | One active at a time |
| **Additive** | Hooks, supplements, supplements | Multiple plugins contribute |

## Trusted vs Untrusted Plugins

```typescript
// Trusted policy — runs BEFORE all hooks (bundled-only)
api.registerTrustedToolPolicy({
    toolPolicy(event): BeforeToolCallResult {
        // Workspace policy, budget enforcement
    }
});

// Regular hook — runs after trusted policies
api.on("before_tool_call", handler, { priority: 50 });
```

Trusted policies cannot be bypassed by extension hooks.

## Memory System

### Memory Files

```
MEMORY.md         → Long-term, injected every turn
memory/*.md       → Daily notes, searched on demand
DREAMS.md         → Dream diary (background consolidation)
```

### Memory + Compaction Interaction

```
Before compaction: Memory flush turn runs silently
  → Agent saves important context to MEMORY.md / memory/*.md
  → Configurable model override

During compaction: Messages summarized; memory files NOT affected
After compaction: MEMORY.md still injected; recent tail preserved
```

### Memory Plugin SDK

```typescript
// Exclusive memory capability
api.registerMemoryCapability(capability);

// Additive supplements (any plugin can contribute)
api.registerMemoryPromptSupplement(builder);
api.registerMemoryCorpusSupplement(adapter);
```

## Observation vs Modification Hooks

```typescript
// Observation (sanitized, no raw content)
api.agent.events.registerAgentEventSubscription(
    (event) => { /* has sanitized metadata only */ },
    { events: ["model_call_started", "model_call_ended"] }
);

// Modification (can change behavior)
api.on("before_tool_call", handler);  // Can block, rewrite params
```

Observation hooks should not receive raw prompt content. Modification hooks should.

## 6 Key Design Lessons

1. **State lifetime hierarchy** — define clear scopes (config/session/turn/run/tool) with persistence guarantees
2. **Namespace organization** — grouped namespaces scale better than flat APIs
3. **Exclusive vs additive** — exclusive slots for singletons (memory, context engine), additive for composable behavior
4. **System policy vs extensions** — trusted policies run before all hooks and cannot be bypassed
5. **Memory hooks** — memory flush before compaction, memory search/get as tools, dreaming as background cron
6. **Observation vs modification** — separate sanitized subscriptions from behavior-modifying hooks
