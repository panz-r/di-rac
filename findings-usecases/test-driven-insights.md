# Test-Driven Insights — Condensed

**Verified event ordering, compaction edge cases, and hook implications from Pi's actual test files.**

## Session Lifecycle Ordering (Verified)

```
Startup: session_start({ reason: "startup" })
New:     session_before_switch → session_shutdown → [invalidate] → [rebind] → session_start({ reason: "new", previousSessionFile })
Resume:  session_before_switch → session_shutdown → [invalidate] → [rebind] → session_start({ reason: "resume", previousSessionFile })
Fork:    session_before_fork → session_shutdown → session_start({ reason: "fork", previousSessionFile })
```

Cancellation verified: `return { cancel: true }` aborts the operation, session unchanged.

## Compaction Edge Cases (Verified)

| Behavior | Detail |
|----------|--------|
| **Only latest matters** | Multiple compactions — only the most recent summary appears in context |
| **Re-compaction** | Previously-kept messages can be re-summarized if `keepRecentTokens` shrinks |
| **Aborted messages skipped** | Messages with `stopReason: "aborted"` don't count in usage |
| **Cut point** | `findCutPoint()` → `{ firstKeptEntryIndex, isSplitTurn }` — cuts at message boundary |
| **Prep return** | `prepareCompaction()` → `{ messagesToSummarize, tokensBefore, firstKeptEntryId, previousSummary }` |
| **Token calc** | `calculateContextTokens()` = input + output + cacheRead + cacheWrite |

## Context Staleness (Verified)

After any session transition, old extension context **throws**:
```
"This extension ctx is stale after session replacement or reload..."
```
The exact message is verified in tests.

## Hook Implications

1. **Event ordering is contract** — Document exact lifecycle ordering. Extensions depend on it.
2. **Cancellation semantics** — `session_before_*` with `{ cancel: true }` is clean veto pattern.
3. **Compaction is not data loss** — Full history on disk. Only LLM's view affected. Use `appendEntry("custom")` for state that must survive compaction.
4. **Session identity is file-based** — `previousSessionFile` links sessions. Fork creates new files.
5. **Enforce context staleness** — Don't just deprecate old contexts. **Throw** on access. Silent stale > loud error.
6. **Two-tier state** — `custom_message` (LLM-visible, subject to compaction) vs `custom` (LLM-invisible, survives).
