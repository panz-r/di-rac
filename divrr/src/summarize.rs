/// Prefix prepended to thinking content in System blocks.
pub const THINKING_PREFIX: char = '\u{00B7}';

/// Summarize tool arguments into a short display string for the UI.
pub fn summarize_tool_args(tool: &str, args: &serde_json::Value) -> String {
    match tool {
        "read" => args.get("path").and_then(|v| v.as_str()).unwrap_or("?").to_string(),
        "write" => args.get("path").and_then(|v| v.as_str()).unwrap_or("?").to_string(),
        "edit" => args.get("path").and_then(|v| v.as_str()).unwrap_or("?").to_string(),
        "bash" => {
            let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("?");
            if cmd.len() > 60 {
                let mut end = 57;
                while !cmd.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...", &cmd[..end])
            } else {
                cmd.to_string()
            }
        }
        "search" => args.get("pattern").and_then(|v| v.as_str()).unwrap_or("?").to_string(),
        "get_outputs" => {
            let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("list");
            match action {
                "list" => "list".to_string(),
                "read" => {
                    let file = args.get("file").and_then(|v| v.as_str()).unwrap_or("?");
                    format!("read {}", file)
                }
                "clear" => "clear".to_string(),
                _ => action.to_string(),
            }
        }
        "symbols" => {
            let sub = args.get("subcommand").and_then(|v| v.as_str()).unwrap_or("search");
            let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.is_empty() { sub.to_string() } else { format!("{} {}", sub, name) }
        }
        _ => args.to_string().chars().take(40).collect(),
    }
}

/// Format a tool call result for display in the UI.
pub fn format_result_summary(result: &serde_json::Value) -> String {
    if let Some(s) = result.as_str() {
        let lines: Vec<&str> = s.lines().take(4).collect();
        return lines.join("\n");
    }
    if let Some(s) = result.get("status").and_then(|v| v.as_str()) {
        s.to_string()
    } else if let Some(s) = result.get("stdout").and_then(|v| v.as_str()) {
        let lines: Vec<&str> = s.lines().take(3).collect();
        lines.join("\n")
    } else {
        let s = result.to_string();
        if s.len() > 80 {
            let mut end = 77;
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}...", &s[..end])
        } else {
            s
        }
    }
}

/// Get the full text of a block for copy/save operations.
pub fn block_full_text(block: &crate::agent::Block) -> String {
    match block {
        crate::agent::Block::User { content }
        | crate::agent::Block::Assistant { content }
        | crate::agent::Block::System { content } => content.clone(),
        crate::agent::Block::Tool { call, result } => {
            let mut s = format!("Tool: {} ({})\n", call.tool, call.args_summary);
            if let Some(r) = result {
                s.push_str(&r.content);
            }
            s
        }
        crate::agent::Block::Finish { message, .. } => message.clone(),
    }
}

/// Convert a char index to a byte index in a string.
pub fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_read() {
        let args = serde_json::json!({ "path": "src/main.rs" });
        assert_eq!(summarize_tool_args("read", &args), "src/main.rs");
    }

    #[test]
    fn summarize_read_missing() {
        let args = serde_json::json!({});
        assert_eq!(summarize_tool_args("read", &args), "?");
    }

    #[test]
    fn summarize_write() {
        let args = serde_json::json!({ "path": "output.txt" });
        assert_eq!(summarize_tool_args("write", &args), "output.txt");
    }

    #[test]
    fn summarize_edit() {
        let args = serde_json::json!({ "path": "src/lib.rs" });
        assert_eq!(summarize_tool_args("edit", &args), "src/lib.rs");
    }

    #[test]
    fn summarize_bash_short() {
        let args = serde_json::json!({ "command": "ls -la" });
        assert_eq!(summarize_tool_args("bash", &args), "ls -la");
    }

    #[test]
    fn summarize_bash_long() {
        let cmd = "a".repeat(100);
        let args = serde_json::json!({ "command": cmd });
        let result = summarize_tool_args("bash", &args);
        assert_eq!(result.len(), 60);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn summarize_bash_non_ascii_boundary() {
        let mut cmd = "a".repeat(56);
        cmd.push('é');
        cmd.push('a');
        cmd.push('a');
        cmd.push('a');
        let args = serde_json::json!({ "command": cmd });
        let result = summarize_tool_args("bash", &args);
        assert!(result.ends_with("..."));
        assert_eq!(result.len(), 59);
    }

    #[test]
    fn summarize_bash_missing() {
        let args = serde_json::json!({});
        assert_eq!(summarize_tool_args("bash", &args), "?");
    }

    #[test]
    fn summarize_search() {
        let args = serde_json::json!({ "pattern": "fn main" });
        assert_eq!(summarize_tool_args("search", &args), "fn main");
    }

    #[test]
    fn summarize_search_missing() {
        let args = serde_json::json!({});
        assert_eq!(summarize_tool_args("search", &args), "?");
    }

    #[test]
    fn summarize_get_outputs_list() {
        let args = serde_json::json!({ "action": "list" });
        assert_eq!(summarize_tool_args("get_outputs", &args), "list");
    }

    #[test]
    fn summarize_get_outputs_read() {
        let args = serde_json::json!({ "action": "read", "file": "out.txt" });
        assert_eq!(summarize_tool_args("get_outputs", &args), "read out.txt");
    }

    #[test]
    fn summarize_get_outputs_clear() {
        let args = serde_json::json!({ "action": "clear" });
        assert_eq!(summarize_tool_args("get_outputs", &args), "clear");
    }

    #[test]
    fn summarize_get_outputs_default_list() {
        let args = serde_json::json!({});
        assert_eq!(summarize_tool_args("get_outputs", &args), "list");
    }

    #[test]
    fn summarize_symbols_search() {
        let args = serde_json::json!({ "subcommand": "search", "name": "foo" });
        assert_eq!(summarize_tool_args("symbols", &args), "search foo");
    }

    #[test]
    fn summarize_symbols_no_name() {
        let args = serde_json::json!({ "subcommand": "search" });
        assert_eq!(summarize_tool_args("symbols", &args), "search");
    }

    #[test]
    fn summarize_symbols_default_subcommand() {
        let args = serde_json::json!({ "name": "bar" });
        assert_eq!(summarize_tool_args("symbols", &args), "search bar");
    }

    #[test]
    fn summarize_unknown_tool() {
        let args = serde_json::json!({ "foo": "bar" });
        let result = summarize_tool_args("unknown", &args);
        assert_eq!(result, r#"{"foo":"bar"}"#);
    }

    #[test]
    fn format_result_plain_string() {
        let result = serde_json::json!("hello\nworld\nline3\nline4\nline5");
        let s = format_result_summary(&result);
        assert_eq!(s, "hello\nworld\nline3\nline4");
    }

    #[test]
    fn format_result_status() {
        let result = serde_json::json!({ "status": "done" });
        assert_eq!(format_result_summary(&result), "done");
    }

    #[test]
    fn format_result_stdout() {
        let result = serde_json::json!({ "stdout": "a\nb\nc\nd\ne" });
        assert_eq!(format_result_summary(&result), "a\nb\nc");
    }

    #[test]
    fn format_result_fallback_short() {
        let result = serde_json::json!({ "foo": 42 });
        assert_eq!(format_result_summary(&result), r#"{"foo":42}"#);
    }

    #[test]
    fn format_result_fallback_long_ascii() {
        let result = serde_json::json!({
            "files": ["a".repeat(50), "b".repeat(50)]
        });
        let s = format_result_summary(&result);
        assert!(s.len() <= 80);
        assert!(s.ends_with("..."));
    }

    #[test]
    fn format_result_fallback_long_unicode() {
        let result = serde_json::json!({
            "data": ["é".repeat(50)]
        });
        let s = format_result_summary(&result);
        assert!(s.ends_with("..."));
    }
}
