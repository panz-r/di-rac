# Comparative Analysis — Condensed

**Meta-lessons from framework comparisons and production data across all systems.**

## 4 Multi-Agent Patterns (Cost Profile)

Subagents (cheapest) < Handoffs (medium) < Crews (higher) < Conversations (most expensive)

## 2 Framework Categories

| Category | Trade-off |
|----------|-----------|
| **Provider-native SDKs** | Tight integration, vendor lock-in |
| **Independent frameworks** | Model flexibility, abstraction overhead |

## 6 Production Mistakes

1. Infinite loops on tool failure (no circuit breaker)
2. Training data cutoff as current knowledge
3. Unbounded subagent spawning (no depth limit)
4. No budget enforcement (tool calls, tokens, cost)
5. Shared mutable state across hooks (race conditions)
6. No observability in hooks (silent failures)

## Decision Tree

```
Coding agent?             → Claude SDK
Customer service?          → OpenAI SDK
Multi-language enterprise? → Google ADK
Stateful workflows?        → LangGraph
Quick prototyping?         → CrewAI
Type-safe output?          → Pydantic AI
Open-source models?        → Smolagents
Human-in-the-loop?         → AutoGen/MS Agent Framework
Rust-native?               → Custom harness with trait-based hooks
Sandboxed third-party?     → Extism (Wasm plugins)
```

## Key Cross-Cutting Lessons

- **Enforcement > Instructions** — Hooks that block violations > prompts
- **Invest in the harness** — Execution infrastructure > model choice
- **Circuit breakers essential** — Unbounded loops need limits
- **Stable hook signatures** — Versioned APIs prevent migration pain
- **Test hooks in isolation** — Most underdeveloped practice across all systems
- **Plugin-to-plugin coupling must be prevented** — Core event bus only
- **Microkernel fits multi-process architectures** — Your daemons already prove the pattern
