use std::collections::HashMap;

/// Per-session cache for read file results: tracks content hashes for
/// unchanged detection and cursor positions for pagination.
pub struct ReadFileCache {
    hashes: HashMap<String, String>,
    #[allow(dead_code)]
    cursors: HashMap<String, usize>,
}

impl ReadFileCache {
    pub fn new() -> Self {
        Self {
            hashes: HashMap::new(),
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
        self.hashes.insert(key, hash);
    }

    #[allow(dead_code)]
    pub fn get_cursor(&self, path: &str) -> usize {
        *self.cursors.get(path).unwrap_or(&0)
    }

    #[allow(dead_code)]
    pub fn set_cursor(&mut self, path: &str, line: usize) {
        self.cursors.insert(path.to_string(), line);
    }
}

const MAX_FILE_READ_SIZE: usize = 50 * 1024;
const DEFAULT_PREVIEW_LINES: usize = 200;
const AUTO_EXPAND_PREVIEW_LINES: usize = 500;
const AUTO_EXPAND_READ_THRESHOLD: usize = 3;

/// 8-char content hash for unchanged detection.
fn content_hash(content: &str) -> String {
    crate::util::stable_hash(content.as_bytes())[..8].to_string()
}

/// 3-char line hash for gutter anchors.
fn line_hash(line: &str) -> String {
    crate::util::fast_hash(line.as_bytes())[..3].to_string()
}

/// Width for line number column based on total lines.
fn line_num_width(total_lines: usize) -> usize {
    let digits = if total_lines == 0 { 1 } else { (total_lines as f64).log10().floor() as usize + 1 };
    digits.max(3)
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
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    if let Some((start, end)) = range {
        let start = start.max(1);
        let end = end.min(total_lines);
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
            let lh = line_hash(line);
            format_anchored_line(line, ln, num_width, &lh)
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
            let lh = line_hash(line);
            format_anchored_line(line, i + 1, num_width, &lh)
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
    let lines: Vec<&str> = content.lines().collect();
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

    let num_width = line_num_width(total_lines);
    let anchored: Vec<String> = slice.iter().enumerate().map(|(i, line)| {
        let lh = line_hash(line);
        format_anchored_line(line, i + 1, num_width, &lh)
    }).collect();

    let size_kb = content.len() / 1024;
    let expand_note = if read_count >= AUTO_EXPAND_READ_THRESHOLD {
        "\nNOTE: Extended preview shown due to multiple reads."
    } else {
        ""
    };

    let note = format!(
        "\n\nNOTE: File '{}' is {} KB (~{} lines). Showing first {} lines.{}",
        path, size_kb, total_lines, take, expand_note,
    );

    let output = format!("[File Hash: {}]\n{}{}", hash, anchored.join("\n"), note);
    (output, hash, false)
}

/// Choose the detail level automatically based on file size.
/// Files > 50KB default to preview, everything else to full.
pub fn auto_detail(file_size: usize, requested: Option<&str>) -> String {
    if let Some(d) = requested {
        d.to_string()
    } else if file_size > MAX_FILE_READ_SIZE {
        "preview".to_string()
    } else {
        "full".to_string()
    }
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
        let kind = sym.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let sig = sym.get("signature").and_then(|v| v.as_str()).unwrap_or("");
        let start = sym.get("start_line").and_then(|v| v.as_u64()).unwrap_or(0);
        let end = sym.get("end_line").and_then(|v| v.as_u64()).unwrap_or(0);
        let handle = sym.get("handle").and_then(|v| v.as_str()).unwrap_or(name);

        entries.push(format!(
            "  - [{}] {} (lines {}-{})",
            handle,
            if sig.is_empty() { format!("{} {}", kind, name) } else { sig.to_string() },
            start, end,
        ));
    }

    let output = if entries.is_empty() {
        format!("[File Hash: {}]\nNo symbols found.", hash)
    } else {
        format!("[File Hash: {}]\n{}\n{} symbols total", hash, entries.join("\n"), entries.len())
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
        let (output, hash, unchanged) = format_full("test.rs", content, None, &mut cache);
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
        assert_eq!(auto_detail(100, None), "full");
        assert_eq!(auto_detail(100_000, None), "preview");
        assert_eq!(auto_detail(100_000, Some("outline")), "outline");
    }

}
