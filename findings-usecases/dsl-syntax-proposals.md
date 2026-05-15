# DSL Syntax Proposals — Condensed

**4 concrete approaches inspired by 47 files of research. See individual proposals for full detail.**

## 6 Design Principles

| Principle | Source |
|-----------|--------|
| Two hook types: Actions (observe) + Filters (transform/block) | WordPress, K8s |
| Per-hook failure mode: `fail_closed` or `fail_open` | Kubernetes, Cline SDK |
| Lazy activation: load hook only when its event fires | VS Code |
| Declarative manifest: capabilities discoverable without loading code | Eclipse, IntelliJ, VS Code |
| Tower-style composition: compile-time Service/Layer stacking | Rust ecosystem |
| Multiple hook surfaces: Rust + Go + C daemons + Wasm | Your architecture |

## 4 Syntax Approaches

| Aspect | A. Rust Tower | B. Go Gateway | C. JSON Manifest | D. Daemon JSON |
|--------|-------------|--------------|-----------------|----------------|
| **Type safety** | Compile-time | Runtime | Runtime | Runtime |
| **Performance** | Zero-cost | Minimal | Minimal | Subprocess |
| **Dynamic loading** | Recompile | Yes | Yes (Wasm) | Yes |
| **Best for** | Engine hooks | Provider pipeline | Third-party ext | Command hooks |
| **Language** | Rust | Go | Any (Wasm) | Any (JSON) |

## 6 Key Decisions

1. **Two hook types**, not one — Actions ≠ Filters
2. **`fail_closed` default for security hooks** — K8s learned this after outages
3. **Lazy loading via manifest** — VS Code pattern works at scale
4. **Tower for Rust, function hooks for Go** — Each language's idiomatic pattern
5. **Wasm for third-party hooks** — Extism in all 5 processes
6. **Phase ordering > per-hook ordering** — Mutate → validate → execute
