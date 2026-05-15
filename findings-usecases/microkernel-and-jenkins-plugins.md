# Microkernel & Jenkins — Condensed

**Design patterns from VS Code, Chrome, Eclipse, Jenkins (2000+ plugins).**

## Microkernel Core Rules

1. **Core is stable** — Breaking changes require major version + deprecation period
2. **Core provides services, never depends on plugins**
3. **Plugins communicate only through core event bus**
4. **Plugin contracts define the interface boundary**

## Plugin Isolation Models

| Model | Overhead | Crash Impact |
|-------|----------|-------------|
| In-process (same thread) | None | Plugin crash = app crash |
| In-process (isolated) | Minimal | Exception caught |
| Worker thread | ~2ms | Crash isolated |
| **Separate process** (daemons) | ~10ms IPC | **Full isolation** |
| Sandboxed (Wasm) | Variable | Near-complete |

## Jenkins Lessons (2000+ plugins)

| Problem | Solution |
|---------|----------|
| API breakage | Deprecation policy (deprecate→warn→remove, 3 releases) |
| Dependency hell | Registry with version constraints |
| Startup time | Lazy activation (load only when needed) |
| Security | Sandboxed execution, permission manifests |
| Memory | Plugin lifecycle (activate/deactivate on demand) |

## Your System as a Microkernel

| Component | Role | Language |
|-----------|------|----------|
| **di-core** | Core engine | Rust (stable, minimal, no plugin deps) |
| **api-gateway** | Plugin host | Go (dynamic hooks, dynamic registration) |
| **command-daemon** | Isolated service | C (separate process, full isolation) |
| **treesitter-daemon** | Isolated service | C (separate process, full isolation) |
| **Wasm plugins** | Third-party extensions | Any language (sandboxed, no recompilation) |

Your multi-process architecture **already implements the microkernel pattern**. Formalizing hook contracts between these processes is the next step.
