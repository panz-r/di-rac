# Rust Hooks & Governance — Condensed

## Hook Performance (400-600 invocations/turn)

| Hook Type | Mean | 100 Calls | Architecture |
|-----------|------|-----------|-------------|
| Python subprocess | 37ms | 3.7s | 2,143x slower |
| Python callable | 1ms | 0.1s | 59x slower |
| **Rust callable** | **0.018ms** | **0.002s** | **Baseline** |

## Scaling (10→1000 patterns)

Python: 45μs→5,415μs (linear, 5.4μs/pattern). Rust RegexSet: 0.3μs→1.9μs (flat).

## Governance Toolkit (Microsoft)

Deterministic (non-LLM) policy enforcement via hooks into 4+ frameworks. Addresses all 10 OWASP agentic AI risks.

## Tower Middleware for Rust

Standard pattern: `trait Layer<S> { type Service; fn layer(&self, inner: S) -> Self::Service; }`. Compose via `ServiceBuilder::new().layer(A).layer(B).service(C)`. 16+ built-in middleware.

## 5 Lessons for Your DSL

1. **Go gateway for dynamic hooks** — Existing `ModifyRequest`/`ModifyHeaders`/`ModifyMessages` pattern
2. **Rust engine for perf hooks** — Tower Service/Layer traits, compile-time composition
3. **Wasm for third-party hooks** — Sandboxed, language-agnostic, no recompilation
4. **In-process callables are the minimum bar** — Subprocess adds 37ms overhead per hook
5. **`poll_ready` enables backpressure** — Unique to Tower, enables rich scheduling hooks
