# DSL Design Synthesis — All 5 Processes

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      Extension Manifest                          │
│              ~/.dirac/hooks/<name>/manifest.json                 │
│          (Declarative, no code loaded at scan time)              │
└─────────────────────────────────────────────────────────────────┘
         │                     │                    │
         ▼                     ▼                    ▼
┌─────────────────┐  ┌──────────────────┐  ┌──────────────────┐
│    di-core       │  │   api-gateway     │  │   C Daemons      │
│    (Rust)        │  │   (Go)            │  │   (C)             │
├─────────────────┤  ├──────────────────┤  ├──────────────────┤
│ HookRegistry     │  │ HookConfig       │  │ Subprocess       │
│ Tower traits     │  │ Function fields  │  │ JSON protocol    │
│ Compile-time     │  │ Global + per-    │  │ Hook config file │
│   OR dynamic     │  │ provider merge   │  │ (DIRAC_HOOK_CONF)│
│ Wasm (Extism)    │  │ Wasm (Extism)    │  │ Wasm (Extism)    │
└─────────────────┘  └──────────────────┘  └──────────────────┘
```

## Two Hook Types (All Processes)

```
Action: observe, log, notify  →  void return, never blocks
Filter: transform, block      →  returns modified input OR deny with reason
```

## Fail Mode Per Hook (All Processes)

```
fail_open:   error is logged, hook is skipped, operation continues
fail_closed: error causes denial (for security-critical hooks)
```

## Deny-Wins Composition

Multiple filters on the same hook point run in priority order.
The first `Deny` short-circuits — no subsequent hooks run.

## Lazy Activation

VS Code pattern: scan manifests at startup (parses JSON, loads no code),
activate hook only when its event fires. Confirmed by VS Code's 10+ year
track record with thousands of extensions.

## Wasm Sandbox

Extism PDK in all 5 processes. Same Wasm module works everywhere.
16MB memory limit, 5000ms timeout, no FS/network by default.
~0.1ms FFI overhead per call.

## Implementation Order

| Phase | What | Effort | Risk |
|-------|------|--------|------|
| 1 | `HookRegistry` in di-core with 3-4 hook points | 2 days | Low |
| 2 | Migrate `run_preflight_firewall` to `filter_tool_call` hooks | 1 day | Low |
| 3 | Migrate `ApprovalManager` to `filter_approval_policy` hooks | 1 day | Low |
| 4 | Add `on_before_turn`/`on_after_tool_exec` for observability | 0.5 day | Low |
| 5 | `HookConfig` extensions in api-gateway | 1 day | Low |
| 6 | Subprocess hook protocol in command-daemon | 2 days | Medium |
| 7 | Manifest discovery + lazy loading in di-core | 2 days | Medium |
| 8 | Wasm/Extism integration in all 5 processes | 3 days | Medium |
| 9 | Deny-wins + fail_mode implementation | 1 day | Low |
| 10 | Tests following MockHandler pattern | 2 days | Low |

## Key Files Generated

| File | Purpose |
|------|---------|
| `dsl-design/hook-points.md` | 34 hook points across 5 processes |
| `dsl-design/rust-hooks.md` | Trait definitions, HookRegistry, integration points |
| `dsl-design/go-hooks.md` | HookConfig, global merge, pipeline wrapping |
| `dsl-design/daemon-hooks.md` | JSON subprocess protocol, hook config file format |
| `dsl-design/manifest-schema.md` | VS Code-style lazy activation manifest |
| `dsl-design/wasm-interface.md` | Extism PDK interface for all 5 languages |
| `dsl-design/synthesis.md` | This file — architecture overview |
