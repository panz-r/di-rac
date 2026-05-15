# Api-Gateway Extensibility — Condensed

**8 mechanisms in your Go service.**

## 8 Existing Mechanisms

| # | Mechanism | What It Does |
|---|-----------|-------------|
| 1 | `Handler` interface | `Send(ctx, req)` + `Stream(ctx, req, callback)` — core plugin point |
| 2 | `OpenAICompatConfig` function hooks | `ModifyRequest`, `ModifyHeaders`, `ModifyMessages` — called at pipeline stages |
| 3 | `ResponsesAPIConfig` function hooks | `ModifyRequest`, `ModifyHeaders` |
| 4 | Capability detection | Type assertions for optional interfaces |
| 5 | Stream pipe/interceptor (Minimax) | Wraps callback to intercept/transform chunks |
| 6 | `BaseValidateSettings` options | `InactiveInThinking()`, `CrossParamRule()` |
| 7 | Generic `Settings`/`Extra` maps | Passed through for provider-specific data |
| 8 | Explicit `Register()` | 30 providers registered explicitly |

## Cross-Boundary Recommendations

| Concern | Best In | Why |
|---------|---------|-----|
| Security, auth, rate limiting | **Go gateway** | Dynamic, no recompilation |
| Tool lifecycle, context compaction | **Rust engine** | Direct loop state access |
| Observability, logging | **Both** | Gateway sees requests, engine sees internals |
| Third-party extensions | **Wasm (Extism)** | Sandboxed, language-agnostic |
