# VS Code Extension API — Condensed

**3 mechanisms for your multi-process architecture.**

## 3 Key Mechanisms

| Mechanism | Purpose | Benefit |
|-----------|---------|---------|
| **Activation Events** | Lazy loading — load extension only when its event fires | Fast startup |
| **Contribution Points** | Declarative registration in `package.json` | Discover capabilities without loading code |
| **Lifecycle (`activate`/`deactivate`)** | Clean setup/teardown + Disposable pattern | Resource management |

## Architecture Mapping

| VS Code | Your System |
|---------|------------|
| Main process | **di-core (Rust engine)** |
| Extension Host | **api-gateway (Go process)** |
| Language Server | **tree-sitter daemon (C)** |
| Extensions | **Wasm plugins** or **Go plugins** |

## Patterns for Your DSL

1. **Lazy activation** — `activationEvents: ["onToolCall:bash"]` — load hook only when bash runs
2. **Declarative manifest** — Hooks declare what they do in a manifest without loading code
3. **Lifecycle hooks** — `activate()` and `deactivate()` for clean resource management
4. **Disposable pattern** — Every registration returns a cleanup handle. Auto-disposed on deactivation
5. **Contribution points** — Extensions declare capabilities in a manifest (hook points, tool handlers)
6. **Map to Go gateway** — Dynamic hooks in Go (no recompilation), performance hooks in Rust
