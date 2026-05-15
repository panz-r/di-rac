# Commit Message Style Guide

## Format

```
type(scope): short description (max ~72 chars, lowercase after colon)

- Bullet list of what changed, each starting with a verb
- No redundant explanations

dir/subdir/
 file.rs:
 - what changed
 - each change on its own line

 file2.rs:
 - what changed
```

## Type and Scope

- **Type**: `fix`, `feat`, `perf`, `refactor`, `docs`, `test`, `chore`
- **Scope**: affected subsystem (`core`, `divrr`, `gateway`, `minimax`, `daemon`, etc.) or `*` for cross-cutting

## Subject Line

- Max ~72 characters
- Imperative mood ("revert emit_event" not "reverted emit_event")
- Lowercase after the colon
- No trailing period
- Single scope if focused, comma-separated if multiple (`fix(core,divrr):`)

## Body

- Blank line after subject
- Bullet list of changes, each a sentence fragment starting with a verb
- No redundant extra explanations or "why" paragraphs
- Technical, precise language

## File Sections

- Directory on its own line, trailing slash
- File name on next line, colon suffix
- Changes indented under the file, each on its own line
- One file per section, no grouping

## Examples

```
fix(treesitter-daemon): various fixes

- Check ts_query_cursor_new() return in all 3 query paths
- db_invalidate_file deletes symbols first (BEGIN/COMMIT wrapped)
- db_index_file and db_index_observation rollback on COMMIT failure
- handle_search_symbols uses memory buffer for slow fallback scan
- Move content buffer from stack to heap (1MB)

treesitter-daemon/src/
 analyzer.c:
 - ts_query_cursor_new NULL checks in 3 functions
 - ApiDependencies leak fix

 db.c:
 - invalidate cascade delete
 - COMMIT rollback in index_file + index_observation

 main.c:
 - heap content buffer
 - search uses memstream for DB-less path
 - sizeof fix
```

```
feat(di-core,command-daemon): track agent CWD through bash protocol

- Add agent_cwd to AgentEngine, inject _cwd into every tool call
- Send cwd in bash execute requests, parse in command-daemon
- repo --detail files resolves paths relative to agent_cwd

di-core/src/agent/
 engine.rs:
 - add agent_cwd, inject _cwd into tool args, update after bash

di-core/src/tools/
 mod.rs:
 - bash sends cwd in request, format_bash_result returns _cwd + _output_str

 list_files.rs:
 - use _cwd for path resolution, skip outside-CWD files

command-daemon/src/
 protocol.c:
 - parse cwd field from execute requests
```
