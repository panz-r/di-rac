# Session Persistence & Architecture — Pi & Cross-System

**Condensed — how hooks interact with session state, compaction, and branching.**

## Pi's JSONL Persistence Model

Each session is a single `.jsonl` file with tree-structured entries:

| Entry Type | LLM-Visible? | Purpose |
|------------|-------------|---------|
| `message` | Yes | Conversation turns |
| `compaction` | No (replaced by summary) | Context window management |
| `branch_summary` | Yes | Summary of abandoned branch |
| `custom_message` | Yes | Extension context for LLM |
| `custom` | **No** | Extension state, ignored by LLM |
| `label` | No | User bookmarks |

## Critical Distinction: Two Extension State Types

```typescript
// LLM-VISIBLE: Injected into context, subject to compaction
pi.sendMessage({ customType: "my_data", content: "visible to LLM", display: "auto", details: {} });

// LLM-INVISIBLE: Ignored by LLM, persists across compaction
pi.appendEntry("my_state", { counter: 42, lastAction: "read" });
```

Hooks must choose based on whether the model should see the data.

## Tree Branching & Context Reconstruction

```
Root → A → B → C (current branch, leafId=C)
          ↘ D → E (forked branch)
```

`buildSessionContext()` traverses leafId → root, collecting only the linear path. When navigating to D, C's messages are excluded from context. `branch_summary` entries summarize abandoned branches.

**Hook implication**: State stored as `custom_message` on one branch won't be visible on the other. Use `custom` entries (LLM-invisible) for cross-branch state.

## Compaction Architecture

### Trigger

- `(usageTokens / maxContextTokens) >= autoCompactThreshold` (default 0.80)
- OR context overflow error from provider

### Process

1. **Cut point selection**: `findCutPoint()` scans backward, keeps `keepRecentTokens` (default 20000)
2. **LLM summarization**: Summarization prompt summarizes messages before cut
3. **Compaction entry**: Appended to session with `summary` + `firstKeptEntryId`
4. **Only latest matters**: Previous compaction summaries hidden from context

### Hook Points

```typescript
pi.on("session_before_compact", async (event) => {
    event.preparation.messagesToSummarize  // Messages to summarize
    event.preparation.turnPrefixMessages     // Prefix context messages
    event.preparation.tokensBefore           // Token count
    event.preparation.firstKeptEntryId       // First entry to keep
    event.preparation.previousSummary        // Summary from last compaction
    // Can return { cancel: true } or { compaction: customResult }
});

pi.on("session_compact", async (event) => {
    event.compactionEntry  // The compaction that was created
});
```

## Cross-System Comparison

| Feature | Pi | OpenClaw | Haystack |
|---------|----|----------|----------|
| **Storage** | JSONL per session | Session rows + memory files | Pipeline snapshots |
| **Branching** | Tree structure | Session keys | N/A |
| **Compaction** | LLM summarization | LLM summarization | N/A |
| **Pruning** | Not built-in | Cache-TTL tool result pruning | N/A |
| **State visibility** | `custom` vs `custom_message` | Session extensions vs next-turn injection | Full pipeline snapshots |
| **Error recovery** | N/A | Snapshot on failure (Haystack) | Auto snapshot + resume |

## 5 Key Lessons for DSL Design

1. **Two-tier extension state** — provide LLM-visible and LLM-invisible state. Each has different compaction semantics.
2. **Branching changes context** — when users branch, old branch messages disappear. Hook state must be branch-aware.
3. **Compaction is not data loss** — full history stays on disk. Only the LLM's view is affected.
4. **Compaction-aware aggregation** — use running aggregates in `custom` entries (not per-message metadata) to survive compaction.
5. **Write locking** — Pi uses process-aware file-based write locks with timeout to prevent concurrent session corruption.
