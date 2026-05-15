# Dirac Agent Guide

This is the codebase of our coding agent Dirac. It consists of a Rust TUI frontend (`divrr`), a Rust agent engine (`di-core`), a Go API gateway (`api-gateway`), and supporting C daemons. The TypeScript codebase (`src/`, `cli/`) and npm build system have been fully removed.

## System Architecture

```
divrr (Rust TUI)          — Terminal UI, user input, settings panel
  │
  │  JSON over stdin/stdout (FrontendMessage / CoreEvent)
  │
di-core (Rust engine)     — Agent loop, tool execution, context management
  │
  │  JSON over Unix socket (GatewayRequest / stream chunks)
  │
api-gateway (Go)          — LLM API proxy, provider routing, rate limiting
  │
  ▼
Provider APIs (OpenAI, Anthropic, MiniMax, etc.)
```

Supporting daemons (launched by di-core):
- `command-daemon` (C) — Child process execution with timeout/safety enforcement
- `treesitter-daemon` (C, CMake) — AST analysis via vendored tree-sitter C library; out-of-memory guarded, bounds-checked
- `central-daemon` (C) — Coordination daemon with trie-based file routing

## 🏗️ Codebase Modules

### Rust

- **`divrr/`** — Terminal UI (ratatui/crossterm). Manages gateway and di-core child processes, settings dialog (provider selection, model listing, API keys), conversation rendering, and key bindings.
  - `src/gateway.rs` — api-gateway child process launch and socket path management
  - `src/backend.rs` — di-core child process management
  - `src/settings.rs` — Settings dialog state, `GatewayConnection` for gateway queries
  - `src/settings_model.rs` — Settings types and gateway query functions (model listing, validation)
  - `src/event.rs` — Core event handler (tool calls, approval, thought deltas)
  - `src/ui/` — Ratatui rendering components
  - `src/message.rs` — Protocol message types (matching di-core's `FrontendMessage`)

- **`di-core/`** — Agent engine. Manages conversation trajectory, streaming LLM calls, tool execution, observer/context distillation.
  - `src/agent/engine.rs` — Main agent loop (`run_task`, `run_turn`), tool approval, mistake limits
  - `src/agent/parser.rs` — `StreamingToolAccumulator` for tool call extraction from SSE
  - `src/agent/` — Agent state, recovery/circuit breakers
  - `src/context/` — Context management, budget tracking, reranking
  - `src/daemons/mod.rs` — Gateway stream client and resilient daemon wrappers
  - `src/tools/` — Tool definitions and execution
  - `src/observer/` — Observer agent for context compression and monitoring
  - `src/prompt/` — System prompt assembly (stable + session + dynamic context)
  - `src/protocol/` — `FrontendMessage` and `CoreEvent` protocol types
  - `src/main.rs` — Entry point: stdin loop, routing SetProviderConfig/SpawnAgent messages

- **`treesitter-daemon/`** — AST analyzer daemon. Tree-sitter language grammars and symbol extraction.

### Go

- **`api-gateway/`** — LLM API proxy over Unix socket (NDJSON protocol).
  - `main.go` — Unix socket server, connection handling, message dispatch, streaming
  - `providers/provider.go` — Registry, handler interface, model listing cache, HTTP client
  - `providers/openai_compat.go` — OpenAI-compatible SSE streaming parser, model listing
  - `providers/capabilities.go` — Type definitions (ModelEntry, ProviderInfo, settings schema)
  - `providers/minimax.go` — MiniMax handler with XML tool call parsing
  - `providers/mistral.go`, `anthropic.go`, `gemini.go`, etc. — Per-provider implementations
  - `codex_oauth.go` — OpenAI Codex OAuth flow

### C

- **`command-daemon/`** — Child process execution daemon. Spawns, pipes I/O, enforces timeouts, JSON-over-stdout protocol.
- **`central-daemon/`** — Coordination daemon. Trie-based file routing, links against `draugr` hash table library.
- **`draugr/`** — Standalone C11 hash table library (Robin-Hood, Graveyard, Zombie hashing). Static dependency of central-daemon.

## 📂 Important Files

- `Makefile` — Single build system. Targets: `build`, `build-api-gateway`, `build-command-daemon`, `build-treesitter-daemon`, `build-divrr`, `build-di-core`, `install`, `clean`
- `.gitignore` — Ignores `bin/`, `dist/`, runtime state dirs, build artifacts
- `AGENTS.md` — This file

## 🔌 API Providers (api-gateway)

All providers are implemented in `api-gateway/providers/`. Each implements the `Handler` interface. Key providers:

| Provider | File | Protocol |
|----------|------|----------|
| MiniMax | `minimax.go` | OpenAI-compatible + XML tool calls |
| Mistral | `mistral.go` | OpenAI-compatible |
| Anthropic | `anthropic.go` | Anthropic native SDK |
| Gemini | `gemini.go` | Google Gemini SDK |
| OpenAI | `openai.go` | OpenAI SDK |
| DeepSeek | `deepseek.go` | OpenAI-compatible |
| Groq | `groq.go` | OpenAI-compatible |
| Together | `together.go` | OpenAI-compatible |
| OpenRouter | `openrouter.go` | OpenAI-compatible |
| opencode-go | `opencode_go.go` | OpenAI-compatible (router) |

Model metadata (IDs, pricing, capabilities) is in each handler's `Capabilities()` method.

## 🛠️ Dev Flow

- **Build all**: `make` or `make build`
- **Build fast (gateway only)**: `make build-fast`
- **Build individual**: `make build-api-gateway`, `make build-divrr`, etc.
- **Build to `bin/`**: all targets copy their binary to `bin/` automatically
- **Run TUI**: `./bin/divrr --core ./bin/di-core --task "<task>"` (starts api-gateway and di-core automatically)
- **Run engine only**: `./bin/di-core` (reads FrontendMessage NDJSON from stdin)
- **Run gateway alone**: `./bin/api-gateway`
- **Test all**: `make test` (Rust `cargo test` + Go `go test`)
- **Test divrr**: `cargo test --manifest-path divrr/Cargo.toml`
- **Test di-core**: `cargo test --manifest-path di-core/Cargo.toml`
- **Test gateway**: `go test ./api-gateway/...`

## 🏃 Runtime

Binaries are built to `bin/`. The TUI (`divrr`) launches `api-gateway` and `di-core` as child processes, locating them in order:
1. Same directory as the `divrr` binary
2. `$PWD/bin/`
3. `$HOME/bin/`
4. PATH

Runtime state is stored in `~/.di/`:
- `provider-settings.json` — Saved provider configs per role
- `logs/` — Agent engine log (`di-core.log`), TUI log (`divrr.log`)
- `state/` — Persistent agent state (daemon state, recovery data)
- `api-gateway-<pid>.sock` — Gateway Unix socket

## Protocol: divrr ↔ di-core (JSON over stdin/stdout)

`divrr` spawns `di-core` as a child process and communicates via NDJSON on stdin/stdout.

**FrontendMessage** (divrr → di-core):
- `SetProviderConfig { role, provider, model, api_key, base_url, params }` — Configure provider per role (act, plan, distiller, observer)
- `SetObserverConfig { ... }` — Observer behavior settings
- `SpawnAgent { task }` — Start a new agent with the given task
- `UserResponse { agent_id, text }` — User message reply
- `ApprovalResponse { agent_id, approved }` — Tool call approval
- `FollowupAnswer { agent_id, text }` — Follow-up question answer
- `Interrupt { agent_id }` — Abort the current turn

**CoreEvent** (di-core → divrr):
- `ThoughtDelta { text, thinking }` — Streaming text/thinking delta
- `ThoughtFinished` — End of thinking block
- `ToolCallStarted { call_id, tool, args }` — Tool call awaiting approval
- `ToolCallFinished { call_id, result }` — Tool execution result
- `TaskInitialized`, `TaskFinished` — Task lifecycle
- `ApprovalNeeded { ... }` — Tool approval prompt
- `MetricsUpdate` — Token usage and latency

## Protocol: di-core ↔ api-gateway (JSON over Unix socket)

`di-core` connects to the gateway's Unix socket and sends requests as NDJSON lines.

**GatewayRequest** (di-core → gateway):
- `id`, `stream` — Request ID and streaming flag
- `provider` — `ProviderConfig { id, api_key, base_url, model, extra }`
- `messages` — Conversation messages (role, content, tool_calls, thinking)
- `system` — System prompt
- `tools` — Available tool definitions
- `max_tokens`, `temperature`, `timeout`

**Response** (gateway → di-core):
- Type `"delta"` — Text delta, thinking delta, or tool call fragment
- Type `"stop"` — End of response with finish reason and usage
- Type `"complete"` — Stream complete signal

## Note on grep/search

Avoid searching in the following directories as they contain large generated files or binary data:

- `bin/` — Build output
- `dist/` — Legacy build output
- `build/` — CMake build artifacts
- `.git/`
- `**/target/` — Cargo build artifacts
- `**/vendor/` — Vendored dependencies
- `command-daemon/build/`
- `treesitter-daemon/build/`
- `di-core/target/`
- `divrr/target/`
- `node_modules/` (if present)
