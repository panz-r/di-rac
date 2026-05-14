use crate::daemons::ResilientDaemon;
use crate::tools::ToolCall;
use crate::tools::response::{ToolResponse, ToolErrorCode, ToolError};
use crate::util::FileAnchorIndex;
use serde_json::json;
use std::sync::Arc;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Constants (matching TS EditExecutor + EditFormatter)
// ---------------------------------------------------------------------------

const ANCHOR_DELIMITER: &str = "|";
const FUZZY_SEARCH_RADIUS: usize = 5;
const FUZZY_AUTO_THRESHOLD: f64 = 0.90;
const FUZZY_SUGGEST_THRESHOLD: f64 = 0.70;
const MAX_SUGGESTION_DISTANCE: usize = 2;
const MAX_SUGGESTIONS: usize = 3;
const DIFF_CONTEXT_LINES: usize = 3;
const FULL_FILE_FALLBACK_RATIO: f64 = 0.70;

const WHITESPACE_SENSITIVE_EXTENSIONS: &[&str] = &[
    ".py", ".hs", ".lhs", ".yaml", ".yml", ".mak",
];

// ---------------------------------------------------------------------------
// Types (matching TS edit-file/types.ts)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum EditType {
    Replace,
    InsertBefore,
    InsertAfter,
}

#[derive(Debug, Clone)]
pub struct Edit {
    pub anchor: String,
    pub end_anchor: Option<String>,
    pub edit_type: EditType,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedEdit {
    pub line_idx: usize,
    pub end_idx: usize,
    pub edit: Edit,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FailedEdit {
    pub edit: Edit,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct AppliedEdit {
    pub start_idx: usize,
    pub end_idx: usize,
    pub original_start_idx: usize,
    pub original_end_idx: usize,
    pub edit: Edit,
    pub lines_added: usize,
    pub lines_deleted: usize,
}

struct PreparedEdits {
    original_lines: Vec<String>,
    original_hashes: Vec<String>,
    resolved_edits: Vec<ResolvedEdit>,
    failed_edits: Vec<FailedEdit>,
    applied_edits: Vec<AppliedEdit>,
    final_lines: Vec<String>,
}

struct FileEdit {
    path: String,
    edits: Vec<Edit>,
}

// ---------------------------------------------------------------------------
// Anchor parsing (matching TS parseAnchorFromLine)
// ---------------------------------------------------------------------------

/// Parse an anchor string from the LLM into (hash, content).
/// Handles gutter prefix: "   42 │ a3|code" -> ("a3", "code")
/// Also handles bare "a3|code" or just "a3" (hash-only).
fn parse_anchor_from_line(raw: &str) -> Option<(String, String)> {
    if raw.is_empty() {
        return None;
    }
    let trimmed = raw.trim();

    // Strip gutter prefix: digits + whitespace + │ or |
    let after_gutter = if let Some(rest) = strip_gutter(trimmed) {
        rest
    } else {
        trimmed
    };

    let delim_idx = after_gutter.find(ANCHOR_DELIMITER);
    match delim_idx {
        Some(idx) => {
            let hash = &after_gutter[..idx];
            let content = &after_gutter[idx + ANCHOR_DELIMITER.len()..];
            if is_valid_hash(hash) {
                Some((hash.to_string(), content.to_string()))
            } else {
                None
            }
        }
        None => {
            // No delimiter — might be just a hash
            if is_valid_hash(after_gutter) {
                Some((after_gutter.to_string(), String::new()))
            } else {
                None
            }
        }
    }
}

fn strip_gutter(s: &str) -> Option<&str> {
    static GUTTER_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"^\d+\s*[│|]\s*").unwrap()
    });
    let m = GUTTER_RE.find(s)?;
    Some(&s[m.end()..])
}

fn is_valid_hash(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Strip anchor hash prefixes from edit text content.
/// Removes patterns like "a3|" from the start of each line.
fn strip_hashes(content: &str) -> String {
    content
        .split('\n')
        .map(|line| {
            let trimmed = line.trim_start();
            // Check if line starts with hash| prefix
            if let Some(delim) = trimmed.find(ANCHOR_DELIMITER) {
                let prefix = &trimmed[..delim];
                if is_valid_hash(prefix) && prefix.len() <= 5 {
                    return trimmed[delim + 1..].to_string();
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Levenshtein + fuzzy matching (matching TS EditExecutor)
// ---------------------------------------------------------------------------

fn levenshtein(a: &str, b: &str) -> usize {
    let m = a.len();
    let n = b.len();
    if m == 0 { return n; }
    if n == 0 { return m; }

    // Use two rows for space efficiency
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];

    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_bytes[i - 1] == b_bytes[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

fn similarity(a: &str, b: &str) -> f64 {
    let max_len = a.len().max(b.len());
    if max_len == 0 { return 1.0; }
    1.0 - (levenshtein(a, b) as f64) / (max_len as f64)
}

fn normalize_whitespace(s: &str) -> String {
    s.replace('\t', "    ").trim_end().to_string()
}

fn is_whitespace_sensitive(path: &str) -> bool {
    if path.ends_with("Makefile") || path == "Makefile" {
        return true;
    }
    let last_dot = path.rfind('.');
    match last_dot {
        Some(idx) => {
            let ext = &path[idx..].to_lowercase();
            WHITESPACE_SENSITIVE_EXTENSIONS.contains(&ext.as_str())
        }
        None => false,
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let end = s.floor_char_boundary(max_len);
        format!("{}...", &s[..end])
    }
}

fn fuzzy_match_content(
    provided_content: &str,
    anchor_line_idx: usize,
    index: &FileAnchorIndex,
    is_ws_sensitive: bool,
) -> Option<(usize, f64)> {
    let start_line = anchor_line_idx.saturating_sub(FUZZY_SEARCH_RADIUS);
    let end_line = (anchor_line_idx + FUZZY_SEARCH_RADIUS).min(index.line_count().saturating_sub(1));

    let mut best_match: Option<(usize, f64)> = None;

    for i in start_line..=end_line {
        let candidate = index.get_line(i);
        let (cmp_provided, cmp_candidate) = if is_ws_sensitive {
            (provided_content.to_string(), candidate.to_string())
        } else {
            (normalize_whitespace(provided_content), normalize_whitespace(candidate))
        };

        let score = similarity(&cmp_provided, &cmp_candidate);
        match best_match {
            Some((_, best_score)) if score > best_score => {
                best_match = Some((i, score));
            }
            None => {
                best_match = Some((i, score));
            }
            _ => {}
        }
    }

    match best_match {
        Some((idx, confidence)) if confidence >= FUZZY_SUGGEST_THRESHOLD => {
            Some((idx, confidence))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Anchor resolution (matching TS EditExecutor.resolveAnchor)
// ---------------------------------------------------------------------------

struct AnchorResolution {
    line_idx: i64, // -1 = unresolved
    error: Option<String>,
    warning: Option<String>,
}

fn build_unknown_anchor_error(hash: &str, index: &FileAnchorIndex) -> String {
    let all_hashes = index.get_all_hashes();
    let mut scored: Vec<(&str, usize, usize)> = all_hashes
        .iter()
        .enumerate()
        .map(|(i, h)| (h.as_str(), levenshtein(hash, h), i))
        .collect();
    scored.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.2.cmp(&b.2)));

    let suggestions: Vec<String> = scored
        .iter()
        .filter(|(_, dist, _)| *dist <= MAX_SUGGESTION_DISTANCE)
        .take(MAX_SUGGESTIONS)
        .map(|(h, _, line_idx)| {
            let content = truncate(index.get_line(*line_idx), 40);
            format!("\"{}\" (line {}): '{}'", h, line_idx + 1, content)
        })
        .collect();

    if suggestions.is_empty() {
        format!(
            "Anchor \"{}\" not found in the file. Please re-read the file to get current anchors.",
            hash
        )
    } else {
        format!(
            "Anchor \"{}\" not found. Did you mean: {}?",
            hash,
            suggestions.join(", ")
        )
    }
}

fn resolve_anchor(
    anchor_type: &str,
    raw_anchor: Option<&str>,
    index: &FileAnchorIndex,
    is_ws_sensitive: bool,
) -> AnchorResolution {
    let anchor_raw = match raw_anchor {
        Some(s) if !s.trim().is_empty() => s,
        _ => {
            return AnchorResolution {
                line_idx: -1,
                error: Some(format!("{} is missing.", anchor_type)),
                warning: None,
            };
        }
    };

    let parsed = match parse_anchor_from_line(anchor_raw) {
        Some(p) => p,
        None => {
            return AnchorResolution {
                line_idx: -1,
                error: Some(format!(
                    "{} is missing or incorrectly formatted. It must follow the format \"hash{}content\" (e.g., \"a3{}code\").",
                    anchor_type, ANCHOR_DELIMITER, ANCHOR_DELIMITER
                )),
                warning: None,
            };
        }
    };

    let (hash, provided_content_raw) = parsed;

    let line_idx = match index.get_line_idx(&hash) {
        Some(idx) => idx,
        None => {
            return AnchorResolution {
                line_idx: -1,
                error: Some(build_unknown_anchor_error(&hash, index)),
                warning: None,
            };
        }
    };

    // Strip echo prefix (LLM sometimes echoes a hash prefix in content)
    static ECHO_RE: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"^[a-z0-9_]{1,32}\|").unwrap());
    let provided_content = ECHO_RE.replace(&provided_content_raw, "").to_string();

    if provided_content.contains('\n') || provided_content.contains('\r') {
        return AnchorResolution {
            line_idx: -1,
            error: Some(format!(
                "{} \"{}\" exists, but the provided code line contains a newline character. Use format hash{}{{line_text}}.",
                anchor_type, hash, ANCHOR_DELIMITER
            )),
            warning: None,
        };
    }

    let actual_content = index.get_line(line_idx).to_string();
    let (provided_cmp, actual_cmp) = if is_ws_sensitive {
        (provided_content.clone(), actual_content.clone())
    } else {
        (normalize_whitespace(&provided_content), normalize_whitespace(&actual_content))
    };

    if provided_cmp == actual_cmp {
        return AnchorResolution {
            line_idx: line_idx as i64,
            error: None,
            warning: None,
        };
    }

    // Content mismatch — try fuzzy matching
    match fuzzy_match_content(&provided_content, line_idx, index, is_ws_sensitive) {
        Some((fuzzy_idx, confidence)) if confidence >= FUZZY_AUTO_THRESHOLD => {
            AnchorResolution {
                line_idx: fuzzy_idx as i64,
                error: None,
                warning: Some(format!(
                    "Anchor \"{}\" drifted — fuzzy-resolved to line {} ({}% match).",
                    hash,
                    fuzzy_idx + 1,
                    (confidence * 100.0).round() as usize
                )),
            }
        }
        Some((fuzzy_idx, confidence)) if confidence >= FUZZY_SUGGEST_THRESHOLD => {
            let fuzzy_line = index.get_line(fuzzy_idx);
            AnchorResolution {
                line_idx: -1,
                error: Some(format!(
                    "Anchor \"{}\" is stale (current: '{}'). Fuzzy match at line {}: '{}' ({}%). Re-read the file or use the suggested line.",
                    hash,
                    truncate(&actual_content, 60),
                    fuzzy_idx + 1,
                    truncate(fuzzy_line, 60),
                    (confidence * 100.0).round() as usize
                )),
                warning: None,
            }
        }
        _ => {
            AnchorResolution {
                line_idx: -1,
                error: Some(format!(
                    "Anchor \"{}\" is stale. Current content: '{}' with new anchor {}.",
                    hash,
                    truncate(&actual_content, 60),
                    index.get_hash(line_idx)
                )),
                warning: None,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Edit resolution (matching TS EditExecutor.resolveEdits)
// ---------------------------------------------------------------------------

fn resolve_edits(
    edits: &[Edit],
    index: &FileAnchorIndex,
    file_path: &str,
) -> (Vec<ResolvedEdit>, Vec<FailedEdit>) {
    let mut resolved = Vec::new();
    let mut failed = Vec::new();
    let is_ws_sensitive = is_whitespace_sensitive(file_path);

    for edit in edits {
        let mut diagnostics: Vec<String> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();

        let start = resolve_anchor("anchor", Some(&edit.anchor), index, is_ws_sensitive);
        if let Some(e) = start.error {
            diagnostics.push(e);
        }
        if let Some(w) = start.warning {
            warnings.push(w);
        }

        let mut end_idx = start.line_idx;
        let edit_type = &edit.edit_type;
        if *edit_type == EditType::Replace {
            let end = resolve_anchor(
                "end_anchor",
                edit.end_anchor.as_deref(),
                index,
                is_ws_sensitive,
            );
            if let Some(e) = end.error {
                diagnostics.push(e);
            }
            if let Some(w) = end.warning {
                warnings.push(w);
            }
            end_idx = end.line_idx;
        }

        if start.line_idx >= 0 && end_idx >= 0 && end_idx < start.line_idx {
            diagnostics.push("Range error: anchor must refer to a line that precedes or is the same as end_anchor.".to_string());
        }

        if !diagnostics.is_empty() {
            failed.push(FailedEdit {
                edit: edit.clone(),
                error: diagnostics.join(" "),
            });
        } else if start.line_idx >= 0 {
            resolved.push(ResolvedEdit {
                line_idx: start.line_idx as usize,
                end_idx: end_idx as usize,
                edit: edit.clone(),
                warnings,
            });
        } else {
            failed.push(FailedEdit {
                edit: edit.clone(),
                error: diagnostics.join(" "),
            });
        }
    }

    (resolved, failed)
}

// ---------------------------------------------------------------------------
// Edit application (matching TS EditExecutor.applyEdits)
// ---------------------------------------------------------------------------

fn apply_edits(
    lines: &[String],
    resolved_edits: &[ResolvedEdit],
    index: &mut FileAnchorIndex,
) -> (Vec<String>, Vec<AppliedEdit>) {
    // Sort bottom-to-top
    let mut sorted: Vec<&ResolvedEdit> = resolved_edits.iter().collect();
    sorted.sort_by(|a, b| b.line_idx.cmp(&a.line_idx));

    let mut new_lines: Vec<String> = lines.to_vec();
    let mut changes: Vec<(usize, usize, usize, Edit)> = Vec::new(); // (orig_idx, replacement_count, removed_count, edit)

    for re in &sorted {
        let clean_text = strip_hashes(&re.edit.text).replace("\r\n", "\n");
        // Trim trailing newline to avoid spurious empty line in output
        let trimmed = clean_text.trim_end_matches('\n');
        let replacement_lines: Vec<String> = if trimmed.is_empty() {
            Vec::new()
        } else {
            trimmed.split('\n').map(|s| s.to_string()).collect()
        };

        let (splice_index, removed_count) = match re.edit.edit_type {
            EditType::InsertAfter => (re.line_idx + 1, 0),
            EditType::InsertBefore => (re.line_idx, 0),
            EditType::Replace => (re.line_idx, re.end_idx - re.line_idx + 1),
        };

        new_lines.splice(splice_index..splice_index + removed_count, replacement_lines.clone());

        // Note: anchor index is rebuilt from scratch after all edits apply,
        // so no per-line index update needed here.

        changes.push((re.line_idx, replacement_lines.len(), removed_count, re.edit.clone()));
    }

    // Compute AppliedEdit metadata with shift offsets using signed arithmetic
    // so that edits which remove more lines than they add produce negative shifts.
    let applied_edits: Vec<AppliedEdit> = changes
        .iter()
        .map(|(orig_idx, rep_count, removed, edit)| {
            let shift: isize = changes
                .iter()
                .filter(|(other_orig, _, _, _)| *other_orig < *orig_idx)
                .map(|(_, rep, rem, _)| (*rep as isize) - (*rem as isize))
                .sum();

            let start = (*orig_idx as isize + shift) as usize;
            let end = (*orig_idx as isize + shift + (*rep_count as isize - 1).max(0)) as usize;

            AppliedEdit {
                start_idx: start,
                end_idx: end,
                original_start_idx: *orig_idx,
                original_end_idx: orig_idx + removed.saturating_sub(1),
                edit: edit.clone(),
                lines_added: *rep_count,
                lines_deleted: *removed,
            }
        })
        .collect();

    (new_lines, applied_edits)
}

// ---------------------------------------------------------------------------
// Diff formatting (matching TS EditFormatter)
// ---------------------------------------------------------------------------

fn format_line_with_hash(line: &str, hash: &str) -> String {
    format!("{}{}{}", hash, ANCHOR_DELIMITER, line)
}

fn get_diff_block(
    original_lines: &[String],
    original_hashes: &[String],
    final_lines: &[String],
    final_hashes: &[String],
    applied: &AppliedEdit,
) -> String {
    let mut res = Vec::new();

    // Context before (from original)
    let before_start = applied.original_start_idx.saturating_sub(DIFF_CONTEXT_LINES);
    for i in before_start..applied.original_start_idx {
        if i < original_lines.len() {
            let h = original_hashes.get(i).map(|s| s.as_str()).unwrap_or("");
            res.push(format!(" {}", format_line_with_hash(&original_lines[i], h)));
        }
    }

    // Deleted lines
    let final_hashes_set: std::collections::HashSet<&str> =
        final_hashes[applied.start_idx..=applied.end_idx.min(final_hashes.len() - 1)]
            .iter()
            .map(|s| s.as_str())
            .collect();
    for i in applied.original_start_idx..=applied.original_end_idx.min(original_lines.len() - 1) {
        let h = original_hashes.get(i).map(|s| s.as_str()).unwrap_or("");
        if !final_hashes_set.contains(h) {
            res.push(format!("-{}", format_line_with_hash(&original_lines[i], h)));
        }
    }

    // Added/unchanged lines (from final)
    let original_hashes_set: std::collections::HashSet<&str> =
        original_hashes[applied.original_start_idx..=applied.original_end_idx.min(original_hashes.len() - 1)]
            .iter()
            .map(|s| s.as_str())
            .collect();
    for i in applied.start_idx..=applied.end_idx.min(final_lines.len() - 1) {
        let h = final_hashes.get(i).map(|s| s.as_str()).unwrap_or("");
        let prefix = if original_hashes_set.contains(h) { " " } else { "+" };
        res.push(format!("{}{}", prefix, format_line_with_hash(&final_lines[i], h)));
    }

    // Context after (from final)
    let after_end = (applied.end_idx + DIFF_CONTEXT_LINES).min(final_lines.len().saturating_sub(1));
    for i in (applied.end_idx + 1)..=after_end {
        let h = final_hashes.get(i).map(|s| s.as_str()).unwrap_or("");
        res.push(format!(" {}", format_line_with_hash(&final_lines[i], h)));
    }

    res.join("\n")
}

fn get_addition_only_diff_block(
    original_lines: &[String],
    original_hashes: &[String],
    final_lines: &[String],
    final_hashes: &[String],
    applied: &AppliedEdit,
) -> String {
    let mut res = Vec::new();

    // Context before
    let before_start = applied.original_start_idx.saturating_sub(DIFF_CONTEXT_LINES);
    for i in before_start..applied.original_start_idx {
        if i < original_lines.len() {
            let h = original_hashes.get(i).map(|s| s.as_str()).unwrap_or("");
            res.push(format!(" {}", format_line_with_hash(&original_lines[i], h)));
        }
    }

    // Deletion summary
    let final_hashes_set: std::collections::HashSet<&str> =
        final_hashes[applied.start_idx..=applied.end_idx.min(final_hashes.len() - 1)]
            .iter()
            .map(|s| s.as_str())
            .collect();
    let truly_removed: usize = (applied.original_start_idx..=applied.original_end_idx.min(original_lines.len() - 1))
        .filter(|i| {
            let h = original_hashes.get(*i).map(|s| s.as_str()).unwrap_or("");
            !final_hashes_set.contains(h)
        })
        .count();

    if truly_removed > 0 {
        res.push(format!(
            "{} lines between {} and {} have been deleted",
            truly_removed,
            applied.edit.anchor.split(ANCHOR_DELIMITER).next().unwrap_or(""),
            applied.edit.end_anchor.as_deref().unwrap_or("").split(ANCHOR_DELIMITER).next().unwrap_or("")
        ));
    }

    // Added/neutral lines
    let original_hashes_set: std::collections::HashSet<&str> =
        original_hashes[applied.original_start_idx..=applied.original_end_idx.min(original_hashes.len() - 1)]
            .iter()
            .map(|s| s.as_str())
            .collect();
    for i in applied.start_idx..=applied.end_idx.min(final_lines.len() - 1) {
        let h = final_hashes.get(i).map(|s| s.as_str()).unwrap_or("");
        let prefix = if original_hashes_set.contains(h) { " " } else { "+" };
        res.push(format!("{}{}", prefix, format_line_with_hash(&final_lines[i], h)));
    }

    // Context after
    let after_end = (applied.end_idx + DIFF_CONTEXT_LINES).min(final_lines.len().saturating_sub(1));
    for i in (applied.end_idx + 1)..=after_end {
        let h = final_hashes.get(i).map(|s| s.as_str()).unwrap_or("");
        res.push(format!(" {}", format_line_with_hash(&final_lines[i], h)));
    }

    res.join("\n")
}

fn create_results_response(
    prepared: &PreparedEdits,
    final_hashes: &[String],
    was_stringified: bool,
) -> String {
    let mut results: Vec<String> = Vec::new();
    let mut total_added = 0usize;
    let mut total_removed = 0usize;

    // Compute added/removed counts and diff blocks
    let original_hashes = &prepared.original_hashes;
    let original_lines = &prepared.original_lines;
    let final_lines = &prepared.final_lines;

    for applied in &prepared.applied_edits {
        let orig_set: std::collections::HashSet<&str> =
            original_hashes[applied.original_start_idx..=applied.original_end_idx.min(original_hashes.len() - 1)]
                .iter()
                .map(|s| s.as_str())
                .collect();
        let final_set: std::collections::HashSet<&str> =
            final_hashes[applied.start_idx..=applied.end_idx.min(final_hashes.len() - 1)]
                .iter()
                .map(|s| s.as_str())
                .collect();

        for i in applied.original_start_idx..=applied.original_end_idx.min(original_lines.len() - 1) {
            let h = original_hashes.get(i).map(|s| s.as_str()).unwrap_or("");
            if !final_set.contains(h) {
                total_removed += 1;
            }
        }
        for i in applied.start_idx..=applied.end_idx.min(final_lines.len() - 1) {
            let h = final_hashes.get(i).map(|s| s.as_str()).unwrap_or("");
            if !orig_set.contains(h) {
                total_added += 1;
            }
        }

        let diff_block = get_diff_block(original_lines, original_hashes, final_lines, final_hashes, applied);
        results.push(diff_block);
    }

    // Full-file fallback if diff > 70% of file
    let total_diff_lines: usize = results.iter().map(|r| r.lines().count()).sum();
    let use_full_file = total_diff_lines > (final_lines.len() as f64 * FULL_FILE_FALLBACK_RATIO) as usize
        && !final_lines.is_empty();

    let mut output_parts: Vec<String> = Vec::new();

    if use_full_file {
        let full = final_lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let h = final_hashes.get(i).map(|s| s.as_str()).unwrap_or("");
                format_line_with_hash(line, h)
            })
            .collect::<Vec<_>>()
            .join("\n");
        output_parts.push(format!(
            "Because the changes were extensive, the full updated file content with anchors is provided below to ensure clarity:\n\n{}",
            full
        ));
    } else {
        output_parts.extend(results);
    }

    // Failed edit messages
    for failed in &prepared.failed_edits {
        output_parts.push(format_failure_message(&failed.edit, &failed.error));
    }

    // Fuzzy warnings
    let fuzzy_warnings: Vec<String> = prepared
        .resolved_edits
        .iter()
        .flat_map(|r| r.warnings.iter().cloned())
        .collect();
    if !fuzzy_warnings.is_empty() {
        output_parts.push(format!("Fuzzy anchor resolution: {}", fuzzy_warnings.join(" ")));
    }

    // Literal \n detection
    for applied in &prepared.applied_edits {
        if applied.edit.text.contains("\\n") {
            let anchor_name = applied.edit.anchor.split(ANCHOR_DELIMITER).next().unwrap_or("");
            let end_name = applied.edit.end_anchor.as_deref()
                .and_then(|s| s.split(ANCHOR_DELIMITER).next());
            let end_part = end_name.map(|n| format!(" and ending with {}", n)).unwrap_or_default();
            output_parts.push(format!(
                "Your edit starting with {}{} inserted a '\\n' literal in the code because you supplied double backslash '\\\\n'. If you meant to add a newline char instead, update it using '\\n' in the next call. You do not need escape characters in the text portion",
                anchor_name, end_part
            ));
        }
    }

    if was_stringified {
        output_parts.push(
            "Note: Your edit arguments were auto-corrected. Use CLI syntax: edit <path> --anchor <id> --content <text>.".to_string()
        );
    }

    let line_changes = format!(" (+{}, -{} lines)", total_added, total_removed);
    let failed_note = if !prepared.failed_edits.is_empty() {
        format!(" {} edit(s) failed.", prepared.failed_edits.len())
    } else {
        String::new()
    };
    let summary = format!(
        "Applied {} edit(s) successfully{}. NOTE the UPDATED anchors below.{}",
        prepared.resolved_edits.len(),
        line_changes,
        failed_note
    );

    format!("{}\n\n{}", summary, output_parts.join("\n\n---\n\n"))
}

fn format_failure_message(edit: &Edit, error: &str) -> String {
    let diagnostic = if error.is_empty() {
        " This almost certainly is because the anchors used were incorrect or not in ascending order or the text supplied was incorrect. Please check again before editing.".to_string()
    } else {
        format!(" Diagnostics: {}", error)
    };
    format!(
        "Edit (anchor: \"{}\", end_anchor: \"{}\") failed.{}",
        edit.anchor,
        edit.end_anchor.as_deref().unwrap_or(""),
        diagnostic
    )
}

// ---------------------------------------------------------------------------
// Input parsing (replaces old parse_edits)
// ---------------------------------------------------------------------------

fn parse_edits(call: &ToolCall) -> Result<Vec<FileEdit>, String> {
    // Try to auto-parse stringified JSON
    let args = &call.args;

    // Schema 1: CLI-style single edit (anchor field present)
    if let Some(anchor) = args.get("anchor").and_then(|v| v.as_str()) {
        let path = args.get("path")
            .or_else(|| args.get("filePath"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing path argument for edit".to_string())?;

        let end_anchor = args.get("end_anchor").and_then(|v| v.as_str()).map(String::from);
        let text = args.get("content")
            .or_else(|| args.get("text"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing content/text argument for edit".to_string())?
            .to_string();

        let edit_type = parse_edit_type(args.get("edit_type").or_else(|| args.get("type")));

        return Ok(vec![FileEdit {
            path: path.to_string(),
            edits: vec![Edit {
                anchor: anchor.to_string(),
                end_anchor,
                edit_type,
                text,
            }],
        }]);
    }

    // Schema 2: files array
    if let Some(files) = args.get("files") {
        let files_arr = auto_parse_array(files)?;
        let mut result = Vec::new();
        for file_entry in &files_arr {
            let path = file_entry.get("path")
                .or_else(|| file_entry.get("filePath"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing path in files array".to_string())?;
            let edits = parse_edits_array(file_entry)?;
            result.push(FileEdit {
                path: path.to_string(),
                edits,
            });
        }
        return Ok(result);
    }

    // Schema 3: path + edits array
    let path = args.get("path")
        .or_else(|| args.get("filePath"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing path argument for edit".to_string())?;
    let edits = parse_edits_array(args)?;
    Ok(vec![FileEdit {
        path: path.to_string(),
        edits,
    }])
}

fn parse_edits_array(obj: &serde_json::Value) -> Result<Vec<Edit>, String> {
    let edits_val = obj.get("edits").ok_or_else(|| "Missing edits array".to_string())?;
    let edits_arr = auto_parse_array(edits_val)?;

    edits_arr
        .iter()
        .map(|edit| {
            let anchor = edit.get("anchor")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing anchor in edit".to_string())?
                .to_string();
            let end_anchor = edit.get("end_anchor").and_then(|v| v.as_str()).map(String::from);
            let text = edit.get("text")
                .or_else(|| edit.get("content"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing text/content in edit".to_string())?
                .to_string();
            let edit_type = parse_edit_type(
                edit.get("edit_type").or_else(|| edit.get("type"))
            );
            Ok(Edit { anchor, end_anchor, edit_type, text })
        })
        .collect()
}

fn parse_edit_type(val: Option<&serde_json::Value>) -> EditType {
    match val.and_then(|v| v.as_str()).unwrap_or("replace") {
        "insert_before" => EditType::InsertBefore,
        "insert_after" => EditType::InsertAfter,
        _ => EditType::Replace,
    }
}

fn auto_parse_array(val: &serde_json::Value) -> Result<Vec<serde_json::Value>, String> {
    match val {
        serde_json::Value::Array(arr) => Ok(arr.clone()),
        serde_json::Value::String(s) => {
            serde_json::from_str(s).map_err(|e| format!("Failed to parse JSON string: {}", e))
        }
        _ => Err("Expected array".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Top-level handler
// ---------------------------------------------------------------------------

pub async fn edit_file(
    _command_daemon: &Arc<tokio::sync::Mutex<ResilientDaemon>>,
    call: &ToolCall,
) -> ToolResponse {
    let file_edits = match parse_edits(call) {
        Ok(fe) => fe,
        Err(e) => return ToolResponse::fail(ToolErrorCode::MissingArgument, e, "edit"),
    };

    let dry_run = call.args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);
    let was_stringified = call.args.get("_was_stringified").and_then(|v| v.as_bool()).unwrap_or(false);

    let mut all_results = Vec::new();

    for fe in &file_edits {
        let content = match std::fs::read_to_string(&fe.path) {
            Ok(c) => c,
            Err(e) => {
                return ToolResponse::fail(
                    ToolErrorCode::IoFileNotFound,
                    format!("Failed to read {}: {}", fe.path, e),
                    "edit",
                );
            }
        };

        let normalized = content.replace("\r\n", "\n");
        let lines: Vec<String> = normalized.split('\n').map(|s| s.to_string()).collect();
        let line_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let mut index = FileAnchorIndex::new(&line_refs);

        let (resolved_edits, failed_edits) = resolve_edits(&fe.edits, &index, &fe.path);

        if resolved_edits.is_empty() && !failed_edits.is_empty() {
            let messages: Vec<String> = failed_edits
                .iter()
                .map(|f| format_failure_message(&f.edit, &f.error))
                .collect();
            return ToolResponse::Failure {
                error: ToolError::new(
                    ToolErrorCode::AnchorNotFound,
                    messages.join("\n\n"),
                    "edit",
                ).with_details(json!({ "path": fe.path })),
                metadata: None,
            };
        }

        let original_hashes: Vec<String> = (0..lines.len())
            .map(|i| index.get_hash(i).to_string())
            .collect();

        let (final_lines, applied_edits) = apply_edits(&lines, &resolved_edits, &mut index);

        // Write to disk (atomic: write to temp file, then rename)
        if !dry_run {
            let final_content = final_lines.join("\n");
            let tmp_path = format!("{}.tmp.{}", fe.path, std::process::id());
            if let Err(e) = std::fs::write(&tmp_path, &final_content) {
                let _ = std::fs::remove_file(&tmp_path);
                return ToolResponse::fail(
                    ToolErrorCode::PatchApplyFailed,
                    format!("Failed to write {}: {}", fe.path, e),
                    "edit",
                );
            }
            if let Err(e) = std::fs::rename(&tmp_path, &fe.path) {
                let _ = std::fs::remove_file(&tmp_path);
                return ToolResponse::fail(
                    ToolErrorCode::PatchApplyFailed,
                    format!("Failed to rename temp file for {}: {}", fe.path, e),
                    "edit",
                );
            }
        }

        // Compute final hashes
        let final_line_refs: Vec<&str> = final_lines.iter().map(|s| s.as_str()).collect();
        let final_index = FileAnchorIndex::new(&final_line_refs);
        let final_hashes: Vec<String> = (0..final_lines.len())
            .map(|i| final_index.get_hash(i).to_string())
            .collect();

        let prepared = PreparedEdits {
            original_lines: lines,
            original_hashes,
            resolved_edits,
            failed_edits,
            applied_edits,
            final_lines,
        };

        let response_text = create_results_response(&prepared, &final_hashes, was_stringified);
        all_results.push(json!({
            "path": fe.path,
            "result": response_text,
        }));
    }

    if all_results.len() == 1 {
        ToolResponse::ok(json!({
            "edits": all_results,
            "result": all_results[0].get("result"),
        }))
    } else {
        let combined = all_results
            .iter()
            .filter_map(|r| r.get("result").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
            .join("\n\n");
        ToolResponse::ok(json!({
            "edits": all_results,
            "result": combined,
        }))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Anchor parsing ---

    #[test]
    fn parse_bare_hash() {
        let (hash, content) = parse_anchor_from_line("a3f").unwrap();
        assert_eq!(hash, "a3f");
        assert_eq!(content, "");
    }

    #[test]
    fn parse_hash_with_content() {
        let (hash, content) = parse_anchor_from_line("a3f|fn main() {").unwrap();
        assert_eq!(hash, "a3f");
        assert_eq!(content, "fn main() {");
    }

    #[test]
    fn parse_gutter_prefixed() {
        let (hash, content) = parse_anchor_from_line("   42 │ a3f|code here").unwrap();
        assert_eq!(hash, "a3f");
        assert_eq!(content, "code here");
    }

    #[test]
    fn parse_gutter_pipe_separator() {
        let (hash, content) = parse_anchor_from_line("42 | a3f|code").unwrap();
        assert_eq!(hash, "a3f");
        assert_eq!(content, "code");
    }

    #[test]
    fn parse_invalid_returns_none() {
        assert!(parse_anchor_from_line("").is_none());
        assert!(parse_anchor_from_line("   ").is_none());
        assert!(parse_anchor_from_line("|just content no hash").is_none());
    }

    #[test]
    fn parse_collision_suffixed_hash() {
        let (hash, content) = parse_anchor_from_line("a3f_0|code").unwrap();
        assert_eq!(hash, "a3f_0");
        assert_eq!(content, "code");
    }

    // --- Levenshtein / similarity ---

    #[test]
    fn levenshtein_identical() {
        assert_eq!(levenshtein("hello", "hello"), 0);
    }

    #[test]
    fn levenshtein_different() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
    }

    #[test]
    fn similarity_identical() {
        assert!((similarity("hello", "hello") - 1.0).abs() < 0.001);
    }

    #[test]
    fn similarity_empty() {
        assert!((similarity("", "") - 1.0).abs() < 0.001);
    }

    // --- Whitespace sensitivity ---

    #[test]
    fn python_is_ws_sensitive() {
        assert!(is_whitespace_sensitive("src/main.py"));
    }

    #[test]
    fn rust_is_not_ws_sensitive() {
        assert!(!is_whitespace_sensitive("src/main.rs"));
    }

    #[test]
    fn makefile_is_ws_sensitive() {
        assert!(is_whitespace_sensitive("Makefile"));
    }

    #[test]
    fn yaml_is_ws_sensitive() {
        assert!(is_whitespace_sensitive("config.yaml"));
    }

    // --- Strip hashes ---

    #[test]
    fn strip_hashes_removes_prefix() {
        assert_eq!(strip_hashes("a3f|hello"), "hello");
    }

    #[test]
    fn strip_hashes_multiline() {
        assert_eq!(strip_hashes("a3f|line1\nb7k|line2"), "line1\nline2");
    }

    #[test]
    fn strip_hashes_no_prefix() {
        assert_eq!(strip_hashes("plain text"), "plain text");
    }

    // --- Resolve anchor ---

    #[test]
    fn resolve_anchor_exact_match() {
        let lines: Vec<&str> = vec!["fn main() {", "    println!(\"hello\");", "}"];
        let index = FileAnchorIndex::new(&lines);
        let hash = index.get_hash(0).to_string();
        let anchor_str = format!("{}|fn main() {{", hash);

        let result = resolve_anchor("anchor", Some(&anchor_str), &index, false);
        assert_eq!(result.line_idx, 0);
        assert!(result.error.is_none());
    }

    #[test]
    fn resolve_anchor_not_found() {
        let lines: Vec<&str> = vec!["hello"];
        let index = FileAnchorIndex::new(&lines);
        let result = resolve_anchor("anchor", Some("zzz|hello"), &index, false);
        assert_eq!(result.line_idx, -1);
        assert!(result.error.is_some());
        let err = result.error.unwrap();
        assert!(err.contains("not found"));
    }

    #[test]
    fn resolve_anchor_missing() {
        let lines: Vec<&str> = vec!["hello"];
        let index = FileAnchorIndex::new(&lines);
        let result = resolve_anchor("anchor", None, &index, false);
        assert_eq!(result.line_idx, -1);
        assert!(result.error.unwrap().contains("missing"));
    }

    // --- Resolve edits ---

    #[test]
    fn resolve_edits_mixed_success_failure() {
        let lines: Vec<&str> = vec!["line one", "line two", "line three"];
        let index = FileAnchorIndex::new(&lines);
        let h0 = index.get_hash(0).to_string();
        let h2 = index.get_hash(2).to_string();

        let edits = vec![
            Edit {
                anchor: format!("{}|line one", h0),
                end_anchor: Some(format!("{}|line one", h0)),
                edit_type: EditType::Replace,
                text: "replaced".to_string(),
            },
            Edit {
                anchor: "zzz|nonexistent".to_string(),
                end_anchor: None,
                edit_type: EditType::Replace,
                text: "wont work".to_string(),
            },
            Edit {
                anchor: format!("{}|line three", h2),
                end_anchor: None,
                edit_type: EditType::InsertAfter,
                text: "inserted".to_string(),
            },
        ];

        let (resolved, failed) = resolve_edits(&edits, &index, "test.rs");
        assert_eq!(resolved.len(), 2);
        assert_eq!(failed.len(), 1);
        assert!(failed[0].error.contains("not found"));
    }

    // --- Apply edits ---

    #[test]
    fn apply_single_replace() {
        let lines: Vec<String> = vec!["aaa".into(), "bbb".into(), "ccc".into()];
        let line_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let mut index = FileAnchorIndex::new(&line_refs);

        let edits = vec![ResolvedEdit {
            line_idx: 1,
            end_idx: 1,
            edit: Edit {
                anchor: "x|bbb".into(),
                end_anchor: None,
                edit_type: EditType::Replace,
                text: "replaced".into(),
            },
            warnings: vec![],
        }];

        let (final_lines, applied) = apply_edits(&lines, &edits, &mut index);
        assert_eq!(final_lines, vec!["aaa", "replaced", "ccc"]);
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].lines_added, 1);
        assert_eq!(applied[0].lines_deleted, 1);
    }

    #[test]
    fn apply_insert_after() {
        let lines: Vec<String> = vec!["aaa".into(), "bbb".into()];
        let line_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let mut index = FileAnchorIndex::new(&line_refs);

        let edits = vec![ResolvedEdit {
            line_idx: 0,
            end_idx: 0,
            edit: Edit {
                anchor: "x|aaa".into(),
                end_anchor: None,
                edit_type: EditType::InsertAfter,
                text: "inserted".into(),
            },
            warnings: vec![],
        }];

        let (final_lines, _) = apply_edits(&lines, &edits, &mut index);
        assert_eq!(final_lines, vec!["aaa", "inserted", "bbb"]);
    }

    #[test]
    fn apply_insert_before() {
        let lines: Vec<String> = vec!["aaa".into(), "bbb".into()];
        let line_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let mut index = FileAnchorIndex::new(&line_refs);

        let edits = vec![ResolvedEdit {
            line_idx: 1,
            end_idx: 1,
            edit: Edit {
                anchor: "x|bbb".into(),
                end_anchor: None,
                edit_type: EditType::InsertBefore,
                text: "inserted".into(),
            },
            warnings: vec![],
        }];

        let (final_lines, _) = apply_edits(&lines, &edits, &mut index);
        assert_eq!(final_lines, vec!["aaa", "inserted", "bbb"]);
    }

    #[test]
    fn apply_multi_line_replace() {
        let lines: Vec<String> = vec!["a".into(), "b".into(), "c".into(), "d".into()];
        let line_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let mut index = FileAnchorIndex::new(&line_refs);

        let edits = vec![ResolvedEdit {
            line_idx: 1,
            end_idx: 2,
            edit: Edit {
                anchor: "x|b".into(),
                end_anchor: Some("y|c".into()),
                edit_type: EditType::Replace,
                text: "new1\nnew2".into(),
            },
            warnings: vec![],
        }];

        let (final_lines, applied) = apply_edits(&lines, &edits, &mut index);
        assert_eq!(final_lines, vec!["a", "new1", "new2", "d"]);
        assert_eq!(applied[0].lines_added, 2);
        assert_eq!(applied[0].lines_deleted, 2);
    }

    #[test]
    fn apply_bottom_to_top_ordering() {
        let lines: Vec<String> = vec!["a".into(), "b".into(), "c".into(), "d".into()];
        let line_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let mut index = FileAnchorIndex::new(&line_refs);

        let edits = vec![
            ResolvedEdit {
                line_idx: 0,
                end_idx: 0,
                edit: Edit {
                    anchor: "x|a".into(),
                    end_anchor: None,
                    edit_type: EditType::Replace,
                    text: "A".into(),
                },
                warnings: vec![],
            },
            ResolvedEdit {
                line_idx: 2,
                end_idx: 2,
                edit: Edit {
                    anchor: "y|c".into(),
                    end_anchor: None,
                    edit_type: EditType::Replace,
                    text: "C".into(),
                },
                warnings: vec![],
            },
        ];

        let (final_lines, _) = apply_edits(&lines, &edits, &mut index);
        assert_eq!(final_lines, vec!["A", "b", "C", "d"]);
    }

    // --- Diff formatting ---

    #[test]
    fn diff_block_has_context_and_prefixes() {
        let original: Vec<String> = vec!["ctx0".into(), "old".into(), "ctx2".into(), "ctx3".into()];
        let final_lines: Vec<String> = vec!["ctx0".into(), "new".into(), "ctx2".into(), "ctx3".into()];
        let orig_hashes: Vec<String> = vec!["h0".into(), "h1".into(), "h2".into(), "h3".into()];
        let final_hashes: Vec<String> = vec!["h0".into(), "h4".into(), "h2".into(), "h3".into()];

        let applied = AppliedEdit {
            start_idx: 1, end_idx: 1,
            original_start_idx: 1, original_end_idx: 1,
            edit: Edit {
                anchor: "h1|old".into(),
                end_anchor: Some("h1|old".into()),
                edit_type: EditType::Replace,
                text: "new".into(),
            },
            lines_added: 1, lines_deleted: 1,
        };

        let diff = get_diff_block(&original, &orig_hashes, &final_lines, &final_hashes, &applied);
        assert!(diff.contains(" h0|ctx0")); // context before
        assert!(diff.contains("-h1|old"));  // deletion
        assert!(diff.contains("+h4|new"));  // addition
        assert!(diff.contains(" h2|ctx2")); // context after
    }

    // --- Input parsing ---

    #[test]
    fn parse_cli_style() {
        let call = ToolCall {
            name: "edit".into(),
            args: json!({
                "path": "test.rs",
                "anchor": "a3f|code",
                "end_anchor": "b7k|more",
                "content": "new code",
                "edit_type": "replace"
            }),
        };
        let result = parse_edits(&call).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "test.rs");
        assert_eq!(result[0].edits.len(), 1);
        assert_eq!(result[0].edits[0].anchor, "a3f|code");
        assert_eq!(result[0].edits[0].end_anchor, Some("b7k|more".into()));
        assert_eq!(result[0].edits[0].text, "new code");
    }

    #[test]
    fn parse_structured_with_edits() {
        let call = ToolCall {
            name: "edit".into(),
            args: json!({
                "path": "test.rs",
                "edits": [
                    { "anchor": "a3f|code", "text": "new" },
                    { "anchor": "b7k|more", "edit_type": "insert_after", "text": "inserted" }
                ]
            }),
        };
        let result = parse_edits(&call).unwrap();
        assert_eq!(result[0].edits.len(), 2);
        assert_eq!(result[0].edits[0].edit_type, EditType::Replace);
        assert_eq!(result[0].edits[1].edit_type, EditType::InsertAfter);
    }

    #[test]
    fn parse_files_array() {
        let call = ToolCall {
            name: "edit".into(),
            args: json!({
                "files": [
                    {
                        "path": "a.rs",
                        "edits": [{ "anchor": "h1|x", "end_anchor": "h2|y", "text": "new" }]
                    },
                    {
                        "path": "b.rs",
                        "edits": [{ "anchor": "h3|z", "text": "other" }]
                    }
                ]
            }),
        };
        let result = parse_edits(&call).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].path, "a.rs");
        assert_eq!(result[1].path, "b.rs");
    }

    #[test]
    fn parse_stringified_json() {
        let call = ToolCall {
            name: "edit".into(),
            args: json!({
                "path": "test.rs",
                "edits": "[{\"anchor\":\"h1|x\",\"text\":\"new\"}]"
            }),
        };
        let result = parse_edits(&call).unwrap();
        assert_eq!(result[0].edits.len(), 1);
        assert_eq!(result[0].edits[0].anchor, "h1|x");
    }

    #[test]
    fn parse_missing_path_fails() {
        let call = ToolCall {
            name: "edit".into(),
            args: json!({"anchor": "h1|x", "content": "new"}),
        };
        assert!(parse_edits(&call).is_err());
    }

    // --- Full-file fallback ---

    #[test]
    fn full_file_fallback_triggered() {
        // Create a small file where the diff > 70% of lines
        let original: Vec<String> = (0..3).map(|i| format!("line{}", i)).collect();
        let final_lines: Vec<String> = (0..3).map(|i| format!("changed{}", i)).collect();
        let orig_hashes: Vec<String> = (0..3).map(|i| format!("oh{}", i)).collect();
        let final_hashes: Vec<String> = (0..3).map(|i| format!("fh{}", i)).collect();

        let prepared = PreparedEdits {
            original_lines: original.clone(),
            original_hashes: orig_hashes,
            resolved_edits: vec![ResolvedEdit {
                line_idx: 0, end_idx: 2,
                edit: Edit {
                    anchor: "oh0|line0".into(),
                    end_anchor: Some("oh2|line2".into()),
                    edit_type: EditType::Replace,
                    text: "changed".into(),
                },
                warnings: vec![],
            }],
            failed_edits: vec![],
            applied_edits: vec![AppliedEdit {
                start_idx: 0, end_idx: 2,
                original_start_idx: 0, original_end_idx: 2,
                edit: Edit {
                    anchor: "oh0|line0".into(),
                    end_anchor: Some("oh2|line2".into()),
                    edit_type: EditType::Replace,
                    text: "changed".into(),
                },
                lines_added: 3, lines_deleted: 3,
            }],
            final_lines,
        };

        let response = create_results_response(&prepared, &final_hashes, false);
        assert!(response.contains("full updated file content with anchors"));
    }

    // --- Literal \n detection ---

    #[test]
    fn detect_literal_newline() {
        let prepared = PreparedEdits {
            original_lines: vec!["old".into()],
            original_hashes: vec!["h0".into()],
            resolved_edits: vec![ResolvedEdit {
                line_idx: 0, end_idx: 0,
                edit: Edit {
                    anchor: "h0|old".into(),
                    end_anchor: Some("h0|old".into()),
                    edit_type: EditType::Replace,
                    text: "line1\\nline2".into(),
                },
                warnings: vec![],
            }],
            failed_edits: vec![],
            applied_edits: vec![AppliedEdit {
                start_idx: 0, end_idx: 0,
                original_start_idx: 0, original_end_idx: 0,
                edit: Edit {
                    anchor: "h0|old".into(),
                    end_anchor: Some("h0|old".into()),
                    edit_type: EditType::Replace,
                    text: "line1\\nline2".into(),
                },
                lines_added: 1, lines_deleted: 1,
            }],
            final_lines: vec!["line1\\nline2".into()],
        };

        let response = create_results_response(&prepared, &["h1".to_string()], false);
        assert!(response.contains("literal"));
    }
}
