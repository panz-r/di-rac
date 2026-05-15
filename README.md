<p align="center">
  <img src="di-vrr-logo.png" alt="di-vrr" width="320">
</p>

# di-rac

A fork of [dirac-run/dirac](https://github.com/dirac-run/dirac), a coding agent focused on efficiency and context curation. **divrr** replaces the original VS Code extension and TypeScript runtime with native components — a Rust agent engine, a Rust TUI, a Go API gateway, and a C tree-sitter daemon for code analysis. The entire system builds from a single `Makefile`.

## Architecture

```
┌─────────┐   JSON/stdin   ┌──────────────┐   HTTP/streaming   ┌──────────────┐
│  divrr   │ ◄───────────► │   di-core    │ ◄────────────────► │ api-gateway  │ ◄──► LLM providers
│ (Rust    │   (NDJSON)    │ (Rust engine)│    (NDJSON/UDS)    │    (Go)      │
│  TUI)    │               │              │                    │              │
└─────────┘               └──┬─────────┬──┘                    └──────────────┘
                             │         │
                  stdin/stdout│         │stdin/stdout
                  NDJSON      │         │NDJSON
                     ┌────────▼───┐  ┌──▼─────────────┐
                     │ treesitter │  │ command-daemon  │
                     │ daemon     │  │ (C — child      │
                     │ (C)     │  │  process exec)  │
                     └────────────┘  └─────────────────┘
```

| Component | Language | Role |
|-----------|----------|------|
| `di-core/` | Rust | Agent engine: streaming LLM loop, tool dispatch, context compilation |
| `divrr/` | Rust | Terminal UI (ratatui): conversation view, approval flow, settings |
| `api-gateway/` | Go | LLM proxy: streaming NDJSON over UDS, multi-provider support |
| `treesitter-daemon/` | C | Code analysis: AST outline/skeleton, symbol search, repo map |
| `command-daemon/` | C | Shell command execution with sandboxing and output truncation |
| `central-daemon/` | C | Session state and coordination (in development) |

Components communicate via **NDJSON over Unix domain sockets** (di-core ↔ api-gateway) and **stdin/stdout piped NDJSON** (divrr ↔ di-core, di-core ↔ tree-sitter daemon, di-core ↔ command-daemon).

## What's built so far

### Agent engine (di-core — Rust rewrite of the TS agent runtime)

- **Streaming LLM loop** — full request/response cycle with delta streaming, tool call accumulation, text/tool-use interleaving
- **CLI-modelled tool system** — file I/O, bash execution, code search, context management, and agent interaction via structured tool calls
- **Tool coordination** — result caching, retry with exponential backoff, auto-correction for truncated output, same-input guard
- **Structured error handling** — discriminated `ToolResponse` with error codes, severity classification, recoverability hints, LLM-facing formatting
- **Context compilation** — priority-based prompt building (Critical/Important/Normal), stale-read exclusion for edited files, oversized first-message truncation with task state fallback
- **Context lifecycle** — compaction triggers, turn metrics, adaptive thresholds, reranking pipeline for long trajectories
- **Observer role** — non-acting mode that monitors agent activity without executing tools, for supervision and auditing
- **Background command tracking** — fire-and-forget bash with log files and `--await` polling
- **Artifact store** — large tool outputs stored on disk with token-budget eviction and `artifact://` references
- **Context distillation** — model-backed distiller framework scaffolded (prompts, schemas, validation, admission); `NoopDistiller` wired as placeholder

### Single-token base-32 content-hash anchors

- 3-char `[0-9a-v]` anchors — deterministic, collision-resistant, always a single token for every major LLM tokenizer

### Progressive file exploration

- `skeleton` / `outline` / `expand` modes let the model pay for structure first, bodies later

### TUI (divrr)

- **Conversation rendering** — collapsible blocks for user/assistant/tool/system messages with streaming indicator
- **Approval flow** — tool approval prompts with inline response
- **Follow-up questions** — ask/followup tool with option display
- **Settings panel** — provider selection, model configuration, API key management
- **Palette theming** — configurable color schemes
- **Gateway connection** — connects to di-core over UDS, streams agent events

### API gateway (Go)

- **Multi-provider support** — OpenAI, Anthropic, Google Gemini, Codex (OAuth), DeepSeek, MiniMax, Mistral, Groq, Together, OpenRouter, opencode-go router
- **Streaming proxy** — NDJSON chunk streaming over UDS with usage tracking
- **Model listing** — provider-agnostic model discovery with 1-hour TTL cache
- **Provider persistence** — saves/loads provider config to disk

### Tree-sitter daemon (Rust)

- **AST operations** — outline, skeleton, symbol search, repo map
- **SQLite-backed** — persistent symbol index across restarts
- **Memory safety audit** — NULL checks, OOM guards, buffer overflow protection across all C code

## What's removed from upstream

- VS Code extension and webview — removed entirely
- Headless browser tool
- Browser-use / screenshot capabilities
- The entire TypeScript codebase (`src/`, `cli/`) — replaced by di-core + divrr
- npm/Node.js build infrastructure — replaced by `make`

## Building

```bash
make
```

All binaries are built to `bin/`:
- `bin/api-gateway` — LLM proxy (Go)
- `bin/divrr` — Terminal UI (Rust)
- `bin/di-core` — Agent engine (Rust)
- `bin/di-rvv-cmd` — Command daemon (C)
- `bin/di-rvv-analyzer` — Tree-sitter daemon (Rust)

## Running

```bash
./bin/divrr --core ./bin/di-core
```

Or with an initial task:

```bash
./bin/divrr --core ./bin/di-core --task "Review the codebase"
```

divrr starts the api-gateway and di-core as child processes automatically.
The gateway and daemon binaries are found relative to the divrr binary,
in `$PWD/bin/`, or in PATH.

## License

Apache 2.0 — same as upstream. Thanks to the Dirac authors for the foundation.
