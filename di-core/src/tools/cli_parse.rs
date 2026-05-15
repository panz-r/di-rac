//! CLI command parser for tool arguments.
//!
//! The LLM sends tool calls as `{"command": "read src/file.ts --detail outline"}`.
//! This module parses those CLI-style command strings into the structured JSON
//! that each tool handler expects.

use serde_json::{json, Value};

/// Parse a tool call's `command` string into structured arguments.
///
/// If `args` already contains structured fields (no `command` key), returns as-is.
/// Otherwise, parses the `command` string based on the tool name.
pub fn parse_command_args(tool_name: &str, args: &Value) -> Value {
    // If there's no "command" field, assume already structured
    let cmd_str = match args.get("command").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return args.clone(),
    };

    match tool_name {
        "bash" => parse_bash(cmd_str),
        "read" => parse_read(cmd_str),
        "write" => parse_write(cmd_str),
        "edit" => parse_edit(cmd_str),
        "search" => parse_search(cmd_str),
        "repo" => parse_repo(cmd_str),
        "compact" => parse_compact(cmd_str),
        "ask" => parse_ask(cmd_str),
        "done" => parse_done(cmd_str),
        "symbols" => parse_symbols(cmd_str),
        "plan" => parse_plan(cmd_str),
        "task" => parse_task(cmd_str),
        "tools" => parse_tools(cmd_str),
        "memory" => parse_memory(cmd_str),
        _ => {
            // Unknown tool — pass through with command as-is
            let mut result = args.clone();
            result.as_object_mut().map(|m| m.remove("command"));
            result
        }
    }
}

// ---------------------------------------------------------------------------
// Shell tokenizer
// ---------------------------------------------------------------------------

/// Tokenize a command string respecting single and double quotes.
/// Does NOT handle escape sequences inside quotes — matches LLM output patterns.
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Skip whitespace
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }

        if chars[i] == '\'' {
            // Single-quoted string: everything until closing '
            i += 1;
            let start = i;
            while i < len && chars[i] != '\'' {
                i += 1;
            }
            tokens.push(chars[start..i].iter().collect());
            if i < len { i += 1; } // skip closing quote
        } else if chars[i] == '"' {
            // Double-quoted string: everything until closing "
            i += 1;
            let start = i;
            while i < len && chars[i] != '"' {
                i += 1;
            }
            tokens.push(chars[start..i].iter().collect());
            if i < len { i += 1; } // skip closing quote
        } else {
            // Unquoted token: until whitespace
            let start = i;
            while i < len && !chars[i].is_whitespace() {
                i += 1;
            }
            tokens.push(chars[start..i].iter().collect());
        }
    }

    tokens
}

/// Parsed flag state from tokenized input.
struct ParsedFlags {
    flags: Vec<(String, Option<String>)>,
    positionals: Vec<String>,
}

/// Extract --flag [value] pairs and positional arguments from tokens.
fn extract_flags(tokens: &[String]) -> ParsedFlags {
    let mut flags = Vec::new();
    let mut positionals = Vec::new();
    let mut i = 0;

    // Boolean flags that don't take a value
    let bool_flags = [
        "--dry-run", "--recursive", "--create-dirs",
        "--help", "--verbose", "--verify", "--list", "--clear",
    ];

    while i < tokens.len() {
        if tokens[i].starts_with("--") {
            let flag_name = tokens[i].clone();
            // Check if it's a boolean flag (no value) or the next token is another flag
            if bool_flags.contains(&flag_name.as_str()) || i + 1 >= tokens.len() || tokens[i + 1].starts_with("--") {
                flags.push((flag_name, None));
                i += 1;
            } else {
                let value = tokens[i + 1].clone();
                flags.push((flag_name, Some(value)));
                i += 2;
            }
        } else {
            positionals.push(tokens[i].clone());
            i += 1;
        }
    }

    ParsedFlags { flags, positionals }
}

/// Get a flag value by name (without -- prefix).
fn get_flag<'a>(flags: &'a [(String, Option<String>)], name: &str) -> Option<&'a str> {
    let full = format!("--{}", name);
    flags.iter().find(|(k, _)| k == &full).and_then(|(_, v)| v.as_deref())
}

/// Check if a boolean flag is present.
fn has_flag(flags: &[(String, Option<String>)], name: &str) -> bool {
    let full = format!("--{}", name);
    flags.iter().any(|(k, _)| k == &full)
}

/// Get all values for a multi-valued flag (e.g., --keep a --keep b).
fn get_flag_all<'a>(flags: &'a [(String, Option<String>)], name: &str) -> Vec<&'a str> {
    let full = format!("--{}", name);
    flags.iter()
        .filter(|(k, _)| k == &full)
        .filter_map(|(_, v)| v.as_deref())
        .collect()
}

/// Get positional arg at index, or None.
fn pos(positionals: &[String], idx: usize) -> Option<&str> {
    positionals.get(idx).map(|s| s.as_str())
}

/// Strip leading tool name from positionals (e.g. "read" from ["read", "src/main.rs"]).
/// The LLM often includes the tool name as the first token in the command string.
fn strip_tool_name<'a>(tool_name: &str, positionals: &'a [String]) -> &'a [String] {
    if positionals.first().map(|s| s.as_str()) == Some(tool_name) {
        &positionals[1..]
    } else {
        positionals
    }
}

// ---------------------------------------------------------------------------
// Per-tool parsers
// ---------------------------------------------------------------------------

/// bash: the entire command string IS the shell command.
fn parse_bash(cmd: &str) -> Value {
    // Check for --await <id>
    let tokens = tokenize(cmd);
    if let Some(await_idx) = tokens.iter().position(|t| t == "--await") {
        if let Some(id) = tokens.get(await_idx + 1) {
            return json!({ "await": id });
        }
    }

    let timeout = tokens.iter().position(|t| t == "--timeout")
        .and_then(|i| tokens.get(i + 1))
        .and_then(|v| v.parse::<i64>().ok());

    // The shell command is the raw string — don't restructure it
    let mut result = json!({ "command": cmd });
    if let Some(t) = timeout {
        result["timeout"] = json!(t);
    }
    result
}

/// read: [path] [--detail level] [--range start-end] [--section handle]
fn parse_read(cmd: &str) -> Value {
    let tokens = tokenize(cmd);
    let parsed = extract_flags(&tokens);
    let positionals = strip_tool_name("read", &parsed.positionals);

    let mut result = json!({});
    if let Some(path) = pos(positionals, 0) {
        result["path"] = json!(path);
    }
    if let Some(detail) = get_flag(&parsed.flags, "detail") {
        result["detail"] = json!(detail);
    }
    // Range can be specified as --range, --start-line/--end-line, or --ranges
    if let Some(range) = get_flag(&parsed.flags, "range") {
        result["range"] = json!(range);
    }
    if let Some(start) = get_flag(&parsed.flags, "start-line") {
        result["start_line"] = json!(start);
    }
    if let Some(end) = get_flag(&parsed.flags, "end-line") {
        result["end_line"] = json!(end);
    }
    if let Some(ranges) = get_flag(&parsed.flags, "ranges") {
        result["ranges"] = json!(ranges);
    }
    if let Some(section) = get_flag(&parsed.flags, "section") {
        result["section"] = json!(section);
    }
    if let Some(page) = get_flag(&parsed.flags, "page") {
        result["page"] = json!(page);
    }
    if let Some(max_tokens) = get_flag(&parsed.flags, "max-tokens") {
        result["max_tokens"] = json!(max_tokens);
    }
    result
}

/// write: [path] --content <text> [--dry-run]
fn parse_write(cmd: &str) -> Value {
    let tokens = tokenize(cmd);
    let parsed = extract_flags(&tokens);
    let positionals = strip_tool_name("write", &parsed.positionals);

    let mut result = json!({});
    if let Some(path) = pos(positionals, 0) {
        result["path"] = json!(path);
    }
    if let Some(content) = get_flag(&parsed.flags, "content") {
        result["content"] = json!(content);
    }
    if has_flag(&parsed.flags, "verify") {
        result["verify"] = json!(true);
    }
    result
}

/// edit: [path] --anchor <hash|text> [--end-anchor <hash|text>] --content <text> [--type <type>] [--dry-run] [--verify]
fn parse_edit(cmd: &str) -> Value {
    let tokens = tokenize(cmd);
    let parsed = extract_flags(&tokens);
    let positionals = strip_tool_name("edit", &parsed.positionals);

    let mut result = json!({});
    if let Some(path) = pos(positionals, 0) {
        result["path"] = json!(path);
    }
    if let Some(anchor) = get_flag(&parsed.flags, "anchor") {
        result["anchor"] = json!(anchor);
    }
    if let Some(end_anchor) = get_flag(&parsed.flags, "end-anchor") {
        result["end_anchor"] = json!(end_anchor);
    }
    if let Some(content) = get_flag(&parsed.flags, "content") {
        result["content"] = json!(content);
    }
    if let Some(edit_type) = get_flag(&parsed.flags, "type") {
        result["edit_type"] = json!(edit_type);
    }
    if has_flag(&parsed.flags, "dry-run") {
        result["dry_run"] = json!(true);
    }
    if has_flag(&parsed.flags, "verify") {
        result["verify"] = json!(true);
    }
    result
}

/// search: --pattern <regex> [--path <dir>] [--context <n>] [--file-pattern <glob>]
fn parse_search(cmd: &str) -> Value {
    let tokens = tokenize(cmd);
    let parsed = extract_flags(&tokens);
    let positionals = strip_tool_name("search", &parsed.positionals);

    let mut result = json!({});
    if let Some(pattern) = get_flag(&parsed.flags, "pattern") {
        result["pattern"] = json!(pattern);
    } else if let Some(p) = pos(positionals, 0) {
        result["pattern"] = json!(p);
    }
    if let Some(path) = get_flag(&parsed.flags, "path") {
        result["paths"] = json!(path);
    }
    if let Some(ctx) = get_flag(&parsed.flags, "context") {
        result["context_lines"] = json!(ctx.parse::<u64>().unwrap_or(0));
    }
    if let Some(fp) = get_flag(&parsed.flags, "file-pattern") {
        result["file_pattern"] = json!(fp);
    }
    result
}

/// repo: [--detail level] [path]
fn parse_repo(cmd: &str) -> Value {
    let tokens = tokenize(cmd);
    let parsed = extract_flags(&tokens);
    let positionals = strip_tool_name("repo", &parsed.positionals);

    let mut result = json!({});
    if let Some(detail) = get_flag(&parsed.flags, "detail") {
        result["detail"] = json!(detail);
    }
    if let Some(path) = pos(positionals, 0) {
        result["path"] = json!(path);
    }
    result
}

/// compact: [summary text] [--keep path ...]
fn parse_compact(cmd: &str) -> Value {
    let tokens = tokenize(cmd);
    let parsed = extract_flags(&tokens);
    let positionals = strip_tool_name("compact", &parsed.positionals);

    let summary = if !positionals.is_empty() {
        positionals.join(" ")
    } else {
        String::new()
    };

    let mut result = json!({ "context": summary });
    let keep = get_flag_all(&parsed.flags, "keep");
    if !keep.is_empty() {
        result["keep"] = json!(keep);
    }
    result
}

/// ask: [question text] [--options A,B,C]
fn parse_ask(cmd: &str) -> Value {
    let tokens = tokenize(cmd);
    let parsed = extract_flags(&tokens);
    let positionals = strip_tool_name("ask", &parsed.positionals);

    let question = if !positionals.is_empty() {
        positionals.join(" ")
    } else {
        String::new()
    };

    let mut result = json!({ "question": question });
    if let Some(options) = get_flag(&parsed.flags, "options") {
        result["options"] = json!(options);
    }
    result
}

/// done: [result text] [--cmd demo_command]
fn parse_done(cmd: &str) -> Value {
    let tokens = tokenize(cmd);
    let parsed = extract_flags(&tokens);
    let positionals = strip_tool_name("done", &parsed.positionals);

    let result_text = if !positionals.is_empty() {
        positionals.join(" ")
    } else {
        String::new()
    };

    let mut result = json!({ "result": result_text });
    if let Some(demo_cmd) = get_flag(&parsed.flags, "cmd") {
        result["command"] = json!(demo_cmd);
    }
    result
}

/// symbols: [subcommand] [--name pattern] [--kind type] [--text code] [--old name] [--new name] [--path dir]
fn parse_symbols(cmd: &str) -> Value {
    let tokens = tokenize(cmd);
    let parsed = extract_flags(&tokens);
    let positionals = strip_tool_name("symbols", &parsed.positionals);

    let mut result = json!({});
    if let Some(subcmd) = pos(positionals, 0) {
        result["subcommand"] = json!(subcmd);
    }
    if let Some(path) = pos(positionals, 1) {
        result["path"] = json!(path);
    }
    if let Some(name) = get_flag(&parsed.flags, "name") {
        result["name"] = json!(name);
    }
    if let Some(kind) = get_flag(&parsed.flags, "kind") {
        result["kind"] = json!(kind);
    }
    if let Some(text) = get_flag(&parsed.flags, "text") {
        result["text"] = json!(text);
    }
    if let Some(old) = get_flag(&parsed.flags, "old") {
        result["old"] = json!(old);
    }
    if let Some(new) = get_flag(&parsed.flags, "new") {
        result["new"] = json!(new);
    }
    if has_flag(&parsed.flags, "dry-run") {
        result["dry_run"] = json!(true);
    }
    result
}

/// plan: [plan text]
fn parse_plan(cmd: &str) -> Value {
    let tokens = tokenize(cmd);
    let parsed = extract_flags(&tokens);
    let positionals = strip_tool_name("plan", &parsed.positionals);
    let text = if !positionals.is_empty() {
        positionals.join(" ")
    } else {
        String::new()
    };
    json!({ "plan": text })
}

/// task: [task text]
fn parse_task(cmd: &str) -> Value {
    let tokens = tokenize(cmd);
    let parsed = extract_flags(&tokens);
    let positionals = strip_tool_name("task", &parsed.positionals);
    let text = if !positionals.is_empty() {
        positionals.join(" ")
    } else {
        String::new()
    };
    json!({ "task": text })
}

/// tools: [filter keyword]
fn parse_tools(cmd: &str) -> Value {
    let tokens = tokenize(cmd);
    let positionals = strip_tool_name("tools", &tokens);
    if positionals.is_empty() {
        json!({})
    } else {
        json!({ "filter": positionals[0].clone() })
    }
}

/// get_outputs: [action] [filename]
fn parse_memory(cmd: &str) -> Value {
    let tokens = tokenize(cmd);
    let positionals = strip_tool_name("memory", &tokens);
    let mut result = json!({});
    if let Some(action) = positionals.get(0) {
        result["action"] = json!(action);
    }
    if let Some(file) = positionals.get(1) {
        result["file"] = json!(file);
    }
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_simple() {
        assert_eq!(tokenize("read src/file.ts --detail outline"), vec!["read", "src/file.ts", "--detail", "outline"]);
    }

    #[test]
    fn test_tokenize_quoted() {
        assert_eq!(tokenize("search --pattern 'TODO|FIXME'"), vec!["search", "--pattern", "TODO|FIXME"]);
    }

    #[test]
    fn test_tokenize_double_quoted() {
        assert_eq!(tokenize("done \"Fixed the bug\" --cmd 'npm test'"), vec!["done", "Fixed the bug", "--cmd", "npm test"]);
    }

    #[test]
    fn test_parse_read_basic() {
        let result = parse_command_args("read", &json!({ "command": "read src/main.rs --detail outline" }));
        assert_eq!(result["path"], "src/main.rs");
        assert_eq!(result["detail"], "outline");
    }

    #[test]
    fn test_parse_bash_passthrough() {
        let result = parse_command_args("bash", &json!({ "command": "npm test && npm run build" }));
        assert_eq!(result["command"], "npm test && npm run build");
    }

    #[test]
    fn test_parse_bash_await() {
        let result = parse_command_args("bash", &json!({ "command": "--await 42" }));
        assert_eq!(result["await"], "42");
    }

    #[test]
    fn test_parse_search() {
        let result = parse_command_args("search", &json!({ "command": "search --pattern 'TODO|FIXME' --context 2" }));
        assert_eq!(result["pattern"], "TODO|FIXME");
        assert_eq!(result["context_lines"], 2);
    }

    #[test]
    fn test_parse_compact() {
        let result = parse_command_args("compact", &json!({ "command": "compact 'Fixed auth bug' --keep src/auth.ts" }));
        assert_eq!(result["context"], "Fixed auth bug");
        assert_eq!(result["keep"], json!(["src/auth.ts"]));
    }

    #[test]
    fn test_parse_done() {
        let result = parse_command_args("done", &json!({ "command": "done 'Fixed the bug' --cmd 'npm test'" }));
        assert_eq!(result["result"], "Fixed the bug");
        assert_eq!(result["command"], "npm test");
    }

    #[test]
    fn test_parse_symbols() {
        let result = parse_command_args("symbols", &json!({ "command": "symbols search src/ --name AuthService --kind class" }));
        assert_eq!(result["subcommand"], "search");
        assert_eq!(result["path"], "src/");
        assert_eq!(result["name"], "AuthService");
        assert_eq!(result["kind"], "class");
    }

    #[test]
    fn test_parse_edit() {
        let result = parse_command_args("edit", &json!({ "command": "edit src/auth.ts --anchor 'a3|def login():' --content 'new body'" }));
        assert_eq!(result["path"], "src/auth.ts");
        assert_eq!(result["anchor"], "a3|def login():");
        assert_eq!(result["content"], "new body");
    }

    #[test]
    fn test_passthrough_structured() {
        // If no "command" field, return args unchanged
        let original = json!({ "path": "src/main.rs", "detail": "full" });
        let result = parse_command_args("read", &original);
        assert_eq!(result, original);
    }

    #[test]
    fn test_parse_memory() {
        let result = parse_command_args("memory", &json!({ "command": "memory read output.txt" }));
        assert_eq!(result["action"], "read");
        assert_eq!(result["file"], "output.txt");
    }

    #[test]
    fn test_parse_repo() {
        let result = parse_command_args("repo", &json!({ "command": "repo --detail files src/" }));
        assert_eq!(result["detail"], "files");
        assert_eq!(result["path"], "src/");
    }

    #[test]
    fn test_parse_write() {
        let result = parse_command_args("write", &json!({ "command": "write src/new.ts --content 'export const X = 1'" }));
        assert_eq!(result["path"], "src/new.ts");
        assert_eq!(result["content"], "export const X = 1");
    }
}
