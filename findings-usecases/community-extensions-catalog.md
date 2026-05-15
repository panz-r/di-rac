# Community Extensions — Notable Patterns

**Key Pi extensions organized by design pattern relevance.**

## Security (5 patterns)

`tool-override.ts` — override built-in tools (read → add audit logging + path blocking). `permission-gate.ts` + `dirty-repo-guard.ts` — `tool_call` + `session_before_switch` hooks with confirm dialog. `protected-paths.ts` + `safe-git` — block sensitive paths/dangerous git ops.

## State & Workflow (5 patterns)

`handoff.ts` — `newSession()` with `withSession()` callback (stale context prevention). `session-observer.ts` — track workflow signals across turns via `session_start` + `turn_end`. `dynamic-tools.ts` — register tools in `session_start` or via commands at runtime.

## UI (5 patterns)

`my-footer.ts` — `setFooter()` with git branch + context meter. `modal-editor.ts` — `setEditorComponent()` for custom editor. `github-issue-autocomplete.ts` — `addAutocompleteProvider()` stacking. `custom-compaction.ts` — replace default LLM summarization with custom model.

## Subagents (2 key extensions)

`pi-subagents` — parallel/chain/background subagents, worktree isolation, intercom bridge for child↔parent communication. Most popular community extension (parallel execution with concurrency limits, git worktree isolation for parallel edits).
