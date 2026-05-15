use serde_json::{json, Value};
use std::sync::LazyLock;

pub static TOOL_DEFINITIONS: LazyLock<Vec<Value>> = LazyLock::new(|| {
    vec![
        json!({
            "name": "read",
            "description": "Read files with detail levels: hint (kind+name only), preview (auto for large files), outline (defs with handles like fn:Name), skeleton (signatures only), full. Use --section fn:Name to jump to a symbol. Use --detail outline before --detail full for large files.\n\nExample: read src/auth.ts --detail outline\n\nResponse: OK | detail:<level> | handles:N | lines:N | tokens:N. Content follows. Handles like fn:Name work with --section.\nNote: --detail full auto-downgrades to preview for files over 50KB. Repeated reads at same detail are cached.\nFails when: file >50KB auto-downgrades, binary files show minimal content, --section not found (returns warning).\nIf fails: for large files use --detail outline first then --section or --range. For binary use bash file.\nAfter results: if outline, use --section fn:Name to jump to body. If preview, use --range for specific lines.\nGood: symbols visible, content at expected line, hash anchors stable. Bad: auto-downgraded (file too large, use --range), binary (use bash), section not found (use outline).\nDon't use for: searching patterns (use search), code structure across files (use symbols or repo).\nOutput example (outline): OK | detail:outline | handles:3 | lines:42 | tokens:120\n  fn:login (line 42)  fn:logout (line 58)  class:AuthService (line 10)\nTypical: read src/file.ts --detail outline",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "CLI arguments for read. Use --detail, --range, --section, --retry flags." }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "write",
            "description": "Create or overwrite a file atomically. Skips write if content unchanged (idempotent). Auto-creates parent directories.\n\nExample: write src/new.ts --content \"export function foo() { ... }\"\n\nChain: write a.ts --content '...'; write b.ts --content '...'\nResponse: OK | lines:N | path:<path> | tokens:N\nFails when: path is a directory, content missing, disk full.\nIf fails: verify path is a file (not dir), ensure --content is provided, check disk space.\nAfter results: read the file to verify content, or use edit for targeted refinements.\nGood: file created/overwritten with expected content. Bad: directory path (specify a file), missing content (ensure --content is set).\nDon't use for: editing existing files (use edit), reading content (use read).\nOutput example: OK | lines:5 | path:src/new.ts | tokens:20\nUniversal flags: --dry-run (write to temp file, show diff), --retry N.\nTypical: write src/new.ts --content 'export const X = ...'",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "CLI arguments for write. Use --content, --dry-run, --retry flags." }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "edit",
            "description": "Edit files using hash-anchored line references. Anchors verified against current file content before applying — stale anchors trigger fuzzy matching (≥90% auto-resolves, 70-90% suggests, <70% fails with re-read advice). Don't rewrite entire files — use targeted anchors. Don't edit without first reading anchors (e.g. \"a3|def foo():\"). Types: replace (anchor to end-anchor), insert_after, insert_before.\n\nExample: edit src/auth.ts --anchor \"a3|def login():\" --end-anchor \"k7|  pass\" --content \"def login():\\n  ...\"\n\nChain: edit a.ts --anchor 'a3|...' --content '...'; edit b.ts --anchor 'b2|...' --content '...'\nAnchor format: hash|content (e.g. \"a3|def foo():\"). Use read --detail outline or symbols search to get anchors before editing. Response: OK | edits:N | tokens:N. Diffs follow. Verify with read after editing.\nFails when: anchor not found (file changed since last read), end-anchor before start-anchor.\nIf fails: re-read the file to get current anchors, then retry. Use --dry-run to preview changes.\nDon't use for: creating new files (use write), reading content (use read).\nGood: diff shows expected changes applied. Bad: anchor not found (re-read file for current anchors), wrong lines (check anchor placement).\nOutput example: OK | edits:1 | tokens:12  - a3|old line  + new line\nUniversal flags: --dry-run (edit temp file, show diff), --retry N.\nTypical: edit src/file.ts --anchor 'a3|def foo' --content 'new body'",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "CLI arguments for edit. Use --anchor, --end-anchor, --content, --dry-run, --retry flags." }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "search",
            "description": "Search files with regex patterns. Skips .git, node_modules, build/, binaries. Returns first 30 matches with file, line, and context. Use for text patterns, config values, comments.\n\nExample: search --pattern \"TODO|FIXME\" --context 2\n\nResponse: OK | matches:N | files:N | hint:refinements | tokens:N\n\tMatches follow: file:line:context (one per line, max 30).\nNote: path is optional (defaults to cwd). --context 0-5. Results auto-truncated at 30 matches; narrow your pattern or path if partial.\nFails when: 0 matches (pattern too specific or wrong path), 100+ matches (too broad).\nIf fails: broaden pattern with .*, narrow with path, or try symbols search for code structure.\nAfter results: read the specific file:line from matches. If too many, narrow path or pattern.\nGood: 3-30 matches with files in repo, context shows pattern. Bad: 0 matches (broaden pattern), 100+ (narrow with path).\nDon't use for: code navigation (use symbols), full-file content (use read).\nOutput example: OK | matches:3 | files:2 | tokens:45\n  src/auth.ts:42:  // TODO: Refresh token\n  src/auth.ts:156: // FIXME: Add rate limit\n  config/api.env:2: TODO=remove_from_git\nTip: too many matches? Narrow with path or --context 0.\nTypical: search --pattern 'TODO|FIXME'",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "CLI arguments for search. Use --pattern, --path, --context, --file-pattern, --retry flags." }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "repo",
            "description": "Get repository structural overview. --detail: summary (default, top symbols per file), files (all files with line counts), skeleton (all defs). Optional paths filter directories.\n\nExample: repo --detail files src/\n\nResponse: OK | files:N | lines:N | symbols:N | detail:<summary|files|skeleton> | tokens:N\n\tContent follows. Structure varies by detail level.\nFails when: path doesn't exist (returns empty), very large repos (--detail skeleton may be slow).\nIf fails: verify path with repo --detail files, or narrow to a subdirectory.\nAfter results: read --detail outline on specific files to explore, or search for patterns within.\nGood: files listed with expected structure. Bad: empty (wrong path), too many files (narrow with path filter).\nDon't use for: file content (use read), text search (use search), specific definitions (use symbols).\nOutput example (files): OK | files:12 | detail:files | tokens:30\n  src/auth.ts 142\n  src/config.ts 58\n  src/utils/helpers.ts 89\nTip: use path filter to limit scope (e.g. repo --detail files src/auth/). Use --detail summary instead of skeleton for large repos.\nTypical: repo --detail files src/",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "CLI arguments for repo. Use --detail, --path, --retry flags." }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "bash",
            "description": "Execute shell commands. Composition (pipes, &&, ||) is encouraged to minimize round-trips. Use heredocs for multi-line scripts. Dangerous commands (recursive deletes, reverse shells) are blocked; blocked will name the pattern. Don't edit files with bash — use edit. Don't read files — use read.\n\n\t\tExample: bash \"npm test && npm run build\"\n\n\t\tResponse: OK | tokens:N | lines:N | exit:N followed by stdout. [stderr], [truncated], [timed out], [blocked:pattern], [security:violation] appended as applicable.\n\t\tNote: stdout truncated at ~8KB, stderr at ~2KB (head+tail preserved). Use redirects to file for larger output.\n\t\tFails when: timeout (>300s default), exit≠0 (check stderr), output truncated, blocked:pattern.\n\t\tIf fails: --timeout 60 for slow commands; redirect large output to file; blocked shows the pattern.\n\t\tAfter results: check exit code. If non-zero, read stderr. If truncated, redirect to file then read.\n\t\tGood: exit:0 with expected output visible. Bad: exit!=0 (read stderr), truncated (redirect to file), timed_out (use --timeout).\n\t\tOutput example: exit:0\n\t\t  src/auth.ts  42 | a3|def login():\n\t\t  src/auth.ts  58 | k7|  return token\n\t\tUniversal flags: --timeout N (max seconds to wait, default 300s, max 600s), --retry N (retry on error, max 5).\n\t\tTypical: bash 'npm test && npm run build'",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The full shell command to execute. Pipes, &&, ||, heredocs, and subshells all work. Use --await <id> to retrieve a background command's result." }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "compact",
            "description": "Compress conversation history. Summary becomes your only context. --keep: file paths to reload (up to 8). Don't compact mid-edit — finish changes first. For reading saved outputs, use memory.\n\nExample: compact \"Fixed auth bug. Changed middleware to JWT.\" --keep src/auth.ts\n\nResponse: OK | summary:<text> | reloaded:N | tokens:N\n\tSummary follows header line.\nAfter results: context is compressed. Use memory to reload key outputs if needed.\nGood: summary captures key state, --keep files reloaded. Bad: lost important context (use --keep next time), compacted mid-edit (finish edits first).\nTypical: compact 'Summary of work so far' --keep src/file.ts",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "CLI arguments for compact. Provide summary text, use --keep for file paths to reload." }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "ask",
            "description": "Ask user for clarification. --options: comma-separated choices (2-5).\n\nExample: ask \"JWT or session?\" --options JWT,Session,OAuth\n\nResponse: OK | <user_response> | tokens:N\n\tGood: clear answer with one of the options. Bad: ambiguous response (ask again with narrower options).\nTypical: ask 'Which approach?' --options A,B,C",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "CLI arguments for ask. Provide question text, use --options for choices." }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "done",
            "description": "Mark task complete with result summary. --cmd: optional demo command (not echo/cat).\n\nExample: done \"Added caching layer\" --cmd \"npm test\"\n\nResponse: OK | summary:<text> | tokens:N\n\tSummary follows header line.\n\tGood: clear summary of what changed and how to verify. Bad: vague summary (be specific about what was done).\nTypical: done 'Fixed the bug'",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "CLI arguments for done. Provide result summary, use --cmd for demo command." }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "symbols",
            "description": "Perform AST symbol operations: search definitions, replace bodies, rename across files, find references. For text/regex patterns, use search instead. For reading file content, use read --detail outline --section fn:Name.\n\nSubcommands:\n  search --name PATTERN [--kind function|class]    Find definitions\n  replace --name SYMBOL --text CODE                Replace definition body\n  rename --old NAME --new NAME                     Rename across files\n  refs --name SYMBOL                               Find all references\n\nExample: symbols search src/ --name AuthService --kind class\n\nResponse: OK | matches:N | hint:Try --kind function/class or different name | tokens:N\nFails when: no matches (typo, wrong --kind), file type not supported by tree-sitter.\nIf fails: try without --kind, use search for text patterns, check file extension support.\nAfter results: use read --section <handle> to see full body. Use refs to find usages.\nGood: definitions found with types and signatures. Bad: no matches (try without --kind or use search), unsupported file type (check extension).\nDon't use for: text/regex patterns across files (use search), file overview (use repo).\nOutput example: OK | matches:2 | tokens:35\n  src/auth.ts:10 class AuthService (fn:login, fn:logout)\n  src/auth.ts:42 fn login()\nUniversal flags: --dry-run (preview changes without applying), --retry N.\nTypical: symbols search src/ --name AuthService --kind class",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "CLI arguments for symbols. Subcommands: search, replace, rename, refs. Use --name, --kind, --text, --old, --new, --dry-run flags." }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "plan",
            "description": "Propose a plan. Plan mode only. --explore: more investigation needed.\n\nExample: plan \"Refactor auth first, then update tests.\"\n\nResponse: OK | plan:<text> | tokens:N\n\tPlan text follows header line.\n\tAfter results: wait for user approval. If approved, start executing first step.\n\tGood: clear steps with dependencies. Bad: vague plan (add specifics), missing edge cases (consider error paths).",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "CLI arguments for plan. Provide plan text, use --explore if more investigation needed." }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "task",
            "description": "Create a new task with preloaded context. Use for major context switches. The new task starts with ONLY the context you provide — make it self-contained with work summary, key files, progress, and next steps.\n\nExample: task \"Refactoring auth. Done: extracted middleware (auth.ts). Next: token refresh tests, login flow update. Key files: src/auth.ts, src/middleware.ts\"\n\nResponse: OK | task_id:<id> | tokens:N\n\tAfter results: new task created. Start working in it. Use compact to save current context if needed.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "CLI arguments for task. Provide task description with self-contained context." }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "tools",
            "description": "Discover available tools. Optional keyword to filter.\n\nExample: tools file\n\nResponse: OK | tools:N | <list> | tokens:N\n\tAfter results: pick the right tool and call it directly. Don't call tools again.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Optional keyword to filter tool names." }
                },
                "required": []
            }
        }),
        json!({
            "name": "memory",
            "description": "Manage saved tool outputs. No args or --list: list files. Filename: read file. --clear: delete all. Use to preserve outputs across compactions.\n\nExample: memory output.txt\n\nResponse: OK | items:N | <list> | tokens:N\nDon't use for: current code (use read/search), temporary data (use bash temp files).\nTypical: memory --list",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "CLI arguments. Use --list (default), filename to read, or --clear to delete all." }
                },
                "required": []
            }
        }),
    ]
});
