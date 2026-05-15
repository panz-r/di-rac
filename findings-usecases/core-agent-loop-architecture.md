# Core Agent Loop Architecture — Condensed

**Pi's two-level loop structure with cross-system comparison. Focus on the patterns that matter for your Rust-based di-core DSL.**

## The Two-Level Loop

```
OUTER while(true):
    Check follow-up queue → if messages, continue outer loop
    INNER while(hasMoreToolCalls || pendingMessages > 0):
        Process steering messages (inject mid-turn)
        streamAssistantResponse() → LLM
        executeToolCalls()
    If no follow-up messages → break
```

## Queue Mechanics

| Queue | When Checked | Effect |
|-------|-------------|--------|
| **Steering** | Start of loop + after each turn | Injected mid-execution |
| **Follow-up** | After agent would stop | Runs after agent finishes |
| **Drain mode** | "all" or "one-at-a-time" per queue | Controls pacing |

## 3-Stage Tool Pipeline

```
prepareToolCall() → executePreparedToolCall() → finalizeExecutedToolCall()
    Validate + beforeToolCall     Run tool + catch errors        afterToolCall + merge
```

**Parallel**: Prepare seq → Execute via `Promise.all` → Emit in source order.
**Sequential**: If ANY tool in batch has `executionMode: "sequential"`, all execute sequentially.
**Termination**: ALL tools must return `terminate: true` to stop early.

## Error Recovery

```rust
// The loop NEVER crashes — all errors produce synthetic messages
// 3 levels of isolation:
// 1. Handler errors → caught, logged, continue to next handler
// 2. Tool errors → caught, turned into error messages
// 3. Loop errors → caught, synthetic stopReason: "error" message
```

## AbortSignal

Single `AbortController` flows through: listeners → LLM stream → hooks → tool execute → context transform. Enables clean cancellation of any in-flight operation.

## Cross-System Loop Comparison

| System | Loop Structure | Steering | Error Recovery |
|--------|---------------|----------|----------------|
| **Pi** | 2-level (steering + follow-up) | Queue-based | Synthetic error messages |
| **OpenClaw** | Linear + cron isolation | Next-turn injection | Exponential backoff retry |
| **Hermes** | Linear + iteration limit | `inject_message()` | Per-handler catch + continue |
| **LangChain** | Graph-based (LangGraph nodes) | State reducer | Middleware try/catch |
| **di-core** | Sequential tool dispatch + recovery | Planned | Circuit breakers + stagnation detection |
| **Cline SDK** | 16-stage lifecycle | Hook policies | `fail_closed` / `fail_open` |

## Key Lessons for Your DSL

1. **Two-level loop pattern** — Inner for tools + steering, outer for follow-ups. Consider if your complex loop needs more levels (plan → execute → verify).
2. **Three isolation boundaries** — Handler errors ≠ tool errors ≠ loop errors. Each needs its own boundary.
3. **AbortSignal throughout** — Single cancellation context flowing through all hooks/tools/listeners.
4. **Termination consensus** — ALL tools agree to terminate. One tool shouldn't cut off others.
5. **Queue drain modes** — "all" vs "one-at-a-time" gives extensions control over pacing.
