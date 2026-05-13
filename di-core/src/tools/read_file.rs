use std::collections::HashMap;

const MAX_CACHE_ENTRIES: usize = 256;

/// Per-session cache for read file results: tracks content hashes for
/// unchanged detection and cursor positions for pagination.
pub struct ReadFileCache {
    hashes: HashMap<String, String>,
    /// Insertion-order tracking for eviction.
    keys_order: Vec<String>,
    #[allow(dead_code)]
    cursors: HashMap<String, usize>,
}

impl ReadFileCache {
    pub fn new() -> Self {
        Self {
            hashes: HashMap::new(),
            keys_order: Vec::new(),
            cursors: HashMap::new(),
        }
    }

    fn cache_key(path: &str, mode: &str, range: Option<(usize, usize)>) -> String {
        match range {
            Some((s, e)) => format!("{}:{}:{}-{}", path, mode, s, e),
            None => format!("{}:{}:full", path, mode),
        }
    }

    pub fn get_hash(&self, path: &str, mode: &str, range: Option<(usize, usize)>) -> Option<&str> {
        let key = Self::cache_key(path, mode, range);
        self.hashes.get(&key).map(|s| s.as_str())
    }

    pub fn set_hash(&mut self, path: &str, mode: &str, range: Option<(usize, usize)>, hash: String) {
        let key = Self::cache_key(path, mode, range);
        if !self.hashes.contains_key(&key) {
            // Evict oldest entries if at capacity
            while self.hashes.len() >= MAX_CACHE_ENTRIES {
                if let Some(old_key) = self.keys_order.first().cloned() {
                    self.keys_order.remove(0);
                    self.hashes.remove(&old_key);
                } else {
                    break;
                }
            }
            self.keys_order.push(key.clone());
        }
        self.hashes.insert(key, hash);
    }

    #[allow(dead_code)]
    pub fn get_cursor(&self, path: &str) -> usize {
        *self.cursors.get(path).unwrap_or(&0)
    }

    /// Invalidate all cached entries for a given path (call after edits).
    pub fn invalidate_for_path(&mut self, path: &str) {
        let prefix = format!("{}:", path);
        self.hashes.retain(|key, _| !key.starts_with(&prefix));
        self.keys_order.retain(|key| !key.starts_with(&prefix));
    }

    #[allow(dead_code)]
    pub fn set_cursor(&mut self, path: &str, line: usize) {
        self.cursors.insert(path.to_string(), line);
    }
}

pub const MAX_FILE_READ_SIZE: usize = 50 * 1024;
pub const DEFAULT_PREVIEW_LINES: usize = 200;
pub const AUTO_EXPAND_PREVIEW_LINES: usize = 500;
pub const AUTO_EXPAND_READ_THRESHOLD: usize = 3;

/// 8-char content hash for unchanged detection.
fn content_hash(content: &str) -> String {
    crate::util::stable_hash(content.as_bytes())[..8].to_string()
}

/// Width for line number column based on total lines.
fn line_num_width(total_lines: usize) -> usize {
    let digits = if total_lines == 0 { 1 } else { (total_lines as f64).log10().floor() as usize + 1 };
    digits.max(4)
}

/// Normalize CRLF to LF for consistent line splitting.
fn normalize_newlines(content: &str) -> std::borrow::Cow<'_, str> {
    if content.contains("\r\n") {
        std::borrow::Cow::Owned(content.replace("\r\n", "\n"))
    } else {
        std::borrow::Cow::Borrowed(content)
    }
}

/// Split content into lines matching TS `content.split(/\r?\n/)` behavior.
/// Unlike `str::lines()`, this preserves a trailing empty string for content
/// ending with `\n`. E.g. `"a\nb\n"` produces `["a", "b", ""]`.
fn split_lines(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return vec![""];
    }
    content.split('\n').collect()
}

/// Count lines using the same semantics as `split_lines` (preserves trailing empty line).
pub fn count_lines(content: &str) -> usize {
    if content.is_empty() {
        return 1;
    }
    content.split('\n').count()
}

/// Format a single line with anchored gutter: `   42 │ a3f|code here`
fn format_anchored_line(line: &str, line_num: usize, num_width: usize, hash: &str) -> String {
    format!("{:>width$} │ {}|{}", line_num, hash, line, width = num_width)
}

/// Format full file content with anchored lines and range support.
/// Returns (formatted_output, hash, unchanged).
pub fn format_full(
    path: &str,
    content: &str,
    range: Option<(usize, usize)>,
    cache: &mut ReadFileCache,
) -> (String, String, bool) {
    let normalized = normalize_newlines(content);
    let lines: Vec<&str> = split_lines(&normalized);
    let total_lines = lines.len();

    let anchor_index = crate::util::FileAnchorIndex::new(&lines);

    if let Some((start, end)) = range {
        let start = start.max(1);
        let end = end.min(total_lines);
        if start > end || start > total_lines {
            return (
                format!("[Lines {}-{}: invalid range (file has {} lines)]", start, end, total_lines),
                String::new(),
                false,
            );
        }
        let slice: Vec<&str> = lines[start - 1..end].to_vec();
        let range_content = slice.join("\n");
        let hash = content_hash(&range_content);

        // Unchanged detection
        if let Some(last) = cache.get_hash(path, "full", Some((start, end))) {
            if last == hash {
                return (
                    format!("[Lines {}-{}: unchanged since your last read (Hash: {})]", start, end, hash),
                    hash,
                    true,
                );
            }
        }
        cache.set_hash(path, "full", Some((start, end)), hash.clone());

        let num_width = line_num_width(total_lines);
        let anchored: Vec<String> = slice.iter().enumerate().map(|(i, line)| {
            let ln = start + i;
            let lh = anchor_index.get_hash(ln - 1);
            format_anchored_line(line, ln, num_width, lh)
        }).collect();

        let output = format!("[Lines {}-{}, Hash: {}]\n{}", start, end, hash, anchored.join("\n"));
        (output, hash, false)
    } else {
        // Full file
        let hash = content_hash(content);

        if let Some(last) = cache.get_hash(path, "full", None) {
            if last == hash {
                return (
                    format!("[Full file: unchanged since your last read (Hash: {})]", hash),
                    hash,
                    true,
                );
            }
        }
        cache.set_hash(path, "full", None, hash.clone());

        let num_width = line_num_width(total_lines);
        let anchored: Vec<String> = lines.iter().enumerate().map(|(i, line)| {
            let lh = anchor_index.get_hash(i);
            format_anchored_line(line, i + 1, num_width, lh)
        }).collect();

        let output = format!("[File Hash: {}]\n{}", hash, anchored.join("\n"));
        (output, hash, false)
    }
}

/// Format preview output: first N lines with anchors + line count hint.
pub fn format_preview(
    path: &str,
    content: &str,
    read_count: usize,
    cache: &mut ReadFileCache,
) -> (String, String, bool) {
    let normalized = normalize_newlines(content);
    let lines: Vec<&str> = split_lines(&normalized);
    let total_lines = lines.len();

    let preview_lines = if read_count >= AUTO_EXPAND_READ_THRESHOLD {
        AUTO_EXPAND_PREVIEW_LINES
    } else {
        DEFAULT_PREVIEW_LINES
    };

    let take = preview_lines.min(total_lines);
    let slice = &lines[..take];
    let preview_content = slice.join("\n");
    let hash = content_hash(&preview_content);

    if let Some(last) = cache.get_hash(path, "preview", None) {
        if last == hash {
            return (
                format!("[Preview: unchanged since your last read (Hash: {})]", hash),
                hash,
                true,
            );
        }
    }
    cache.set_hash(path, "preview", None, hash.clone());

    let anchor_index = crate::util::FileAnchorIndex::new(&lines);

    let num_width = line_num_width(total_lines);
    let anchored: Vec<String> = slice.iter().enumerate().map(|(i, line)| {
        let lh = anchor_index.get_hash(i);
        format_anchored_line(line, i + 1, num_width, lh)
    }).collect();

    let size_kb = (content.len() as f64 / 1024.0).round() as usize;
    let expand_note = if read_count >= AUTO_EXPAND_READ_THRESHOLD {
        "\nNOTE: Extended preview shown due to multiple reads."
    } else {
        ""
    };
    let next_line = take + 1;
    let nav_hint = if total_lines > take {
        format!(
            "\nTo view other sections, use read {} --range \"{}-{}\".",
            path, next_line, next_line + DEFAULT_PREVIEW_LINES - 1
        )
    } else {
        String::new()
    };

    let note = format!(
        "\n\nNOTE: File '{}' is {} KB (~{} lines). Showing first {} lines.{}{}",
        path, size_kb, total_lines, take, expand_note, nav_hint,
    );

    let output = format!("[File Hash: {}]\n{}{}", hash, anchored.join("\n"), note);
    (output, hash, false)
}

/// Choose the detail level automatically based on file size.
/// Files > 50KB default to preview, everything else to full.
/// Only auto-selects when no range/page/section context is active.
pub fn auto_detail(file_size: usize, requested: Option<&str>, has_range: bool, has_page: bool, has_section: bool) -> String {
    if let Some(d) = requested {
        d.to_string()
    } else if has_range || has_page || has_section {
        "full".to_string()
    } else if file_size > MAX_FILE_READ_SIZE {
        "preview".to_string()
    } else {
        "full".to_string()
    }
}

// ---------------------------------------------------------------------------
// Multi-range support
// ---------------------------------------------------------------------------

/// Parse range argument(s) into a list of (start, end) tuples.
/// Handles string "10-20,30-40" and JSON array [{start,end}].
pub fn parse_ranges(raw: &serde_json::Value) -> Option<Vec<(usize, usize)>> {
    let ranges: Vec<(usize, usize)> = match raw {
        serde_json::Value::String(s) => {
            s.split(',')
                .filter_map(|part| {
                    let part = part.trim();
                    if let Some(dash) = part.find('-') {
                        let start: usize = part[..dash].parse().ok()?;
                        let end: usize = part[dash + 1..].parse().ok()?;
                        if start >= 1 && start <= end { Some((start, end)) } else { None }
                    } else if let Ok(n) = part.parse::<usize>() {
                        if n >= 1 { Some((n, n)) } else { None }
                    } else {
                        None
                    }
                })
                .collect()
        }
        serde_json::Value::Array(arr) => {
            arr.iter()
                .filter_map(|v| {
                    let start = v.get("start").and_then(|v| v.as_u64())? as usize;
                    let end = v.get("end").and_then(|v| v.as_u64())? as usize;
                    if start >= 1 && start <= end { Some((start, end)) } else { None }
                })
                .collect()
        }
        _ => return None,
    };
    if ranges.is_empty() { None } else { Some(ranges) }
}

/// Merge overlapping or close ranges (within 5 lines of each other).
pub fn merge_ranges(ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    if ranges.len() <= 1 {
        return ranges;
    }
    let mut sorted = ranges;
    sorted.sort_by_key(|r| r.0);
    let mut merged = vec![sorted[0]];
    for current in sorted.iter().skip(1) {
        let last = merged.last_mut().unwrap();
        if current.0 <= last.1 + 5 {
            last.1 = last.1.max(current.1);
        } else {
            merged.push(*current);
        }
    }
    merged
}

/// Format multiple ranges in one file, each as a separate block.
pub fn format_ranges(
    path: &str,
    content: &str,
    ranges: &[(usize, usize)],
    cache: &mut ReadFileCache,
) -> (String, String, bool) {
    let mut parts = Vec::new();
    let mut combined_hash = String::new();
    let mut any_changed = false;

    for &(start, end) in ranges {
        let (output, hash, unchanged) = format_full(path, content, Some((start, end)), cache);
        combined_hash.push_str(&hash);
        any_changed = any_changed || !unchanged;
        parts.push(output);
    }

    let final_hash = content_hash(&combined_hash);
    (parts.join("\n\n"), final_hash, !any_changed)
}

// ---------------------------------------------------------------------------
// Hint detail level
// ---------------------------------------------------------------------------

/// Format hint output: just `kind name` per symbol, no line numbers.
pub fn format_hint(path: &str, data: &serde_json::Value, cache: &mut ReadFileCache) -> serde_json::Value {
    let hash = content_hash(&data.to_string());
    cache.set_hash(path, "hint", None, hash.clone());

    let symbols = data.get("symbols").and_then(|s| s.as_array()).cloned().unwrap_or_default();
    let mut entries = Vec::new();
    for sym in &symbols {
        let name = sym.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let kind = sym.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        entries.push(format!("{} {}", kind, name));
    }

    let output = if entries.is_empty() {
        format!("[File Hash: {}]\nNo definitions found.", hash)
    } else {
        format!("[File Hash: {}]\n{}", hash, entries.join("\n"))
    };

    serde_json::Value::String(output)
}

// ---------------------------------------------------------------------------
// Markdown outline/hint
// ---------------------------------------------------------------------------

const MD_EXTENSIONS: &[&str] = &[".md", ".mdx", ".txt"];

/// Check if a file path is a markdown-like file.
pub fn is_markdown(path: &str) -> bool {
    MD_EXTENSIONS.iter().any(|ext| path.to_lowercase().ends_with(ext))
}

/// Extract markdown headings as an outline.
pub fn format_markdown_outline(content: &str) -> String {
    let mut headings = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('#') {
            if rest.starts_with(' ') || rest.starts_with('\t') {
                let level = trimmed.len() - rest.len(); // count of # chars
                if level >= 1 && level <= 6 {
                    let title = rest.trim();
                    let hashes = &trimmed[..level];
                    headings.push(format!("  {} {} (line {})", hashes, title, i + 1));
                }
            }
        }
    }
    if headings.is_empty() {
        "No headings found.".to_string()
    } else {
        headings.join("\n")
    }
}

/// Extract markdown headings as hint (just the text, no line numbers).
pub fn format_markdown_hint(content: &str) -> String {
    let mut headings = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('#') {
            if rest.starts_with(' ') || rest.starts_with('\t') {
                let title = rest.trim();
                headings.push(title.to_string());
            }
        }
    }
    if headings.is_empty() {
        "No headings found.".to_string()
    } else {
        headings.join("\n")
    }
}

// ---------------------------------------------------------------------------
// Chunk map (preview mode symbol listing)
// ---------------------------------------------------------------------------

/// Format a chunk map from analyzer symbols for preview mode.
pub fn format_chunk_map(data: &serde_json::Value) -> String {
    let symbols = data.get("symbols").and_then(|s| s.as_array()).cloned().unwrap_or_default();
    let mut entries = Vec::new();
    let mut count = 0;
    for sym in &symbols {
        let kind = sym.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        if !matches!(kind, "class" | "function" | "interface" | "method" | "struct" | "enum" | "trait" | "impl") {
            continue;
        }
        if entries.len() >= 50 {
            count = symbols.len();
            break;
        }
        let name = sym.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let handle = sym.get("handle").and_then(|v| v.as_str()).unwrap_or(name);
        let sig = sym.get("signature").and_then(|v| v.as_str()).unwrap_or("");
        let start = sym.get("start_line").and_then(|v| v.as_u64()).unwrap_or(0);
        let end = sym.get("end_line").and_then(|v| v.as_u64()).unwrap_or(0);
        let display = if sig.is_empty() {
            format!("{} {}", kind, name)
        } else {
            sig.to_string()
        };
        entries.push(format!("  - [{}] {} (lines {}-{})", handle, display, start, end));
    }

    if entries.is_empty() {
        return String::new();
    }

    let mut map = format!("\nChunk map (full file):\n{}", entries.join("\n"));
    if count > 50 {
        map.push_str(&format!("\n  ... and {} more symbols. Use read --detail outline to see all.", count - 50));
    }
    map
}

/// Format outline from analyzer response into a readable chunk map.
/// Input: analyzer response data with symbols array.
pub fn format_outline(path: &str, data: &serde_json::Value, cache: &mut ReadFileCache) -> serde_json::Value {
    let hash = content_hash(&data.to_string());
    cache.set_hash(path, "outline", None, hash.clone());

    let symbols = data.get("symbols").and_then(|s| s.as_array()).cloned().unwrap_or_default();
    let mut entries = Vec::new();
    for sym in &symbols {
        let name = sym.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let sig = sym.get("signature").and_then(|v| v.as_str()).unwrap_or("");
        let text = sym.get("text").and_then(|v| v.as_str()).unwrap_or(name);
        let start = sym.get("start_line").and_then(|v| v.as_u64()).unwrap_or(0);
        let end = sym.get("end_line").and_then(|v| v.as_u64()).unwrap_or(0);
        let handle = sym.get("handle").and_then(|v| v.as_str()).unwrap_or(name);

        let display = if !sig.is_empty() {
            sig.to_string()
        } else {
            text.trim().to_string()
        };
        entries.push(format!(
            "  - [{}] {} (lines {}-{})",
            handle, display, start, end,
        ));
    }

    let output = if entries.is_empty() {
        format!("[File Hash: {}]\nNo symbols found.", hash)
    } else {
        format!("[File Hash: {}]\n{}", hash, entries.join("\n"))
    };

    serde_json::Value::String(output)
}

/// Format skeleton from analyzer response.
pub fn format_skeleton(path: &str, data: &serde_json::Value, cache: &mut ReadFileCache) -> serde_json::Value {
    let data_str = data.to_string();
    let hash = content_hash(&data_str);
    cache.set_hash(path, "skeleton", None, hash.clone());

    let skeleton = data.get("skeleton").and_then(|v| v.as_str())
        .unwrap_or(&data_str);

    let output = format!("[File Hash: {}]\n{}", hash, skeleton);
    serde_json::Value::String(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_file_formatting() {
        let content = "fn main() {\n    println!(\"hello\");\n}\n";
        let mut cache = ReadFileCache::new();
        let (output, _hash, unchanged) = format_full("test.rs", content, None, &mut cache);
        assert!(!unchanged);
        assert!(output.contains("[File Hash:"));
        assert!(output.contains("1 │"));
        assert!(output.contains("fn main()"));
    }

    #[test]
    fn unchanged_detection() {
        let content = "line 1\nline 2\n";
        let mut cache = ReadFileCache::new();
        let (_, _, unchanged1) = format_full("test.rs", content, None, &mut cache);
        assert!(!unchanged1);
        let (_, _, unchanged2) = format_full("test.rs", content, None, &mut cache);
        assert!(unchanged2);
    }

    #[test]
    fn range_formatting() {
        let content = "a\nb\nc\nd\ne\n";
        let mut cache = ReadFileCache::new();
        let (output, _, unchanged) = format_full("test.txt", content, Some((2, 4)), &mut cache);
        assert!(!unchanged);
        assert!(output.contains("[Lines 2-4,"));
        assert!(output.contains("2 │"));
    }

    #[test]
    fn preview_formatting() {
        let content: String = (0..500).map(|i| format!("line {}\n", i)).collect();
        let mut cache = ReadFileCache::new();
        let (output, _, unchanged) = format_preview("big.txt", &content, 1, &mut cache);
        assert!(!unchanged);
        assert!(output.contains("Showing first 200 lines"));
        assert!(!output.contains("Extended preview"));
    }

    #[test]
    fn preview_auto_expands() {
        let content: String = (0..600).map(|i| format!("line {}\n", i)).collect();
        let mut cache = ReadFileCache::new();
        let (output, _, _) = format_preview("big.txt", &content, 5, &mut cache);
        assert!(output.contains("Showing first 500 lines"));
        assert!(output.contains("Extended preview"));
    }

    #[test]
    fn auto_detail_selection() {
        assert_eq!(auto_detail(100, None, false, false, false), "full");
        assert_eq!(auto_detail(100_000, None, false, false, false), "preview");
        assert_eq!(auto_detail(100_000, Some("outline"), false, false, false), "outline");
        // With range/page/section, always full regardless of size
        assert_eq!(auto_detail(100_000, None, true, false, false), "full");
        assert_eq!(auto_detail(100_000, None, false, true, false), "full");
        assert_eq!(auto_detail(100_000, None, false, false, true), "full");
    }

}
