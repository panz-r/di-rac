# Ecosystem Comparison — Quick Reference

**20+ systems, 5 key dimensions.**

## By Hook Style

| Style | Systems | Example |
|-------|---------|---------|
| **Event-based** | Pi, Hermes, AG2 | `pi.on("tool_call", handler)` |
| **Middleware** | LangChain, Vercel AI, Genkit, Rust Tower | `@wrap_model_call` |
| **Plugin/directory** | Claude Code, Dify, Cline SDK | `plugin.yaml` + code |
| **Extension points** | Eclipse, IntelliJ, Jenkins | XML manifests, type-safe interfaces |
| **Breakpoints** | Haystack | `Breakpoint(component, visit_count)` |
| **Shell hooks** | Letta, GitHub Copilot | Config-driven shell scripts |
| **Filters** | Semantic Kernel, WordPress | `next()` delegate pattern |
| **Two-type** | WordPress (Actions + Filters) | Observe + Transform |

## By Hook Count

| Count | Systems |
|-------|---------|
| 2 types | WordPress |
| 4-6 hooks | CrewAI, LangChain, OpenAI SDK |
| 10-15 hooks | Hermes, Letta, Cline SDK, AG2 |
| 25+ hooks | Pi, OpenClaw |
| 40+ organized | Pydantic AI (10 categories × 4) |
| 1000+ extension points | IntelliJ, Eclipse, Jenkins |

## By Language

| Language | Systems |
|----------|---------|
| TypeScript | Pi, OpenClaw, Claude Code, Cline SDK, Genkit, Vercel AI |
| Python | Hermes, CrewAI, LangChain, AG2, Pydantic AI, Haystack, Smolagents, OpenAI SDK |
| Rust | **Your di-core**, Tower |
| Go | **Your api-gateway** |
| C | **Your daemons** (command, central, treesitter) |
| Java | Eclipse, IntelliJ, Jenkins |
| PHP | WordPress |

## Your Architecture vs Industry

Your multi-process Rust+Go+C system is unique. Most agent systems are single-language. The closest analog is VS Code (TypeScript main + C++/Rust extensions + JSON protocols), but your system has more language diversity, which means your hook DSL must work across process and language boundaries.
