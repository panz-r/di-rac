use serde_json::{json, Value};
use std::sync::LazyLock;

pub static TOOL_DEFINITIONS: LazyLock<Vec<Value>> = LazyLock::new(|| {
    vec![
        json!({
            "name": "read",
            "description": "Read file contents with optional detail levels (hint, preview, outline, skeleton, full) and section jumping. Use --detail outline for large files first, then --section to jump to specific symbols.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to read" },
                    "detail": { "type": "string", "enum": ["hint", "preview", "outline", "skeleton", "full"], "description": "Detail level (default: full for small files, preview for large)" },
                    "section": { "type": "string", "description": "Jump to symbol body (e.g. fn:Name, class:Service)" },
                    "range": { "type": "string", "description": "Line range (e.g. 10-50)" },
                    "dry_run": { "type": "boolean", "description": "Preview without executing" },
                    "retry": { "type": "integer", "description": "Retry on error (max 5)" }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "write",
            "description": "Create or overwrite a file. Auto-creates parent directories. Use for creating new files or full rewrites. For targeted edits to existing files, use edit instead.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to write" },
                    "content": { "type": "string", "description": "Full file content" },
                    "create_dirs": { "type": "boolean", "description": "Create parent directories if missing (default: true)" },
                    "dry_run": { "type": "boolean", "description": "Preview without writing" },
                    "retry": { "type": "integer", "description": "Retry on error (max 5)" }
                },
                "required": ["path", "content"]
            }
        }),
        json!({
            "name": "edit",
            "description": "Edit files using old_text/new_text replacements. Supports single-file and multi-file batch editing. Read the file first to get current content before editing.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to edit" },
                    "edits": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "old_text": { "type": "string", "description": "Text to find and replace" },
                                "new_text": { "type": "string", "description": "Replacement text" }
                            },
                            "required": ["old_text", "new_text"]
                        },
                        "description": "Array of {old_text, new_text} edit operations"
                    },
                    "files": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string", "description": "File path" },
                                "edits": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "old_text": { "type": "string" },
                                            "new_text": { "type": "string" }
                                        },
                                        "required": ["old_text", "new_text"]
                                    }
                                }
                            },
                            "required": ["path", "edits"]
                        },
                        "description": "Multi-file batch: array of {path, edits: [{old_text, new_text}]}"
                    },
                    "dry_run": { "type": "boolean", "description": "Preview without applying" },
                    "retry": { "type": "integer", "description": "Retry on error (max 5)" }
                }
            }
        }),
        json!({
            "name": "search",
            "description": "Search files with regex patterns. Skips .git, node_modules, build dirs, and binaries. Returns matches with file, line, and context. For code navigation (functions, classes), use symbols search instead.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex pattern to search for" },
                    "path": { "type": "string", "description": "Directory or file to search in (default: cwd)" },
                    "paths": { "type": "array", "items": { "type": "string" }, "description": "Multiple paths to search" },
                    "context_lines": { "type": "integer", "description": "Context lines around match (0-5, default: 3)" },
                    "file_pattern": { "type": "string", "description": "Glob pattern to filter files (e.g. '*.ts')" },
                    "retry": { "type": "integer", "description": "Retry on error (max 5)" }
                },
                "required": ["pattern"]
            }
        }),
        json!({
            "name": "repo",
            "description": "Get repository structural overview: file listing with line counts, symbol summaries, or full skeleton. Use to explore codebase structure before reading specific files.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path to list (default: cwd)" },
                    "paths": { "type": "array", "items": { "type": "string" }, "description": "Multiple paths" },
                    "detail": { "type": "string", "enum": ["summary", "files", "skeleton"], "description": "Detail level (default: summary)" },
                    "recursive": { "type": "boolean", "description": "List recursively (default: false)" },
                    "retry": { "type": "integer", "description": "Retry on error (max 5)" }
                }
            }
        }),
        json!({
            "name": "bash",
            "description": "Execute shell commands. Supports pipes, &&, ||, heredocs. Long-running commands (>10s) run in background — use --await <id> to retrieve results. Max 8 concurrent background commands. Outputs exceeding 8KB are automatically saved to .di/out/ — use get_outputs to retrieve them.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to execute" },
                    "timeout": { "type": "integer", "description": "Timeout in seconds (default: 300)" },
                    "cwd": { "type": "string", "description": "Working directory" },
                    "await": { "type": "string", "description": "Get result of background command by ID" },
                    "retry": { "type": "integer", "description": "Retry on error (max 5)" }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "compact",
            "description": "Advisory: request conversation compaction. The runtime will use your summary on the next turn if compaction thresholds are met. Not guaranteed to compact immediately.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "context": { "type": "string", "description": "Summary of work done so far — this becomes your new context if compaction occurs" }
                },
                "required": ["context"]
            }
        }),
        json!({
            "name": "ask",
            "description": "Ask the user a follow-up question when you need clarification. Optionally provide 2-5 choices for the user to select from.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "question": { "type": "string", "description": "The question to ask the user" },
                    "options": {
                        "description": "Optional choices for the user (2-5 items). Can be a JSON array of strings or a comma-separated string.",
                        "oneOf": [
                            { "type": "array", "items": { "type": "string" } },
                            { "type": "string" }
                        ]
                    }
                },
                "required": ["question"]
            }
        }),
        json!({
            "name": "done",
            "description": "Mark the task as complete with a result summary. Optionally provide a demo command.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "result": { "type": "string", "description": "Summary of what was accomplished" },
                    "command": { "type": "string", "description": "Optional demo command to verify the result" }
                },
                "required": ["result"]
            }
        }),
        json!({
            "name": "symbols",
            "description": "AST symbol operations: search definitions, replace bodies, rename across files, find references. For text/regex patterns, use search instead.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "subcommand": { "type": "string", "enum": ["search", "replace", "rename", "refs"], "description": "Operation to perform (default: search)" },
                    "path": { "type": "string", "description": "Directory or file path (for search)" },
                    "name": { "type": "string", "description": "Symbol name to search/replace/rename" },
                    "kind": { "type": "string", "enum": ["function", "class", "variable", "constant"], "description": "Symbol kind filter (search only)" },
                    "text": { "type": "string", "description": "New body text (for replace)" },
                    "old": { "type": "string", "description": "Current name (for rename)" },
                    "new": { "type": "string", "description": "New name (for rename)" },
                    "dry_run": { "type": "boolean", "description": "Preview without applying" },
                    "retry": { "type": "integer", "description": "Retry on error (max 5)" }
                }
            }
        }),
        json!({
            "name": "plan",
            "description": "Propose a plan for the task. Use in Plan mode to outline your approach before executing.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "plan": { "type": "string", "description": "The proposed plan text" },
                    "text": { "type": "string", "description": "Alternative: plan text" }
                }
            }
        }),
        json!({
            "name": "task",
            "description": "Create a new task with preloaded context. Use for major context switches where a fresh conversation is needed.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "Description of the new task" },
                    "text": { "type": "string", "description": "Alternative: task description" }
                }
            }
        }),
        json!({
            "name": "tools",
            "description": "List available tools. Optionally filter by keyword.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "filter": { "type": "string", "description": "Optional keyword to filter tool names" }
                }
            }
        }),
        json!({
            "name": "get_outputs",
            "description": "Access saved tool outputs. Large outputs (>8KB) from bash, read, search, and other tools are automatically saved to .di/out/. Use this tool to list, read, or clear them.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "read", "clear"], "description": "Action: list saved outputs, read a specific output, or clear all (default: list)" },
                    "file": { "type": "string", "description": "Filename to read (for action=read)" }
                }
            }
        }),
    ]
});
