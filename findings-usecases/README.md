# Pi Agents Extension System — Findings & Use Cases

Research gathered to inform the design of an agent-loop hooks DSL for customizing the agent loop.

## Sources

Sources consulted via web research on 2026-05-15:

- [pi-mono Extension System (DeepWiki)](https://deepwiki.com/badlogic/pi-mono/4.4-extension-system)
- [Custom Tools & Event Hooks (DeepWiki)](https://deepwiki.com/badlogic/pi-mono/4.4.2-custom-tools-and-event-hooks)
- [Custom Commands & Shortcuts (DeepWiki)](https://deepwiki.com/badlogic/pi-mono/4.4.3-custom-commands-and-shortcuts)
- [Extension UI Context (DeepWiki)](https://deepwiki.com/badlogic/pi-mono/4.4.4-extension-ui-context)
- [AgentSession Lifecycle & Architecture (DeepWiki)](https://deepwiki.com/badlogic/pi-mono/4.2-agentsession-lifecycle-and-architecture)
- [Agent Loop & State Machine (DeepWiki)](https://deepwiki.com/badlogic/pi-mono/3.1-agent-loop-and-state-machine)
- [Session Management & History Tree (DeepWiki)](https://deepwiki.com/badlogic/pi-mono/4.3-session-management-and-history-tree)
- [Tool Execution & Built-in Tools (DeepWiki)](https://deepwiki.com/badlogic/pi-mono/4.5-tool-execution-and-built-in-tools)
- [Message Transformation & Cross-Provider Handoffs (DeepWiki)](https://deepwiki.com/badlogic/pi-mono/2.3-message-transformation-and-cross-provider-handoffs)
- [Transport Abstraction & Types (DeepWiki)](https://deepwiki.com/badlogic/pi-mono/3.2-transport-abstraction-and-types)
- [Print Mode, RPC Mode & SDK (DeepWiki)](https://deepwiki.com/badlogic/pi-mono/4.11-print-mode-rpc-mode-and-sdk)
- [Skills & Prompt Templates (DeepWiki)](https://deepwiki.com/badlogic/pi-mono/4.8-skills-and-prompt-templates)
- [Authentication & Cost Tracking (DeepWiki)](https://deepwiki.com/badlogic/pi-mono/2.4-authentication-and-cost-tracking)
- [Hooks System (agentic-dev-io/pi-agent DeepWiki)](https://deepwiki.com/agentic-dev-io/pi-agent/5.2-hooks-system)
- [OpenClaw Agent Loop docs](https://docs.openclaw.ai/concepts/agent-loop)
- [OpenClaw Plugin Hooks](https://docs.openclaw.ai/plugins/hooks)
- [Armin Ronacher: Pi: The Minimal Agent Within OpenClaw](https://lucumr.pocoo.org/2026/1/31/pi/)
- [pi-agent-toolkit (bernardjbs)](https://github.com/bernardjbs/pi-agent-toolkit)
- [awesome-pi-agent (qualisero)](https://github.com/qualisero/awesome-pi-agent) — 907★ curated list

## Files

### Core Analysis

| File | Lines | Description |
|------|-------|-------------|
| [agent-loop-hooks.md](agent-loop-hooks.md) | ~100 | **Condensed** — Cross-system hook catalog (Pi 25+, OpenClaw 40+, Hermes 15, CrewAI 4, LangChain 6) with common patterns table |
| [use-cases.md](use-cases.md) | 185 | 17 categories of use cases with 100+ specific examples organized by hook point |
| [design-patterns.md](design-patterns.md) | ~150 | **Condensed** — 12 cross-system design patterns: block protocols, dispatch strategies, modification patterns, injection targets, state hierarchy, error isolation, registration scope, management APIs, multi-system design, forward compatibility, testing |
| [extension-api-surface.md](extension-api-surface.md) | ~200 | **Condensed** — complete registration API, 25+ event catalog, tool/provider schemas from actual source, conflict rules, context types, UI mode adaptability, 6 key design lessons |
| [extension-runner-internals.md](extension-runner-internals.md) | 242 | Event dispatch architecture, dedicated emitter merge strategies, mutable input pattern, error handling, context lifecycle & staleness, shortcut/command conflict resolution, provider registration lifecycle |
| [core-agent-loop-architecture.md](core-agent-loop-architecture.md) | ~200 | **Condensed** — two-level loop structure, queue mechanics, 3-stage tool pipeline, parallel/sequential execution, termination consensus, error recovery, AbortSignal, cross-system loop comparison table (7 systems) |
| [layering-and-tool-wrapping.md](layering-and-tool-wrapping.md) | 340 | Four-layer architecture, wrapToolDefinition bridge, dual hook system interaction, pure function system prompt assembly, tool snippet/guideline system, pluggable operations, 7 DSL design lessons |
| [extension-authoring-patterns.md](extension-authoring-patterns.md) | 453 | 18 concrete patterns from 40+ real examples: tool definition, security, state persistence, dynamic registration, custom compaction, system prompt injection, handoff, inter-extension communication, input transformation, resource discovery, provider registration, custom editors, autocomplete, and 9 anti-patterns |
| [synthesis-lessons-and-patterns.md](synthesis-lessons-and-patterns.md) | ~200 | **Condensed digest** of all findings: 7 universal design decisions every hook system must make, 10 design questions your DSL must answer, 10 patterns worth stealing from Pi/OpenClaw/Hermes, anti-patterns to avoid |
| [ecosystem-comparison.md](ecosystem-comparison.md) | 257 | Three-way comparison of Pi, OpenClaw, and Claude Code extension systems: extension points, subagent models, state management, authoring experience, key takeaways for DSL design |
| [test-driven-insights.md](test-driven-insights.md) | ~130 | **Condensed** — Verified event ordering, cancellation semantics, compaction edge cases, hook implications from Pi's actual test files |
| [scheduling-and-background-patterns.md](scheduling-and-background-patterns.md) | ~150 | **Condensed** — 3 scheduler types, 4 execution styles, background-specific hooks, skip/retry patterns, delivery modes, cost optimization, 6 design implications |
| [community-extensions-catalog.md](community-extensions-catalog.md) | 124 | 50+ real community extensions across 10 categories with authors and descriptions |

### Architecture Deep Dives

| File | Lines | Description |
|------|-------|-------------|
| [pi-vs-openclaw-comparison.md](pi-vs-openclaw-comparison.md) | 152 | Systematic comparison across 15 dimensions, with "what to steal from Pi", "what to steal from OpenClaw", and "what to avoid" |
| [session-persistence-architecture.md](session-persistence-architecture.md) | 150 | JSONL persistence model, tree branching, context reconstruction, compaction architecture, session entry types, hook implications for cross-branch state |
| [message-transformation-pipeline.md](message-transformation-pipeline.md) | 136 | Two-stage message pipeline, cross-provider edge cases (tool call ID normalization, thinking content, orphaned tool calls, image downgrade, Gemini thought signatures), StreamFn contract |
| [rpc-and-sdk-architecture.md](rpc-and-sdk-architecture.md) | 190 | Three execution modes comparison, Print/RPC mode UI context behavior, RPC protocol, SDK classes, mode-aware hook patterns, session re-registration |

### Cross-System References

| File | Lines | Description |
|------|-------|-------------|
| [hermes-agent-hooks.md](hermes-agent-hooks.md) | ~170 | **Condensed** — 3 hook systems (plugin/gateway/shell), 15 events, 3-stage transform pipeline, shell hooks JSON protocol, plugin system |
| [crewai-hooks-system.md](crewai-hooks-system.md) | ~160 | **Condensed** — 4 hook types, two-layer architecture (agent + HTTP), block protocol, context objects, 4 registration methods, management API |
| [langchain-middleware-system.md](langchain-middleware-system.md) | 330 | 2 hook styles (node + wrap), `request.override()`, `ExtendedModelResponse`, dynamic model/tool/prompt, nested middleware composition |
| [vercel-sk-hooks.md](vercel-sk-hooks.md) | 364 | Vercel AI SDK (TypeScript middleware wrapping model calls) + Semantic Kernel (C#/Python filter pipeline, 3 filter types, `next()` delegate) |
| [ag2-hooks-system.md](ag2-hooks-system.md) | ~200 | AG2 per-agent hooks: 4 active + 5 safeguard, permanent vs temporary changes, send/receive hooks, template-based system message updates |

## Key Takeaway

Pi's entire agent loop is accessible via a TypeScript extension API. Every stage — user input, system prompt assembly, LLM call, tool execution, result rendering — is hookable. The ecosystem shows 50+ community extensions across security, UX, workflow automation, monitoring, and integration — all built on this hook-based architecture. The deeper architecture dives (session persistence, message transformation, RPC/SDK) reveal the design constraints any hook DSL must account for.
