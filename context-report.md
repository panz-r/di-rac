# Context Compilation and Management System

This report covers the conceptual design, algorithms, and behavioral guarantees of the context compilation system in di-core. It is intended as a complete reference for understanding how context is assembled, budgeted, compacted, and surfaced to the gateway across the agent's lifetime.

---

## 1. Architecture Overview

The context system is organized into six cooperating subsystems:

```
┌─────────────────────────────────────────────────────────────────┐
│                     Agent Turn Lifecycle                        │
│                                                                 │
│  ┌──────────┐   ┌──────────────┐   ┌────────────────────────┐  │
│  │  Context  │──▶│    Context    │──▶│   ContextCompiler     │  │
│  │  Manager  │   │   Compiler   │   │   (build_frame)        │  │
│  │ (history) │   │ (system str) │   │   → ContextFrame       │  │
│  └──────────┘   └──────────────┘   └────────────────────────┘  │
│        ▲                ▲                    ▲                   │
│        │                │                    │                   │
│  ┌─────┴──────┐  ┌──────┴──────┐  ┌─────────┴───────────┐     │
│  │ Trajectory │  │ Dynamic     │  │ ArtifactStore +      │     │
│  │ + Messages │  │ Context     │  │ Distiller Pipeline   │     │
│  └────────────┘  └─────────────┘  └──────────────────────┘     │
│                         ▲                                        │
│                   ┌─────┴──────┐                                 │
│                   │ Task State │  File Context  MemoryVault     │
│                   │  Reducer   │  Tracker                       │
│                   └────────────┘                                 │
└─────────────────────────────────────────────────────────────────┘
```

Each subsystem has a single responsibility and communicates through well-defined data flows. No subsystem reaches into another's internals.

---

## 2. The ContextCompiler — Four-Layer System Prompt

The system prompt sent to the gateway is assembled from four layers, each with independent caching semantics:

```
full_system = STABLE_PREFIX + SESSION_STATIC + SESSION_POLICY + DYNAMIC_SUFFIX
              compile-time    once/session    mutable/session  every turn
```

### 2.1 Stable Prefix (compile-time)

A static text template containing the agent identity, prime directives, code exploration heuristics (the "Cost Ladder"), tool use conventions, response format specification, error recovery rules, context management guidance, security rules, and mode descriptions. This string is identical across all turns and all sessions — it only changes between deployments when the prompt template is updated. It is computed once via `LazyLock` and never re-allocated.

### 2.2 Session Static (once per session)

Machine-resolved facts that do not change during a session: OS, shell, CWD, available CPU cores, workspace root, and yolo-mode flag. Concatenated with the stable prefix at compiler initialization to form a cached string (`cached_static_prefix`) that is never recomputed.

### 2.3 Session Policy (mutable per session, recomputed each frame)

Mutable session-level configuration: agent mode (Act/Plan), skills definitions, and user custom instructions. This is recomputed from the live session state on every frame by calling `build_policy_info()`. This allows mid-session mode switches (e.g., from Plan to Act) to take effect immediately without reinitializing the compiler.

When the mode is Plan, the policy injects a constraint restricting the agent to read-only tools only.

### 2.4 Dynamic Suffix (every turn)

Per-turn context that varies with agent activity. Built from six sources:

1. **Task State Summary** — The `TaskStateReducer`'s deterministic summary of the current goal, constraints, and supersession history.
2. **File Context** — The `FileContextTracker`'s validity-aware summary of which files have been read, edited, or observed via metadata tools.
3. **Relevant Observations** — From the `MemoryVault`, filtered by API overlap with the current conversation and a minimum confidence threshold of 0.5.
4. **Background Command Summary** — Status of any background bash commands running concurrently.
5. **Distilled Context** — Optional model-enriched summary from the distiller pipeline.
6. **Compaction Continuation** — After compaction, a continuation prompt is stored and surfaced here.

Each source is only included if it produces non-empty content. The parts are joined with double-newline separators.

---

## 3. The ContextCompiler — Frame Diagnostics

Every `build_frame()` call computes per-region diagnostics:

### 3.1 Region Hashing

Each region (stable, session_static, session_policy, dynamic, tools, messages) is hashed independently using blake3. The hash for each region is compared against the previous frame's hash to detect the first changed region, logged as `first_changed_region`.

This enables cache-efficiency observability: if `first_changed_region = "dynamic"` on most frames, the stable/session regions are cache-stable as intended. If it fires on "stable" or "session_static", something unexpected has changed.

### 3.2 Token Estimation by Region

Each region's character count is divided by 4 to produce a rough token estimate, stored in `estimated_tokens_by_region`. This is used by `last_frame_system_overhead()` to compute an accurate system prompt budget for history selection.

### 3.3 System Overhead Calculation

`last_frame_system_overhead()` sums the previous frame's measured region estimates (stable + session_static + session_policy + dynamic + tools), then adds three constants:

- **Output Reserve (4096)** — guaranteed space for the model's response
- **Safety Margin (256)** — buffer for tokenizer discrepancy
- **Protocol Overhead (128)** — JSON framing, role labels, etc.

On the first frame (before any diagnostics exist), it returns a conservative default of 8000 tokens. Subsequent frames use measured values, which can be substantially different from the default — a large tool definitions list or verbose dynamic suffix will naturally increase the overhead estimate.

---

## 4. History Selection — Priority-Bucket Selection with Stale-Read Detection

The `ContextManager.build_prompt_with_stale_check()` method assembles the message history sent to the gateway. It uses a priority-bucket algorithm with a dynamic token budget.

### 4.1 Budget Computation

```
available_budget = token_limit - system_overhead
```

Where `system_overhead` comes from `ContextCompiler.last_frame_system_overhead()`, which uses the previous frame's actual measurements rather than a hardcoded formula.

### 4.2 Message Classification and Priority Assignment

Every message in the trajectory is assigned a priority level (0–4):

| Priority | Level Name | What gets this priority |
|----------|-----------|------------------------|
| 4 | Critical | Initial task, user corrections, user constraints, user goal changes, latest assistant intent |
| 3 | Important | Latest 3 tool results (from the end of the conversation) |
| 2 | Normal | Recent turns (last 5 assistant-user pairs) |
| 1 | Low | (reserved) |
| 0 | Expendable | Everything else |

User messages are classified by the `TaskStateReducer` into seven semantic kinds:

- **InitialTask** — the first user message (priority 3)
- **Correction** — user messages containing correction signals (priority 4)
- **Constraint** — user messages adding constraints (priority 4)
- **GoalChange** — user messages changing the goal (priority 4)
- **Clarification** — substantive but non-directive user messages (priority 2)
- **Approval** — brief approval/acknowledgment (priority 1)
- **Casual** — short non-substantive messages (priority 0)

### 4.3 Priority-Bucket Fill Algorithm

Messages are filled into the budget from highest to lowest priority:

```
for level in 4..0:
    for each message with priority == level:
        if remaining_budget >= message.tokens:
            include message
            deduct tokens
```

This guarantees that critical context (the task, corrections, latest intent) is always included first, and expendable context (old prose, large tool outputs) is dropped first.

### 4.4 Initial Task Guarantee

After the priority fill, the algorithm enforces a hard guarantee: the initial task message (first user message) is always included. If it was dropped due to budget constraints:

1. **Normal case**: Force-include it and evict the lowest-priority non-first messages until the budget is satisfied.
2. **Oversized case**: If the initial task alone exceeds the budget, include a truncated excerpt (capped to budget*4 characters) with a notice: `[Initial task compacted: N tokens originally. Task state available in system prompt.]`

This guarantee works because the dynamic suffix already contains the `TaskStateReducer`'s summary of the original goal and constraints — so even a truncated initial task still has full context available to the model.

### 4.5 Stale-Read Detection

Tool results from the `read` tool carry structured metadata (`ToolMessageMeta.paths_read`) recording which file paths were read. Before inclusion in the prompt, each read result is checked against the set of files that have been subsequently edited. If a file was edited after it was read, the read result is replaced with:

```
[stale file read omitted: <path> was edited after this read]
```

This is only applied to `read` tool results. Other tools (search, repo, symbols, bash) don't provide complete file content, so staleness is less critical and false positives are common.

The stale-read replacement counts as 20 tokens, freeing up budget for other messages.

---

## 5. File Context Tracking

The `FileContextTracker` maintains per-file state across the agent's lifetime, with three distinct observation types:

### 5.1 Content Reads (`mark_read`)

When the `read` tool executes successfully, the file path and a content hash are recorded. This enables stale-read detection: if the file is subsequently edited, the read is flagged as stale. Re-reading the same file updates the hash and clears the stale flag.

### 5.2 Edits (`mark_edited`)

When `write` or `edit` tools execute, the file is added to the edited set. Any prior content reads of that file are marked as stale.

### 5.3 Metadata Observations (`mark_metadata_observed`)

When `search`, `repo`, or `symbols` tools reference a file, it is recorded as a metadata observation. These do NOT enable stale-read detection — they carry no content hash and are not affected by subsequent edits. This distinction avoids false-positive staleness for partial information (search snippets, directory listings, symbol names).

### 5.4 Summary Output

The tracker produces a structured summary for the dynamic suffix with three sections:

1. **Read files** — path, read count, stale status
2. **Edited files** — files modified during the session
3. **Referenced via search/symbols** — files observed through metadata tools

---

## 6. Artifact Store — Tool Result Compaction

The `ArtifactStore` manages out-of-band storage for large tool results. It provides deterministic compaction with per-tool digest strategies.

### 6.1 Compaction Thresholds

Each tool type has a token threshold below which compaction is skipped:

| Tool | Threshold (tokens) |
|------|-------------------|
| bash | 500 |
| read | 1500 |
| search | 800 |
| repo | 1000 |
| symbols | 1000 |
| default | 500 |

### 6.2 Compaction Process

When a tool result exceeds its threshold:

1. The full output is stored in the artifact store with a unique ID (`tool/<name>/<counter>`).
2. A tool-specific digest is generated containing: status line, important lines (up to 8), and an `artifact://<id>` reference.
3. The digest replaces the full output in the trajectory, dramatically reducing token usage.

### 6.3 Per-Tool Digest Strategies

- **bash**: Extracts exit code, stderr error lines, stdout error/failure lines, and last 4 non-empty stdout lines.
- **read**: Extracts file path, range, and signature lines (function/struct/impl/class/def declarations).
- **search**: Extracts pattern, file count, and up to 8 match locations (file:line:text).
- **symbols**: Extracts subcommand, symbol count, and up to 8 symbol entries (name [kind] at location).
- **repo**: Extracts directory path, entry count, source directories, and file listing.
- **generic**: Extracts first non-empty line as status and last 8 non-empty lines.

### 6.4 Artifact Garbage Collection

Artifacts are garbage-collected via mark-and-sweep after compaction. The GC root set is built from three sources:

1. **Checkpoint artifact refs** — The most important source. When the trajectory is compacted, all artifact references from the messages being discarded are collected and stored in the `Checkpoint.artifact_refs` field. This field is the primary GC root — it ensures artifacts referenced in compacted context survive across compaction cycles.
2. **Structured metadata** — Recent messages' `tool_meta.artifact_ref` fields.
3. **Regex fallback** — `artifact://` references extracted from recent message content.

Artifacts with no live references and a ref_count of 1 are collected.

### 6.5 Persistence

Artifacts can be persisted to a directory of JSON files and loaded back. Each file is named by its artifact ID (with `/` replaced by `_`). The counter is restored from the highest ID number found during loading.

---

## 7. Distiller Pipeline

The distiller provides optional model-enriched summarization for tool results, task state, and checkpoints. It operates as a two-tier pipeline with a deterministic fallback.

### 7.1 Trait Interface

The `ContextDistiller` trait defines three operations:

1. **`distill_tool_result`** — Summarize a large tool result into: summary, key_facts, errors, files_referenced, estimated_tokens, and an optional artifact_ref.
2. **`consolidate_task_state`** — Merge recent assistant summaries and file context into: enriched_summary, open_subgoals, decisions, critical_files.
3. **`generate_checkpoint`** — Create a checkpoint from continuation text: progress_summary, completed, remaining, risks, modified_files, artifact_refs.

Every operation returns a `Provenance` struct recording source_event_ids, confidence, source (Model or DeterministicFallback), and config_version.

### 7.2 Model-Backed Distiller

When a distiller config (provider credentials + model) is provided, the system creates a `ModelContextDistiller` that uses a separate gateway client to call an LLM for summarization. This produces higher-quality summaries with confidence > 0.5.

### 7.3 Deterministic Fallback (NoopContextDistiller)

When no distiller config is provided, or when the model-backed distiller fails (timeout, invalid JSON, schema mismatch, provider error), the system falls back to the `NoopContextDistiller`. This produces bounded deterministic output:

- Tool result summaries are truncated to 2000 characters
- Key facts are the last 5 non-empty lines
- Errors are lines containing "error", "failed", or "fatal" (up to 3)
- File paths are extracted via regex (up to 10, deduplicated)
- Confidence is always 0.1 (below the model threshold)
- `fallback_occurred` is always true

### 7.4 Distiller Integration in Tool Execution

After deterministic compaction, if the tool result is >= 400 tokens and a distiller is configured, the distiller enriches the result with summary, key_facts, errors, and files_referenced. The enriched output replaces the trajectory content while the original result is emitted to the frontend.

### 7.5 Distiller Integration in Compaction

Both runtime compaction and model-initiated compaction use the distiller's `consolidate_task_state` to produce an enriched summary. If the distiller returns Model-sourced output, subgoals, decisions, and critical files are appended. If it falls back to DeterministicFallback, the deterministic summary is used as-is.

---

## 8. Compaction

Compaction truncates the trajectory when it approaches the context limit, preserving essential state through the dynamic suffix and checkpoint.

### 8.1 When Compaction Occurs

Compaction is triggered when `trajectory.get_total_tokens()` exceeds `compression_threshold` (default: 24000 tokens, context limit default: 32000). The check happens at the start of each turn, before the context frame is built.

### 8.2 Runtime Compaction (Deterministic)

When the model has not explicitly requested compaction, the system performs runtime compaction:

1. **Task State Summary** — From the `TaskStateReducer`'s current state.
2. **File Context Summary** — From the `FileContextTracker`.
3. **Recent Assistant Progress** — Last 5 assistant messages, each truncated to 300 characters.
4. **Background Command Summary** — Active background commands.
5. **Optional Distiller Enrichment** — If configured, the distiller produces an enriched summary with subgoals, decisions, and critical files.

These are assembled into a continuation prompt: *"This session is being continued from a previous conversation..."* plus the summary and background.

### 8.3 Model-Initiated Compaction

When the model calls the `compact` tool, its summary is recorded as advisory. On the next turn, if compaction thresholds are met, the model's summary is used instead of the deterministic one. The model's summary still passes through optional distiller enrichment.

### 8.4 Compaction Execution

Both paths converge on the same execution:

1. **Collect artifact references** — All `artifact_ref` fields and `artifact://` references from messages being discarded are collected into `checkpoint.artifact_refs`.
2. **Build checkpoint** — A `Checkpoint` struct with progress_summary, artifact_refs (and empty completed/remaining/risks/modified_files).
3. **Truncate trajectory** — `trajectory.truncate_with_continuation()` clears all messages and stores the checkpoint. No System-role message is pushed (the gateway filters System messages anyway).
4. **Garbage collect artifacts** — Mark-and-sweep using checkpoint refs + recent structured metadata + regex fallback.
5. **Gateway sentinel** — If the resulting message list is empty, a single user message is injected: `[Session continued from previous context. See task state in system prompt.]`

### 8.5 Post-Compaction Context

After compaction, the model receives:

- **System prompt**: Stable prefix + session static + session policy + dynamic suffix (which now contains the TaskState summary, file context, and continuation prompt)
- **Messages**: Either empty (with the sentinel) or the few messages that were added after compaction triggered

The checkpoint's `artifact_refs` field ensures that any artifact referenced in the compacted-away portion of the conversation survives in the artifact store and can be retrieved by `artifact://` references in the dynamic suffix.

---

## 9. Task State Tracking

The `TaskStateReducer` classifies user messages and maintains a structured model of the task's goal evolution.

### 9.1 Classification

User messages are classified by priority-ordered keyword matching:

| Kind | Priority | Detection |
|------|----------|-----------|
| Correction | 4 | "no not that", "wrong", "don't do", "fix", "incorrect", etc. |
| GoalChange | 4 | "instead", "actually", "new goal", "change the", "rather", etc. |
| Constraint | 4 | "must", "always", "never", "ensure", "required", "constraint", etc. |
| InitialTask | 3 | First user message (by position) |
| Clarification | 2 | Longer messages that don't match other patterns |
| Approval | 1 | "yes", "ok", "go ahead", "sure", "approved", etc. |
| Casual | 0 | Short non-substantive messages |

### 9.2 State Mutation

On classification, the reducer updates:

- **original_goal** — Set once from the InitialTask, never overwritten.
- **current_goal** — Updated on GoalChange or Correction.
- **active_constraints** — Appended on Constraint.
- **superseded_by** — Records when a goal is superseded, with source message ID, kind, timestamp, and truncated summary.

### 9.3 Summary Output

`to_critical_summary()` produces a deterministic text block for the dynamic suffix:

```
Current goal: <current_goal>
Constraints: <constraint1>, <constraint2>
Original goal: <original_goal> (superseded)
```

This summary is always included in the dynamic suffix, ensuring the model has access to the full task context even after compaction.

---

## 10. Error Routing

The `ErrorRouter` produces routing decisions from structured tool errors, with a same-input guard to prevent infinite retry loops.

### 10.1 Recoverability Classification

Every tool error carries a `Recoverability` field:

| Recoverability | Meaning | Default Route |
|---------------|---------|---------------|
| NonRetryable | Permanent error (syntax, not found) | Continue (report to model) |
| RequiresUserInput | Need human decision | Escalate to frontend |
| RequiresReplan | Plan is stale | Abort turn |
| Retryable | Transient (timeout, rate limit) | Retry with exponential backoff |
| RetryableAfterRefresh | Context may be stale | Continue (model re-reads) |

### 10.2 Retry Limits and Backoff

Per error code:

| Error Code | Max Retries | Base Backoff |
|-----------|-------------|-------------|
| DaemonUnavailable | 3 | 1000ms |
| DaemonTimeout | 2 | 500ms |
| ShellTimeout | 2 | 500ms |
| RateLimited | 3 | 2000ms |
| Default | 1 | 500ms |

Backoff is exponential: `base * 2^attempt`, capped at 4000ms.

### 10.3 Same-Input Guard

The router tracks retry counts per `(tool_name, error_code, input_hash)`. When the same error with the same input is seen >= 2 times, the router routes by recoverability instead of retrying:

- **Retryable** → Continue (report to model for replanning)
- **RequiresUserInput** → Escalate
- **All others** → Abort

This prevents infinite retry loops while still giving the model a chance to replan on retryable errors.

---

## 11. Secret Redaction

Secrets are redacted at every storage boundary to prevent API keys, tokens, and credentials from persisting in trajectory, artifacts, or distiller output.

### 11.1 Redaction Points

1. **Assistant text** — Before adding to trajectory.
2. **Model compact summary** — Before storing as advisory.
3. **Tool results** — Before adding to trajectory (both compacted and raw).
4. **Error messages** — Before adding to trajectory.
5. **Artifact storage** — Full output is redacted before storage in the artifact store.
6. **Distiller output** — The noop fallback truncates output (bounded to 2000 chars), limiting exposure.

Each redaction increments the `redaction_count` metric counter.

---

## 12. Metrics

The `ContextMetrics` struct provides 20 lock-free atomic counters for observability:

**Routing**: retry_count, continue_count, abort_count, escalate_count, same_input_abort_count
**Context**: stale_read_omitted_count, artifact_compaction_count, artifact_retrieval_count
**Cache**: hit_count, miss_count
**Compaction**: compaction_count
**Distiller**: model_call_count, fallback_count, timeout_count, invalid_json_count, schema_mismatch_count, validation_failed_count, provider_error_count
**Security**: redaction_count

Note: Each distiller error counter also increments `fallback_count`, so the fallback count reflects total fallback events regardless of cause.

A `snapshot()` method produces a JSON summary with all counters and a computed `cache_hit_rate` percentage.

---

## 13. Approval System

The `ApprovalManager` enforces a pre-execution approval gate for destructive tools.

### 13.1 Tool Classification

- **Read-only tools** (read, search, repo, symbols, ask, done, plan, compact, tools) — Auto-approved.
- **Destructive tools** (write, edit, bash) — Require frontend approval before execution.

### 13.2 Approval Flow

1. Before executing a destructive tool, the engine emits an `ApprovalNeeded` event.
2. It blocks waiting for a response from the frontend channel.
3. If approved, execution proceeds. If denied or timed out, a denial message is recorded in the trajectory.

---

## 14. Turn Lifecycle

Each turn of the agent loop follows this sequence:

1. **Init context compiler** (once) — Build stable prefix + session static cache.
2. **API extraction** — Extract API calls from the latest assistant message for observation filtering.
3. **Auto-compaction check** — If trajectory exceeds threshold, perform runtime or model-initiated compaction.
4. **Build dynamic context** — Assemble TaskState summary, file context, relevant observations, background summary, and distilled context.
5. **Compute system overhead** — From previous frame's measured region estimates.
6. **Build message history** — Priority-bucket selection with stale-read exclusion.
7. **Observer metrics** — Compute SQS score and token usage.
8. **Build gateway messages** — Filter to user/assistant/tool only, inject sentinel if empty post-compaction.
9. **Compile context frame** — Assemble system prompt (four layers) + tools + messages + diagnostics.
10. **Stream gateway request** — Send to the LLM provider with streaming.
11. **Record assistant response** — Redact secrets, add to trajectory.
12. **Execute tools** — For each tool:
    a. Mode gate (Plan mode restricts tools)
    b. Approval gate (destructive tools need approval)
    c. Execute via tool executor
    d. Track file context (read → mark_read, search/repo/symbols → mark_metadata_observed, write/edit → mark_edited)
    e. Deterministic compaction (large results → artifact + digest)
    f. Optional distiller enrichment (results >= 400 tokens)
    g. Secret redaction before trajectory storage
    h. Build structured metadata (ToolMessageMeta with paths_read, paths_written, artifact_ref)

---

## 15. Behavioral Guarantees

1. **System prompt never exceeds cache** — The stable + session_static prefix is byte-identical across all turns of a session. Only the policy and dynamic layers change.
2. **Initial task always survives** — The first user message is force-included in history selection. If it exceeds the budget, a truncated excerpt is used with the TaskState summary as backup.
3. **Stale reads are never sent** — Read results for edited files are replaced with stale notices before gateway submission.
4. **Secrets never persist** — Every storage boundary redacts secrets before writing.
5. **Artifacts survive compaction** — The checkpoint's artifact_refs field is the primary GC root, ensuring referenced artifacts survive across compaction cycles.
6. **Compaction never sends empty messages** — If the trajectory is empty post-compaction, a sentinel user message is injected.
7. **Distiller always produces output** — The deterministic fallback guarantees bounded output on model failure.
8. **Same-input loops are broken** — The error router's same-input guard prevents infinite retry with identical inputs.
9. **Destructive tools require approval** — Write, edit, and bash tools are gated behind frontend approval before execution.
10. **Metrics are lock-free** — All counters use atomic operations with Relaxed ordering, ensuring no contention on the hot path.
