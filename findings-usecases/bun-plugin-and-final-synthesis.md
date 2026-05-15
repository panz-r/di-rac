# Bun Plugin API & Final Synthesis

**Bun's bundler plugin API (4 lifecycle hooks) and a final index mapping all 47 findings files to specific di-core components.**

## Bun's Plugin Lifecycle (for Comparison)

Bun's bundler has 4 plugin hooks — a minimal, focused set:

| Hook | When | Can Block? |
|------|------|-----------|
| `onStart` | Bundler started | No |
| `onResolve` | Before module resolution | Yes (return custom resolution) |
| `onLoad` | Before module loading | Yes (return custom content) |
| `onBeforeParse` | Before parsing (native addons) | No |

Minimal hook surface (4 hooks) for a very complex system (JavaScript bundler). More evidence that **fewer, well-designed hooks beat many poorly-designed ones**.

## Final Index: Finding Files → di-core Components

| di-core Component | Relevant Finding Files | Key Patterns |
|-------------------|----------------------|--------------|
| **Tool dispatch** (`tools/`) | `extension-api-surface.md`, `layering-and-tool-wrapping.md`, `rust-tower-middleware.md`, `dsl-syntax-proposals.md` | Tower Service/Layer traits, `before_tool_call`/`after_tool_call` hooks, `ModifyRequest` function hooks (Go) |
| **Pre-flight firewall** (`engine.rs`) | `design-patterns.md`, `k8s-admission-webhooks.md`, `agent-loop-hooks.md` | Mutate + validate phases, `fail_closed`/`fail_open`, block chain with deny-wins |
| **Context compaction** (`context/`) | `session-persistence-architecture.md`, `test-driven-insights.md`, `scheduling-and-background-patterns.md` | `before_compaction`/`after_compaction` hooks, LLM-invisible state via `custom` entries |
| **Agent turn lifecycle** (`engine.rs`) | `core-agent-loop-architecture.md`, `synthesis-lessons-and-patterns.md`, `agent-loop-hooks.md` | Phase-separated hooks, `before_agent_start`, `after_agent_end`, per-hook timeouts |
| **Recovery/circuit breakers** (`recovery.rs`) | `rust-harness-hooks-and-governance.md`, `comparative-analysis-and-tradeoffs.md`, `developer-experience-lessons.md` | `on_error` hooks, `fail_closed` failure mode, circuit breaker hooks |
| **Observer system** (`observer/`) | `pi-creator-design-philosophy.md`, `use-cases.md`, `agent-loop-hooks.md` | `on_stagnation` hooks, observation-only hooks (OpenAI SDK pattern) |
| **Go api-gateway** (`api-gateway/`) | `api-gateway-extensibility.md`, `go-middleware-and-intellij-plugins.md`, `k8s-admission-webhooks.md` | Extend existing `Modify*` hooks, Go `func(next) next` middleware, Wasm via Extism |
| **C daemons** (`command-daemon/`, `treesitter-daemon/`) | `daemon-protocols-hook-opportunities.md`, `treesitter-daemon-analysis.md` | JSON-over-stdin protocol hooks, generalized `function pointer + void* ctx` callback pattern |
| **Third-party extensions** | `wasm-plugins-wordpress-hooks.md`, `vscode-extension-api-patterns.md`, `dsl-syntax-proposals.md` | Extism for sandboxed Wasm, declarative manifest, lazy activation, disposable pattern |

## Final Architecture Recommendation

```
Third-party hooks (Wasm via Extism)
    │  sandboxed, any language, hot-reloadable
    ▼
Declarative Manifest (JSON)
    │  capabilities discoverable without loading code
    │  lazy activation (VS Code pattern)
    ▼
Hook Registry (di-core or api-gateway)
    │  phase ordering: mutate → validate → execute
    │  per-hook: priority, timeout, failure_mode, tools filter
    ▼
Hook Execution
    ├── Rust engine: Tower Service/Layer traits
    ├── Go gateway: existing Modify* function hooks
    ├── C daemons: pre/post execute via JSON protocol
    └── Wasm: Extism runtime in any process
```
