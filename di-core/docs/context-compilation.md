# Context Compilation System — Design Report

## Overview

The context compilation system is responsible for assembling everything the LLM sees on each turn: the system prompt, tool definitions, and conversation history. It is built on two pillars:

1. **Three-layer system string** — a stable-to-dynamic ordering that maximizes provider-side prompt cache hits
2. **Structured tool response pipeline** — a discriminated-union flow where handlers produce structured data, the runtime routes it, and the prompt layer formats it for the LLM

The system now includes five additional hardening layers:
3. **State freshness** — file-version tracking, cache invalidation, content-aware staleness detection
4. **Region-aware history selection** — priority-bucket message selection preserving critical context
5. **Tool result compaction** — artifact store with digest formatting for large outputs
6. **Frame diagnostics** — per-region hash tracking and first-changed-region detection
7. **Pluggable token estimation** — trait-based estimation supporting model-specific calibration

---

## Part I: Three-Layer System String

### Architecture

The `system` field sent to the API gateway is assembled from three layers:

```
full_system = STABLE_PREFIX + SESSION_INFO + DYNAMIC_SUFFIX
              ─────────────   ────────────   ──────────────
              compile-time    once/session   every turn
              never changes   env + config   runtime state
```

### Layer 1: STABLE_PREFIX (compile-time constant)

**Lifecycle**: Baked into the binary via `LazyLock`.

**Content**: 14 sections — Agent Role, Prime Directives, Code Exploration (Cost Ladder), Tool Use (parallel), Universal Flags, Response Format, Hint Examples, Budget Awareness, Error Recovery, Context Management, Security, Decision Rules, Act vs Plan Mode, Feedback.

**Source**: Ported verbatim from the TypeScript system prompt template.

**Size**: ~4KB (~1,000 tokens)

### Layer 2: SESSION_INFO (computed once per agent session)

**Lifecycle**: Computed on first turn. Cached for the agent's lifetime.

**Content**: System Info (OS, shell, CWD, CPU cores, path rules, autonomous mode), Plan Mode notice, Skills, Custom Instructions.

**Size**: ~500 bytes (~125 tokens) without custom instructions

**Cache behavior**: `STABLE_PREFIX + SESSION_INFO` is precomputed into `cached_session_prefix`. Byte-identical across all turns.

### Layer 3: DYNAMIC_SUFFIX (recomputed every turn)

**Content**:
1. **File Context** — validity-aware: "read, unchanged" vs "read, then edited; previous read may be stale"
2. **Past Observations** — filtered by API intersection + confidence threshold
3. **Background Command Summary** — running commands with IDs

If all empty, the dynamic suffix is omitted entirely.

---

## Part II: Tool Definitions

13 tools sent via the gateway's `tools` field in Anthropic-canonical format. Static `LazyLock`, ~2,500 tokens.

| Tool | Category | Purpose |
|------|----------|---------|
| `read` | File | Read files with detail levels and section jumping |
| `write` | File | Create or overwrite files |
| `edit` | File | Edit files using old_text/new_text replacements |
| `search` | Search | Regex search across files |
| `repo` | Navigation | Repository structure overview |
| `bash` | Execution | Shell command execution with background support |
| `compact` | Context | Compress conversation history |
| `ask` | Interaction | Ask user follow-up questions |
| `done` | Interaction | Mark task complete |
| `symbols` | Code | AST symbol operations |
| `plan` | Planning | Propose a plan |
| `task` | Lifecycle | Create a new task |
| `tools` | Meta | List available tools |

The API gateway transforms the format per-provider automatically.

---

## Part III: Conversation Messages

Messages array contains only `user`, `assistant`, and `tool` roles. System-role messages are filtered out (broken on Anthropic/Gemini if left in).

### Region-Aware History Selection

Messages are classified into priority buckets instead of a simple backward walk:

| Priority | Region | Always Included? |
|----------|--------|-----------------|
| **Critical** | Initial task (first user message) | Yes |
| **Critical** | User corrections (subsequent user messages) | Yes |
| **Critical** | Latest assistant intent | Yes |
| **Important** | Latest 3 tool results | If budget allows |
| **Normal** | Last 5 turn pairs | If budget allows |
| **Expendable** | Older messages | Dropped first |

**Budget**: `token_limit - 4000` tokens. Regions are filled from highest to lowest priority. Within the same priority, older messages are dropped first.

This ensures:
- The initial task instruction is **never** dropped
- User corrections ("don't do X") are **never** dropped
- The latest assistant reasoning is preserved
- Old full file reads are dropped first

---

## Part IV: Provider Handling

| Provider | System Field |
|----------|-------------|
| Anthropic | Native `system` parameter with ephemeral cache control |
| OpenAI | Prepended as system message |
| Gemini | `SystemInstruction` field |
| Responses API | `instructions` field |

---

## Part V: Structured Tool Response Pipeline

### ToolResponse (Discriminated Union)

```
Success { data, metadata }     — tool operated, result may be positive or negative
Failure { error, metadata }    — tool could not operate (daemon down, permission denied, timeout)
```

**Tool failure ≠ domain failure**: `bash` returning exit code 1 is `Success` (domain result). Daemon being down is `Failure`.

### ToolError Structure

```
code: ToolErrorCode           — 18 codes across 6 categories
message: String               — for logging, NOT shown to LLM
severity: Warning|Error|Critical
recoverability: Retryable|RetryableAfterRefresh|RequiresReplan|RequiresUserInput|NonRetryable
details: Option<Value>        — selective (paths yes, traces no)
remediation: Option<Remediation>
metadata: ErrorMetadata       — tool name, timestamp, retry count, input_hash
```

### Error Code Taxonomy

| Category | Codes | Default Severity |
|----------|-------|-----------------|
| IO | `io.file.notFound`, `io.file.permissionDenied`, `io.file.changed` | Warning (notFound), Error (others) |
| Editing | `anchor.notFound`, `anchor.ambiguous`, `patch.applyFailed`, `patch.conflict` | Error |
| Shell | `shell.exitNonZero`, `shell.timeout`, `shell.blocked` | Warning (exitNonZero), Error (others) |
| Daemon | `daemon.unavailable`, `daemon.timeout` | Critical (unavailable), Error (timeout) |
| Validation | `validation.missingArgument`, `validation.invalidInput` | Error |
| General | `tool.internalError`, `tool.rateLimited`, `unknown`, `context.stale` | Critical/Warning/Error |

### ErrorRouter

Routes failures based on recoverability:

| Recoverability | Decision |
|---------------|----------|
| NonRetryable | Continue (return formatted error to model) |
| RequiresUserInput | Escalate |
| RequiresReplan | Abort |
| Retryable | Retry with exponential backoff (per-code max) |
| RetryableAfterRefresh | Continue (return to model for re-read) |

**Same-input guard**: Key is `(tool_name, error_code, input_hash)`. If same error on same input appears ≥2 times, force Abort.

**Input hash**: Computed from normalized tool args before routing. Set on `ErrorMetadata.input_hash`.

**RetryableAfterRefresh**: Always returns to the model with a formatted error message. The LLM template says "Re-read the relevant section before trying again," so the model naturally re-reads and produces a corrected edit. No automatic retry.

### LLM Formatting

Policy: Redacted by default. 18 templates keyed by dotted error code. Selective detail inclusion (paths yes, stack traces no). Remediation hints appended.

---

## Part VI: State Freshness

### File-Version Tracking

`FileContextTracker` now uses `HashMap<String, FileReadState>` instead of a flat `HashSet`:

```
FileReadState {
    content_hash: String,       // hash of content at read time
    read_count: usize,
    last_read_event_id: Uuid,
    edited_since_read: bool,    // invalidated after edit/write
}
```

- `mark_read(path, content_hash)` — updates state, clears staleness flag
- `mark_edited(path)` — sets `edited_since_read = true`
- `get_summary()` — produces validity-aware output:
  ```
  File context:
  - src/foo.rs: read, then edited; previous read may be stale
  - src/bar.rs: read, unchanged
  Files edited: src/baz.rs
  ```

### Tool Cache Invalidation

After any `write` or `edit` tool:
1. `invalidate_for_path(path)` — removes cached read/symbol results for that path
2. `invalidate_search_and_repo()` — removes all cached search and repo results

This ensures subsequent reads of edited files return fresh content.

---

## Part VII: Tool Result Compaction

### Artifact Store

When a tool result exceeds 500 tokens, it is compacted:

1. Full output stored in `ArtifactStore` with a unique ID
2. Trajectory receives a compact digest instead of full output

**Digest format**:
```
Tool result: bash (2340 tokens, compacted)
Status: FAIL 3 tests failed
Important lines:
- expected 200, received 401
- mock token expired
Full output: artifact://tool/bash/1
```

**Artifacts are cleared on compaction** to prevent unbounded growth.

---

## Part VIII: Frame Diagnostics

Every context frame carries diagnostics:

```
FrameDiagnostics {
    stable_prefix_hash: String,
    session_info_hash: String,
    dynamic_suffix_hash: String,
    tools_hash: String,
    messages_hash: String,
    first_changed_region: Option<String>,
    estimated_tokens_by_region: HashMap<String, usize>,
}
```

The compiler tracks the previous frame's hashes and detects `first_changed_region`:

```
Turn 1: first_changed = None (no previous frame)
Turn 2: first_changed = "dynamic" (stable+session unchanged)
Turn N: first_changed = "messages" (only messages changed)
```

Logged each turn:
```
[di-core] frame: stable=a1b2c3d4 session=e5f6g7h8 dynamic=i9j0k1l2 msgs=m3n4o5p6 first_changed=Some("dynamic") tokens={"stable":1000,"session":125,"tools":2500,"dynamic":300,"messages":18000}
```

This provides internal evidence that prefix stability is maintained across turns.

---

## Part IX: Pluggable Token Estimation

```rust
trait TokenEstimator: Send + Sync {
    fn count_text(&self, text: &str) -> usize;
    fn count_messages(&self, messages: &[Message]) -> usize;
    fn count_tools(&self, tools: &[Value]) -> usize;
}
```

| Implementation | Chars/Token | Use Case |
|---------------|-------------|----------|
| `HeuristicEstimator` | 4.0 | Default, conservative |
| `CalibratedEstimator::gpt4()` | 3.5 | GPT-4 models |
| `CalibratedEstimator::claude()` | 3.8 | Claude models |
| `CalibratedEstimator::gemini()` | 4.0 | Gemini models |

---

## Part X: Background Commands

Long-running `bash` commands are spawned in background. Tracked by `BackgroundCommandTracker` with Running/Completed/Failed/TimedOut/Cancelled states. Up to 8 concurrent. Results retrieved via `bash --await <id>`. Running commands included in dynamic suffix.

---

## Part XI: Complete Request Assembly Flow

```
1. First turn only:
   - Gather environment → SessionContext
   - Compile STABLE_PREFIX + SESSION_INFO → cached_session_prefix

2. Every turn:
   - Auto-compaction check
   - API extraction
   - Gather dynamic state (file context with validity, observations, background)
   - Region-aware history selection within token budget
   - Filter system-role messages
   - Compile frame:
     system = cached_session_prefix + dynamic_suffix
     tools  = static (13 definitions)
     messages = priority-selected history
     diagnostics = region hashes + first_changed + tokens_by_region

3. Send GatewayRequest { system, tools, messages, stream: true, provider }

4. Stream response → accumulate text + tool calls

5. For each tool:
   - Mode gate (Plan → read-only only)
   - Approval gate (if needed)
   - Execute through coordinator:
     a. Cache check (read-only tools)
     b. Execute → ToolResponse
     c. On Failure: compute input hash → route through ErrorRouter
     d. On Success: auto-correct truncated output → cache store
   - File context tracking (content hash on reads, staleness on edits)
   - Cache invalidation (after write/edit)
   - Artifact compaction (if result > 500 tokens)
   - Add to trajectory
```

---

## Part XII: Cache Efficiency

| Turn | System String | Cacheable Prefix |
|------|--------------|-----------------|
| 1 | `STABLE + SESSION + DYNAMIC_V1 + MSGS_V1` | None (first request) |
| 2 | `STABLE + SESSION + DYNAMIC_V2 + MSGS_V2` | `STABLE + SESSION` (~1,125 tokens) |
| N | `STABLE + SESSION + DYNAMIC_VN + MSGS_VN` | `STABLE + SESSION` (byte-identical) |

Frame diagnostics confirm prefix stability by logging `first_changed_region = "dynamic"` on subsequent turns.

---

## Part XIII: Token Budget

| Region | Typical Size | Notes |
|--------|-------------|-------|
| Stable prefix | ~1,000 tokens | Compile-time constant |
| Session info | ~125 tokens | Computed once |
| Tool definitions | ~2,500 tokens | 13 tools, static |
| Dynamic suffix | 0–500 tokens | File context, observations, background |
| Conversation history | Variable | Budget-controlled, priority-bucket selection |
| **Total input** | ~3,625 + history | |

---

## Separation of Concerns

| Component | Responsibility |
|-----------|---------------|
| `prompt/stable` | Compile-time prompt template |
| `prompt/session` | Session-level context (env, config) |
| `prompt/compiler` | Three-layer compilation, caching, frame building, diagnostics |
| `context/mod.rs` | Priority-bucket history selection, memory vault, auto-compaction |
| `context/token_estimator` | Pluggable token estimation trait |
| `agent/trajectory` | Append-only event log |
| `agent/engine` | Orchestration: compile context, stream, execute tools |
| `agent/file_context` | File-version tracking with staleness detection |
| `agent/artifact` | Tool result compaction store |
| `tools/mod.rs` | ToolExecutor dispatch, ToolCoordinator (cache + retry + auto-correct) |
| `tools/response` | ToolResponse, ToolError, error codes, recoverability |
| `tools/routing` | ErrorRouter with same-input guard (tool + code + input_hash) |
| `tools/format` | LLM error templates, selective formatting |
| `tools/tool_defs` | Static tool schemas |
| `tools/background` | Background command tracking |
| API Gateway | Provider-specific system/tools/messages transformation |

---

## Implementation Status

### Complete

- Three-layer system string with session prefix caching
- 13 tool schemas in Anthropic-canonical format
- Structured tool response pipeline (ToolResponse, ToolError, ErrorRouter, LLM formatting)
- File-version metadata with content hashes and staleness detection
- Tool cache invalidation after file mutations
- Same-input guard with input hash (tool + error_code + input_hash)
- RetryableAfterRefresh returns to model instead of auto-retrying
- Region-aware history selection (priority buckets: Critical/Important/Normal/Expendable)
- Tool result compaction with artifact store
- Frame diagnostics with region hashes and first-changed-region tracking
- Pluggable token estimation (HeuristicEstimator, CalibratedEstimator)
- Background command tracking

### Next

- Migrate individual tool handlers to return ToolResponse directly
- Remove `recovery.rs` (subsumed by ErrorRouter)
- Read tool integration for artifact references
- Observability integration (error event logging, metrics)
