# Hook System Report

## 1. The System

The hook system is a cross-process, multi-language extension architecture for
Dirac's five daemon processes. It spans Rust (di-core engine), Go (api-gateway),
and C (command-daemon, central-daemon, treesitter-daemon), united by three
design pillars:

### 1.1 Two Hook Types — Actions and Filters

Every hook point in every process supports exactly two hook types:

- **Action** (observe, void return): fired for observability, logging, metrics,
  telemetry. Never blocks or modifies. Example: `on_after_tool_exec` logs the
  tool result to a file.

- **Filter** (transform or block, return modified): receives input, returns
  modified input or a denial. Example: `filter_tool_call` checks a bash command
  for dangerous patterns and denies execution if found.

### 1.2 Fail Mode Per Hook — Open or Closed

Every hook declares its failure mode:

- **`fail_open`** (default): on error, log and continue. Safe for observability
  hooks that should never crash the agent.

- **`fail_closed`**: on error, deny the operation. Required for security hooks
  where missing a check is worse than a false positive.

### 1.3 Deny-Wins Composition

When multiple filters are registered on the same hook point, they run in
priority order (lower number = earlier). The first filter to return `Deny`
short-circuits all subsequent filters. This is seccomp's proven model:
the most restrictive policy wins, providing defense in depth.

---

### Per-Process Implementation

#### di-core (Rust) — Tower-Inspired Trait System

```rust
#[async_trait]
pub trait ActionHook<C: Send + Sync>: Send + Sync {
    fn id(&self) -> HookId;
    fn fail_mode(&self) -> FailMode { FailMode::Open }
    async fn call(&self, ctx: &C);
}

#[async_trait]
pub trait FilterHook<I: Send + Sync, O: Send + Sync>: Send + Sync {
    fn id(&self) -> HookId;
    fn fail_mode(&self) -> FailMode { FailMode::Open }
    async fn call(&self, input: I) -> FilterResult<O>;
}
```

A `HookRegistry` holds typed vectors (`Vec<Box<dyn ActionHook<C>>>`) for each
of 16 hook points. Execution helpers (`run_actions`, `run_filters`) iterate
the vector, applying fail_mode and deny-wins logic uniformly. This replaces
hardcoded dispatch in `run_preflight_firewall`, `ApprovalManager`,
`run_turn` prologue/epilogue, and the streaming loop.

#### api-gateway (Go) — Function Hook Fields

Extends the existing and proven `OpenAICompatConfig` pattern (already used by
30+ providers). New hook points are function fields on a config struct:

```go
type HookConfig struct {
    ModifyRequest, ModifyHeaders, ModifyMessages  // existing
    OnBeforeSend, OnAfterSend, OnStreamChunk, FilterResponse, OnRateLimit  // new
}
```

Global hooks (applied to ALL providers) merge with per-provider hooks via
`Merge()`, with per-provider taking precedence.

#### C Daemons — JSON Subprocess Protocol

Daemons execute hook scripts as subprocesses with JSON-in/JSON-out:

```json
// stdin to hook process:
{ "hook": "filter_command", "context": { "command": "rm -rf /", ... } }
// stdout from hook process:
{ "status": "ok", "action": "deny", "reason": "Dangerous recursive deletion" }
```

Hook config loaded from `DIRAC_HOOK_CONFIG` — a JSON file listing each hook's
point, type, executable path, timeout, fail_mode, and priority.

#### Third-Party — Wasm via Extism

The same Wasm module works in all 5 processes via Extism PDK. Sandboxed:
no filesystem, no network, 16MB memory max, 5000ms timeout. ~0.1ms FFI
overhead per call. Extensions are written in any language that compiles to
Wasm (Rust, Go, C, TypeScript/AssemblyScript, Python).

#### Lazy Activation — Declarative Manifest

VS Code pattern: JSON manifests in `~/.dirac/hooks/<name>/manifest.json`
declare capabilities without loading any code. The daemon scans manifests at
startup to discover which hook points are available, but only loads the
Wasm/native code when the event actually fires. Activation can be gated on
conditions (`when: { tool_name: "bash" }`) so hooks are loaded only when
they can meaningfully act.

---

## 2. Benefits

### 2.1 Extensibility Without Forking

Currently, any new behavior requires modifying the core loop. A custom security
policy, a new observability sink, or a dynamic tool gating rule all require
editing `engine.rs` and recompiling. With hooks, each of these becomes a
declarative manifest + 50 lines of code in a separate file. The core is
never touched.

This is the architectural precondition for Dirac to have a third-party
extension ecosystem (like VS Code's, like Pi's, like WordPress's). The
only extension point today — `ContextDistiller` — proves the demand exists.

### 2.2 Defense in Depth

The deny-wins + fail_closed pattern means security hooks compose safely.
If you install both "block curl-pipe-bash" and "block rm -rf /", both run
and either can deny independently. If one has a bug, fail_closed ensures
you err on the side of blocking rather than allowing. This is seccomp's
15-year proven model applied at the agent loop level.

### 2.3 Observability Without Risk

fail_open hooks can observe every tool call, every turn, every compaction
event without any risk of blocking the agent. Observability hooks cannot
accidentally deny an operation because Actions have no return value.
This separation — baked into the type system — is WordPress's 20-year
lesson: Actions ≠ Filters.

### 2.4 Performance Where It Matters

Rust-native hooks via Tower-style Service/Layer traits cost 0.018ms per
invocation — 2,143x faster than the subprocess alternative (37ms/call).
For hot paths (tool call filter, stream chunk processing, context frame
assembly) where 400-600 invocations per turn are expected, this matters.
The trait system lets compile-time-known hooks pay zero runtime cost.

### 2.5 Sandboxed Third-Party Extensions

Wasm via Extism lets third-party authors write hooks without access to the
system's filesystem or network. No recompilation of the host. No risk of
crashing the agent. This is the prerequisite for a public extension
registry — the model that made VS Code the dominant editor.

### 2.6 Gradual Migration

The 10-phase implementation plan starts with 3-4 hook points in di-core
(2 days, low risk) and migrates existing hardcoded logic one piece at a
time. `run_preflight_firewall` becomes `filter_tool_call` hooks.
`ApprovalManager` becomes `filter_approval_policy` hooks.
`run_turn` prologue becomes `on_before_turn` + `filter_context_frame`.
Each migration is self-contained, testable, and reversible.

No big-bang rewrite. The hook system coexists with existing code during
migration.

### 2.7 Cross-Process Consistency

The same two-type model, the same fail_mode semantics, the same deny-wins
composition, and the same Wasm runtime work in all 5 processes. A security
hook that blocks dangerous bash commands can run:
- in the Rust engine (before dispatch)
- in the Go gateway (before sending to LLM)
- in the command-daemon C process (before execution)
- as a Wasm module loaded by any of the above

The choice is deployment-specific, not architecture-specific.

---

## 3. How It Builds on the Research

The 49 findings files covered 20+ systems across 6 programming languages.
Every design decision in this hook system is a direct response to specific
lessons from that material.

### Two Types: Actions + Filters

**From:** WordPress (20+ years, 60,000+ plugins), Kubernetes admission
webhooks (mutate + validate).

WordPress proved that two types cover every use case. Actions for "I want
to know when X happens" and Filters for "I want to change X before it
happens". Every hook need we identified in di-core — from tool call
filtering to context frame assembly to error observation — maps to one
of these two types. Kubernetes independently confirmed the pattern with
its mutate (filter) and validate (observe/block) admission webhooks.

### Deny-Wins Composition

**From:** Linux seccomp (15+ years, kernel-level), Kubernetes failure
policy.

seccomp applies the most restrictive policy among all loaded filters.
Kubernetes's `FailurePolicy` enum (Fail/Ignore) maps directly to
fail_closed/fail_open. Both systems learned through production outages
that security hooks MUST fail closed by default. Our design bakes this
into the per-hook `FailMode` field.

### Lazy Activation + Declarative Manifest

**From:** VS Code (10+ years, 50,000+ extensions), Eclipse/OSGi,
IntelliJ (1,115 extension points).

VS Code proved that scanning manifests at startup (JSON only, no code)
scales to thousands of extensions. Eclipse proved that lazy instantiation
(virtual proxy pattern) avoids startup cost. IntelliJ proved that
activation conditions (when/filter predicates) prevent loading extensions
that have nothing to do. Our manifest schema's `when` field is directly
modeled on IntelliJ's approach.

### Phase Ordering

**From:** Kubernetes admission webhooks, OpenClaw plugin hooks.

Kubernetes proved that phase-level ordering (mutate → validate → execute)
is simpler and more reliable than per-hook ordering. OpenClaw's 40+ hook
types confirmed that phases reduce cognitive load. Our hook points are
assigned to implicit phases (pre-execution → execution → post-execution)
with per-hook priority within each phase.

### Tower Service/Layer for Rust

**From:** Rust Tower ecosystem, zero-cost abstraction middleware.

Tower's `Service` and `Layer` traits provide compile-time composition
with zero runtime overhead. For hooks known at compile time, we provide
an alternative trait path (`ToolCallLayer`, `ToolCallService`) that costs
0.018ms per invocation. Subprocess hooks at 37ms/call are only acceptable
for non-performance-critical paths.

### Per-Hook Timeout Budgets

**From:** OpenClaw, Pydantic AI, Cline SDK.

Every hook gets a timeout. Wasm hooks default to 5000ms. Subprocess hooks
to 2000-5000ms. Rust-native hooks to 0 (synchronous). A hung hook never
hangs the agent loop. This is OpenClaw's explicit timeout field per plugin
and Pydantic AI's per-step timeout.

### Go Function Hooks (Existing Pattern Extensions)

**From:** Your own api-gateway (30 providers, `ModifyRequest`/`ModifyHeaders`/
`ModifyMessages`).

The Go side of the design does not introduce new patterns — it extends
existing ones. `OnBeforeSend`, `OnAfterSend`, `OnStreamChunk` follow
the exact same function-field-on-config convention that already works
for 30+ providers. This was the single strongest signal from the research:
your codebase already has a working hook pattern. The design standardizes
and extends it.

### Wasm via Extism

**From:** Extism project, WebAssembly plugin systems research.

Extism's PDK support for Rust, Go, and C means one Wasm module works in
all 5 processes. The sandboxing model (no FS, no network, bounded memory
and time) matches the security requirements for running third-party code.
The ~0.1ms FFI overhead is acceptable for all but the hottest paths.

### MockHandler Testing Pattern

**From:** Your own api-gateway tests (`SendFunc`/`StreamFunc` + call
counters + sensible defaults).

The hook testing strategy follows this exact pattern. Each hook type gets
an injectable function variant for tests, call counters for assertions,
and sensible defaults (no-op hooks that pass through). This pattern is
already proven in your codebase at `providers/provider_test.go:11-28`.

### Deliberate Minimalism

**From:** Pi creator Mario Zechner ("If I don't need it, it won't be
built", "4 tools are sufficient with an extension system").

The design starts with 16 hook points in di-core — not 40, not 1115.
These are the natural boundaries we identified in the existing loop
(run_preflight_firewall, compaction, approval, error routing, turn
lifecycle, streaming). Additional hook points are added only when a
real use case demands them. The two-type system (Actions + Filters)
is intentionally minimal: it covers every case without introducing
a third or fourth type that would complicate the mental model.

---

## 4. Summary

The hook system is a direct synthesis of 20+ systems' lessons:

| Decision | Source | Why It Matters |
|----------|--------|---------------|
| Two types (Actions + Filters) | WordPress, K8s | Covers all use cases, simple mental model |
| Deny-wins composition | seccomp, K8s | Defense in depth, safe composition |
| fail_closed default for security | K8s production outages | Safety over convenience for security |
| Lazy activation via manifest | VS Code, Eclipse, IntelliJ | Scales to many extensions, fast startup |
| Tower traits for Rust | Rust ecosystem | Zero-cost for hot paths |
| Function hooks for Go | Your 30+ providers | Proven pattern, no new concepts |
| JSON subprocess for C daemons | Your existing daemon protocols | Minimal overhead, easy to implement |
| Wasm/Extism for third-party | Extism, Wasm ecosystem | Sandboxed, language-agnostic, cross-process |
| Per-hook timeouts | OpenClaw, Pydantic AI | One hung hook never hangs the loop |
| Gradual migration | — | No big-bang rewrite, each step is testable |
| Deliberate minimalism | Pi creator philosophy | Add hook points for use cases, not speculation |

The system is ready for implementation starting with Phase 1: `HookRegistry`
in di-core with 3-4 hook points, estimated at 2 days with low risk.
