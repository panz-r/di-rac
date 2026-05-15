# Synthesis — Final DSL Design Reference

**42 files, 20+ ecosystems, 5 processes. The essential patterns.**

## 7 Universal Decisions

1. Map YOUR loop stages to hook points
2. Each hook = Block, Observe, or Transform — never mix
3. Error isolation: one broken hook never crashes the loop
4. Multiple surfaces: Rust engine (tools/context) + Go gateway (observability/security) + C daemons (pre/post exec)
5. Dual injection: system prompt (authority) + user message (cache-friendly)
6. Start simple: registration-order + first-block-wins
7. Explicit state hierarchy: per-run → next-turn → session → permanent

## 18 Patterns Worth Stealing

| # | Pattern | Source |
|---|---------|--------|
| 1 | Lazy context factory (execution-time) | Pi |
| 2 | Self-describing tools (`promptSnippet` + `promptGuidelines`) | Pi |
| 3 | Phase-separated hooks (clear contracts per phase) | OpenClaw |
| 4 | Per-hook timeout budgets | OpenClaw, Pydantic AI |
| 5 | `**kwargs` forward compatibility | Hermes |
| 6 | Three transform stages (raw → tool → LLM output) | Hermes |
| 7 | Skip exceptions (`SkipModelRequest`) | Pydantic AI |
| 8 | Full state snapshots (pause, inspect, resume) | Haystack |
| 9 | Hook policies (`timeoutMs`, `retries`, `fail_closed`) | Cline SDK |
| 10 | Hook management API (`clear_all`, `unregister`) | CrewAI |
| 11 | Actions + Filters (two types, battle-tested 20+ years) | WordPress |
| 12 | Numeric priority with defaults (10 default) | WordPress |
| 13 | Lazy activation (load hook only when event fires) | VS Code |
| 14 | Declarative manifest (capabilities without loading code) | VS Code, Eclipse |
| 15 | Disposable pattern (automatic cleanup on deactivation) | VS Code |
| 16 | Tower Service/Layer (zero-cost Rust middleware) | Rust ecosystem |
| 17 | `ModifyRequest`/`ModifyHeaders`/`ModifyMessages` | Your api-gateway |
| 18 | Deliberate minimalism (4 tools, ~1000 token prompt) | Pi creator |

## Your Architecture Hooks

| Layer | Lang | Hook Hosting | Best For |
|-------|------|-------------|----------|
| **di-core** | Rust | Tower Service/Layer traits | Tool lifecycle, context, recovery |
| **api-gateway** | Go | Function hooks (existing) | Security, auth, observability |
| **command-daemon** | C | Pre/post execute hooks | Command security, auditing |
| **central-daemon** | C | Config change hooks | Coordination, lock lifecycle |
| **treesitter-daemon** | C | User-defined `.scm` query files | Custom AST analysis |
| **Third-party** | Any | Wasm via Extism | Sandboxed, no recompilation |

## 10 Anti-Patterns

| Anti-Pattern | Fix |
|-------------|-----|
| Awaiting in startup hooks | Fire async |
| Assuming UI available | Check `hasUI` / detect mode |
| In-memory-only state | Persistent session state |
| Stale context after transition | `withSession()` callbacks |
| Deep merging results | Field-by-field replacement |
| 40+ unorganized hooks | Categorize by lifecycle stage |
| Deprecated compat hooks | Clean removal |
| No timeout defaults | Always have a default |
| No hook testing utilities | First-class test harnesses |
| Subprocess for perf-critical hooks | In-process callable hooks |

## Pi Creator's Philosophy (For Your Loop)

> "If I don't need it, it won't be built."
> "4 tools are sufficient if you have an extension system."
> "Observability is non-negotiable."
> "Files are the universal extension mechanism."
> "Full YOLO by default — security theater is pointless."
