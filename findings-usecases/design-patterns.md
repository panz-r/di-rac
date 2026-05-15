# DSL Design Patterns — Quick Reference

**Cross-system patterns. For full detail see individual files.**

## Block Protocols

| System | Syntax | Terminal? |
|--------|--------|-----------|
| Pi | `{ block: true, reason }` | Yes |
| OpenClaw | `{ block: true }` + `requireApproval` | Yes |
| Hermes | `{"action": "block"}` | Yes |
| CrewAI | `return False` | Yes |
| LangChain | `jump_to: "end"` | Yes |
| Pydantic AI | `SkipModelRequest` exception | Yes |
| Letta | Exit code 2 | Yes |
| **Kuberenetes** | `{"allowed": false}` | Yes |

## Context Injection

System prompt (Pi, OpenClaw) = cache-invalidating but authoritative. User message (Hermes) = cache-friendly. Both (OpenClaw, LangChain) = granular.

## State Hierarchy

Per-run scratch → next-turn exactly-once → per-session persistent → permanent storage.

## Error Isolation

Every system: `try: handler(); catch: log; continue`. **Non-negotiable.**

## Scope Options

Global (all), per-plugin (Pi, OpenClaw), per-agent (CrewAI, OpenAI SDK), per-invocation (LangChain, Genkit).

## K8s Admission Webhooks (New)

Two types (mutate + validate = transform + observe/block). Failure policy (`Fail`/`Ignore` = `fail_closed`/`fail_open`). Match conditions (CEL = declarative tool filtering). `reinvocationPolicy: IfNeeded` (re-run hooks if prior hooks changed context). Phase ordering beats per-hook ordering.

## WordPress Two-Type System

Actions (observe, void) + Filters (transform, return modified). Maps ALL agent hook use cases. Numeric priority (10 default, 5 early, 15 late). Namespaced hook names (`component/hook_name`). Hook removal API (`remove_action()`).

## For Your DSL

Two types: `on(action)` for observation, `filter(transform)` for modification. Failure mode per hook. Lazy activation (VS Code). Declarative manifest (Eclipse/IntelliJ). Tower Service/Layer for Rust composition.
