use crate::daemons::ResilientDaemon;
use crate::tools::ToolCall;
use crate::tools::read_file::line_hash;
use crate::tools::response::{ToolResponse, ToolErrorCode};
use serde_json::json;
use std::sync::Arc;

const FUZZY_SUGGEST_THRESHOLD: f64 = 0.7;

pub async fn edit_file(
    _command_daemon: &Arc<tokio::sync::Mutex<ResilientDaemon>>,
    call: &ToolCall,
) -> ToolResponse {
    let path = match call.args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return ToolResponse::fail(ToolErrorCode::MissingArgument, "Missing path argument", "edit"),
    };
    let anchor = match call.args.get("anchor").and_then(|v| v.as_str()) {
        Some(a) => a.to_string(),
        None => return ToolResponse::fail(ToolErrorCode::MissingArgument, "Missing --anchor argument", "edit"),
    };
    let content = call.args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let end_anchor = call.args.get("end_anchor").and_then(|v| v.as_str()).map(String::from);
    let edit_type = call.args.get("edit_type").and_then(|v| v.as_str()).unwrap_or("replace");
    let dry_run = call.args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);

    let file_content = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(e) => {
            return ToolResponse::fail(
                ToolErrorCode::IoFileNotFound,
                format!("Failed to read {}: {}", path, e),
                "edit",
            );
        }
    };

    let lines: Vec<&str> = file_content.lines().collect();
    if lines.is_empty() {
        return ToolResponse::fail(ToolErrorCode::AnchorNotFound, format!("File {} is empty", path), "edit");
    }

    // Resolve anchor to a line number (1-based)
    let start_line = match resolve_anchor(&anchor, &lines) {
        Ok(n) => n,
        Err(ResolveError::FuzzyMatch { suggestions }) => {
            let msg = format!(
                "Anchor not found in {}. Closest matches:\n{}Re-read the file to get current anchors.",
                path,
                suggestions.iter()
                    .take(3)
                    .map(|(ln, sim, text)| format!("  line {} ({:.0}% match): {}", ln, sim * 100.0, text))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            return ToolResponse::fail(ToolErrorCode::AnchorNotFound, msg, "edit");
        }
        Err(ResolveError::NotFound) => {
            return ToolResponse::fail(
                ToolErrorCode::AnchorNotFound,
                format!("Anchor '{}' not found in {}. Re-read the file to get current anchors.", anchor, path),
                "edit",
            );
        }
    };

    // Resolve end_anchor if provided
    let end_line = if let Some(ref ea) = end_anchor {
        match resolve_anchor(ea, &lines) {
            Ok(n) => Some(n),
            Err(_) => {
                return ToolResponse::fail(
                    ToolErrorCode::AnchorNotFound,
                    format!("End-anchor '{}' not found in {}", ea, path),
                    "edit",
                );
            }
        }
    } else {
        None
    };

    // Determine the range of lines to work with
    let new_lines: Vec<&str> = content.lines().collect();
    let mut result_lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();

    let diff = match edit_type {
        "replace" => {
            let end = end_line.unwrap_or(start_line);
            if end < start_line {
                return ToolResponse::fail(
                    ToolErrorCode::AnchorNotFound,
                    format!("end-anchor line {} is before start-anchor line {}", end, start_line),
                    "edit",
                );
            }
            // Collect removed lines for diff
            let removed: Vec<&str> = lines[start_line - 1..end].to_vec();
            // Replace lines [start_line-1..end) with new content
            result_lines.splice(start_line - 1..end, new_lines.iter().map(|l| l.to_string()));
            build_range_diff(start_line, &removed, &new_lines)
        }
        "insert_after" => {
            // Insert new lines after the anchor line
            let insert_pos = start_line; // 0-based index after start_line
            let anchor_text = lines[start_line - 1];
            result_lines.splice(insert_pos..insert_pos, new_lines.iter().map(|l| l.to_string()));
            build_insert_diff(start_line, anchor_text, &new_lines, "after")
        }
        "insert_before" => {
            // Insert new lines before the anchor line
            let insert_pos = start_line - 1; // 0-based index at start_line
            let anchor_text = lines[start_line - 1];
            result_lines.splice(insert_pos..insert_pos, new_lines.iter().map(|l| l.to_string()));
            build_insert_diff(start_line, anchor_text, &new_lines, "before")
        }
        other => {
            return ToolResponse::fail(
                ToolErrorCode::MissingArgument,
                format!("Unknown edit type '{}'. Use: replace, insert_after, insert_before", other),
                "edit",
            );
        }
    };

    if dry_run {
        return ToolResponse::ok(json!({
            "path": path,
            "status": "dry_run",
            "diff": diff,
        }));
    }

    let new_content = result_lines.join("\n");
    // Preserve trailing newline if original had one
    let final_content = if file_content.ends_with('\n') {
        format!("{}\n", new_content)
    } else {
        new_content
    };

    match tokio::fs::write(&path, &final_content).await {
        Ok(_) => {
            ToolResponse::ok(json!({
                "path": path,
                "status": "success",
                "diff": diff,
            }))
        }
        Err(e) => {
            ToolResponse::fail(
                ToolErrorCode::PatchApplyFailed,
                format!("Failed to write {}: {}", path, e),
                "edit",
            )
        }
    }
}

/// Resolve an anchor string to a 1-based line number.
/// Anchor format: `hash|content` or plain content.
fn resolve_anchor(anchor: &str, lines: &[&str]) -> Result<usize, ResolveError> {
    let parts: Vec<&str> = anchor.splitn(2, '|').collect();
    let (expected_hash, expected_content) = if parts.len() == 2 {
        (Some(parts[0].trim()), parts[1].trim())
    } else {
        (None, anchor.trim())
    };

    // Try exact hash match first
    if let Some(hash) = expected_hash {
        for (i, line) in lines.iter().enumerate() {
            let h = line_hash(line.trim_end());
            if h == hash && line.trim_end().contains(expected_content) {
                return Ok(i + 1); // 1-based
            }
        }
    }

    // Try content-only match
    for (i, line) in lines.iter().enumerate() {
        if line.trim() == expected_content || line.trim_end() == expected_content {
            return Ok(i + 1);
        }
    }

    // Try substring match
    for (i, line) in lines.iter().enumerate() {
        if line.contains(expected_content) {
            return Ok(i + 1);
        }
    }

    // Fuzzy match — find closest lines
    let mut suggestions: Vec<(usize, f64, String)> = lines.iter().enumerate()
        .filter_map(|(i, line)| {
            let sim = similarity(expected_content, line.trim());
            if sim >= FUZZY_SUGGEST_THRESHOLD {
                Some((i + 1, sim, line.trim().to_string()))
            } else {
                None
            }
        })
        .collect();
    suggestions.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if suggestions.is_empty() {
        Err(ResolveError::NotFound)
    } else {
        Err(ResolveError::FuzzyMatch { suggestions })
    }
}

enum ResolveError {
    NotFound,
    FuzzyMatch { suggestions: Vec<(usize, f64, String)> },
}

/// Simple Jaccard-like similarity on words. Good enough for fuzzy line matching.
fn similarity(a: &str, b: &str) -> f64 {
    if a.is_empty() && b.is_empty() { return 1.0; }
    if a.is_empty() || b.is_empty() { return 0.0; }

    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();
    if a_lower == b_lower { return 1.0; }

    // Use longest common subsequence ratio as approximation
    let lcs_len = lcs(&a_lower, &b_lower);
    (2.0 * lcs_len as f64) / (a.len() as f64 + b.len() as f64)
}

/// Longest common subsequence length.
fn lcs(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let al = a_chars.len();
    let bl = b_chars.len();
    if al == 0 || bl == 0 { return 0; }

    let mut prev = vec![0usize; bl + 1];
    let mut curr = vec![0usize; bl + 1];

    for i in 1..=al {
        for j in 1..=bl {
            curr[j] = if a_chars[i - 1] == b_chars[j - 1] {
                prev[j - 1] + 1
            } else {
                prev[j].max(curr[j - 1])
            };
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[bl]
}

fn build_range_diff(start_line: usize, removed: &[&str], added: &[&str]) -> String {
    let mut diff = String::new();
    for line in removed.iter() {
        diff.push_str(&format!("  {}|{}\n", line_hash(line.trim_end()), line));
    }
    diff.push('\n');
    for line in added {
        diff.push_str(&format!("+{}\n", line));
    }
    format!("Lines {}-{}:\n{}", start_line, start_line + removed.len() - 1, diff)
}

fn build_insert_diff(anchor_line: usize, anchor_text: &str, inserted: &[&str], position: &str) -> String {
    let mut diff = String::new();
    diff.push_str(&format!("  {}|{} (anchor)\n", line_hash(anchor_text.trim_end()), anchor_text));
    for line in inserted {
        diff.push_str(&format!("+{}\n", line));
    }
    format!("Insert {} line {}:\n{}", position, anchor_line, diff)
}
