use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[cfg(test)]
mod compaction_consts {
    pub const COMPACTION_THRESHOLDS: &[(&str, usize)] = &[
        ("bash", 500),
        ("read", 1500),
        ("search", 800),
        ("repo", 1000),
        ("symbols", 1000),
    ];
    pub const DEFAULT_THRESHOLD: usize = 500;
    pub const MAX_IMPORTANT_LINES: usize = 8;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub tool_name: String,
    pub full_output: String,
    pub digest: String,
    pub important_lines: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub token_estimate: usize,
    pub content_hash: String,
}

pub struct ArtifactStore {
    artifacts: HashMap<String, Artifact>,
    #[cfg(test)]
    counter: u64,
}

impl ArtifactStore {
    pub fn new() -> Self {
        Self {
            artifacts: HashMap::new(),
            #[cfg(test)]
            counter: 0,
        }
    }

    pub fn get(&self, artifact_ref: &str) -> Option<&Artifact> {
        self.artifacts.get(artifact_ref)
    }

    pub fn gc_unreferenced(&mut self, live_refs: &HashSet<String>) {
        self.artifacts.retain(|id, _| {
            live_refs.contains(id)
        });
    }
}

#[cfg(test)]
impl ArtifactStore {
    fn next_id(&mut self, tool_name: &str) -> String {
        use compaction_consts::*; // ensure cfg(test) context
        self.counter += 1;
        format!("tool/{}/{}", tool_name, self.counter)
    }

    pub fn maybe_compact(
        &mut self,
        tool_name: &str,
        result: &serde_json::Value,
        estimated_tokens: usize,
    ) -> Option<(String, String)> {
        use compaction_consts::*;
        let threshold = COMPACTION_THRESHOLDS.iter()
            .find(|(t, _)| *t == tool_name)
            .map(|(_, t)| *t)
            .unwrap_or(DEFAULT_THRESHOLD);

        if estimated_tokens < threshold {
            return None;
        }

        let output_str = crate::util::secrets::redact_secrets(&result.to_string());
        let id = self.next_id(tool_name);
        let (digest, important_lines) = build_digest(tool_name, &output_str, &id);

        let content_hash = crate::util::stable_hash(output_str.as_bytes());

        let artifact = Artifact {
            id: id.clone(),
            tool_name: tool_name.to_string(),
            full_output: output_str,
            digest: digest.clone(),
            important_lines: important_lines.clone(),
            created_at: Utc::now(),
            token_estimate: estimated_tokens,
            content_hash,
        };

        self.artifacts.insert(id.clone(), artifact);
        Some((digest, id))
    }
}

/// Extract artifact:// references from a text string.
pub fn extract_artifact_refs(text: &str) -> HashSet<String> {
    let re = regex::Regex::new(r"artifact://([\w./-]+)").unwrap();
    re.captures_iter(text)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .collect()
}

/// Dispatch to per-tool digest builders.
#[cfg(test)]
fn build_digest(tool_name: &str, output: &str, artifact_id: &str) -> (String, Vec<String>) {
    match tool_name {
        "bash" => build_bash_digest(output, artifact_id),
        "read" => build_read_digest(output, artifact_id),
        "search" => build_search_digest(output, artifact_id),
        "symbols" => build_symbols_digest(output, artifact_id),
        "repo" => build_repo_digest(output, artifact_id),
        _ => build_generic_digest(output, artifact_id),
    }
}

#[cfg(test)]
fn build_bash_digest(output: &str, artifact_id: &str) -> (String, Vec<String>) {
    use compaction_consts::MAX_IMPORTANT_LINES;
    let parsed: Option<serde_json::Value> = serde_json::from_str(output).ok();
    let exit_code = parsed.as_ref()
        .and_then(|v| v.get("exit_code"))
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    let stdout = parsed.as_ref()
        .and_then(|v| v.get("stdout"))
        .and_then(|v| v.as_str())
        .unwrap_or(output);
    let stderr = parsed.as_ref()
        .and_then(|v| v.get("stderr"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let status = if exit_code == 0 { "success".to_string() } else { format!("exit_code={}", exit_code) };
    let mut important = Vec::new();

    if !stderr.is_empty() {
        for line in stderr.lines() {
            let lower = line.to_lowercase();
            if lower.contains("error") || lower.contains("failed") || lower.contains("fatal") || lower.contains("panic") {
                important.push(truncate_line(line, 120));
                if important.len() >= MAX_IMPORTANT_LINES { break; }
            }
        }
    }

    for line in stdout.lines() {
        if important.len() >= MAX_IMPORTANT_LINES { break; }
        let lower = line.to_lowercase();
        if lower.contains("failed") || lower.contains("fail:") || lower.contains("error:") {
            let t = truncate_line(line, 120);
            if !important.iter().any(|l| l == &t) { important.push(t); }
        }
    }

    let stdout_lines: Vec<&str> = stdout.lines().rev().filter(|l| !l.trim().is_empty()).take(4).collect();
    for line in stdout_lines.into_iter().rev() {
        if important.len() >= MAX_IMPORTANT_LINES { break; }
        let t = truncate_line(line, 120);
        if !important.iter().any(|l| l == &t) { important.push(t); }
    }

    let tok = output.len() / 4;
    let aref = format!("artifact://{}", artifact_id);
    let digest = format!(
        "Tool result: bash ({} tokens, compacted)\nStatus: {}\nImportant lines:\n{}\nFull output: {}",
        tok, status,
        important.iter().map(|l| format!("- {}", l)).collect::<Vec<_>>().join("\n"),
        aref,
    );
    (digest, important)
}

#[cfg(test)]
fn build_read_digest(output: &str, artifact_id: &str) -> (String, Vec<String>) {
    use compaction_consts::MAX_IMPORTANT_LINES;
    let parsed: Option<serde_json::Value> = serde_json::from_str(output).ok();
    let fpath = parsed.as_ref()
        .and_then(|v| v.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let content = parsed.as_ref()
        .and_then(|v| v.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or(output);

    let mut status = format!("file: {}", fpath);
    if let Some(range) = parsed.as_ref().and_then(|v| v.get("range")).and_then(|v| v.as_str()) {
        status.push_str(&format!(" range: {}", range));
    }

    let mut important = Vec::new();
    let sig_prefixes = ["fn ", "struct ", "impl ", "class ", "def ", "pub fn ", "pub async fn ", "const ", "type ", "enum ", "trait ", "interface "];
    for line in content.lines() {
        if important.len() >= MAX_IMPORTANT_LINES { break; }
        let trimmed = line.trim();
        if sig_prefixes.iter().any(|p| trimmed.starts_with(p)) {
            important.push(truncate_line(trimmed, 120));
        }
    }

    if important.is_empty() {
        for line in content.lines().take(MAX_IMPORTANT_LINES) {
            if !line.trim().is_empty() { important.push(truncate_line(line, 120)); }
        }
    }

    let tok = output.len() / 4;
    let aref = format!("artifact://{}", artifact_id);
    let digest = format!(
        "Tool result: read ({} tokens, compacted)\nStatus: {}\nSymbols:\n{}\nFull output: {}",
        tok, status,
        important.iter().map(|l| format!("- {}", l)).collect::<Vec<_>>().join("\n"),
        aref,
    );
    (digest, important)
}

#[cfg(test)]
fn build_search_digest(output: &str, artifact_id: &str) -> (String, Vec<String>) {
    use compaction_consts::MAX_IMPORTANT_LINES;
    let parsed: Option<serde_json::Value> = serde_json::from_str(output).ok();
    let pattern = parsed.as_ref()
        .and_then(|v| v.get("pattern"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let results = parsed.as_ref()
        .and_then(|v| v.get("results"))
        .and_then(|v| v.as_array());

    let file_count = results.map(|r| r.len()).unwrap_or(0);
    let status = if file_count == 0 {
        format!("no matches for '{}'", pattern)
    } else {
        format!("pattern: '{}', {} files matched", pattern, file_count)
    };

    let mut important = Vec::new();
    if let Some(results) = results {
        for entry in results.iter() {
            if important.len() >= MAX_IMPORTANT_LINES { break; }
            if let Some(fpath) = entry.get("path").and_then(|v| v.as_str()) {
                important.push(fpath.to_string());
            }
            if let Some(matches) = entry.get("matches").and_then(|v| v.as_array()) {
                for m in matches.iter().take(2) {
                    if important.len() >= MAX_IMPORTANT_LINES { break; }
                    let line_num = m.get("line").and_then(|v| v.as_i64()).unwrap_or(0);
                    if let Some(text) = m.get("text").and_then(|v| v.as_str()) {
                        let fpath = entry.get("path").and_then(|v| v.as_str()).unwrap_or("?");
                        important.push(format!("{}:{}: {}", fpath, line_num, truncate_line(text, 80)));
                    }
                }
            }
        }
    }

    let tok = output.len() / 4;
    let aref = format!("artifact://{}", artifact_id);
    let digest = format!(
        "Tool result: search ({} tokens, compacted)\nStatus: {}\nMatches:\n{}\nFull output: {}",
        tok, status,
        important.iter().map(|l| format!("- {}", l)).collect::<Vec<_>>().join("\n"),
        aref,
    );
    (digest, important)
}

#[cfg(test)]
fn build_symbols_digest(output: &str, artifact_id: &str) -> (String, Vec<String>) {
    use compaction_consts::MAX_IMPORTANT_LINES;
    let parsed: Option<serde_json::Value> = serde_json::from_str(output).ok();
    let subcmd = parsed.as_ref()
        .and_then(|v| v.get("subcommand"))
        .and_then(|v| v.as_str())
        .unwrap_or("search");

    let symbols = parsed.as_ref()
        .and_then(|v| v.get("symbols"))
        .and_then(|v| v.as_array());

    let count = symbols.map(|s| s.len()).unwrap_or(0);
    let status = format!("subcommand: {}, {} symbols found", subcmd, count);

    let mut important = Vec::new();
    if let Some(symbols) = symbols {
        for sym in symbols.iter() {
            if important.len() >= MAX_IMPORTANT_LINES { break; }
            let name = sym.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let kind = sym.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let loc = sym.get("location").and_then(|v| v.as_str())
                .or_else(|| sym.get("file").and_then(|v| v.as_str()))
                .unwrap_or("");
            important.push(format!("{} [{}] at {}", name, kind, loc));
        }
    }

    let tok = output.len() / 4;
    let aref = format!("artifact://{}", artifact_id);
    let digest = format!(
        "Tool result: symbols ({} tokens, compacted)\nStatus: {}\nSymbols:\n{}\nFull output: {}",
        tok, status,
        important.iter().map(|l| format!("- {}", l)).collect::<Vec<_>>().join("\n"),
        aref,
    );
    (digest, important)
}

#[cfg(test)]
fn build_repo_digest(output: &str, artifact_id: &str) -> (String, Vec<String>) {
    use compaction_consts::MAX_IMPORTANT_LINES;
    let parsed: Option<serde_json::Value> = serde_json::from_str(output).ok();
    let dir_path = parsed.as_ref()
        .and_then(|v| v.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or(".");

    let entries = parsed.as_ref()
        .and_then(|v| v.get("entries"))
        .and_then(|v| v.as_array());

    let count = entries.map(|e| e.len()).unwrap_or(0);
    let status = format!("directory: {}, {} entries", dir_path, count);

    let mut important = Vec::new();
    let mut dirs = Vec::new();
    let mut src_dirs = Vec::new();

    if let Some(entries) = entries {
        for entry in entries.iter() {
            let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let is_dir = entry.get("type").map(|v| v.as_str() == Some("dir")).unwrap_or(name.ends_with('/'));
            if is_dir {
                dirs.push(name.to_string());
                if ["src", "lib", "pkg", "cmd", "internal", "app", "server", "client"].contains(&name.trim_end_matches('/')) {
                    src_dirs.push(name.to_string());
                }
            }
        }
        for d in &src_dirs {
            if important.len() >= MAX_IMPORTANT_LINES { break; }
            important.push(format!("{}/ (source)", d.trim_end_matches('/')));
        }
        for d in &dirs {
            if important.len() >= MAX_IMPORTANT_LINES { break; }
            if !src_dirs.contains(d) {
                important.push(format!("{}/", d.trim_end_matches('/')));
            }
        }
        for entry in entries.iter() {
            if important.len() >= MAX_IMPORTANT_LINES { break; }
            let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let is_dir = entry.get("type").map(|v| v.as_str() == Some("dir")).unwrap_or(name.ends_with('/'));
            if !is_dir { important.push(name.to_string()); }
        }
    }

    let tok = output.len() / 4;
    let aref = format!("artifact://{}", artifact_id);
    let digest = format!(
        "Tool result: repo ({} tokens, compacted)\nStatus: {}\nContents:\n{}\nFull output: {}",
        tok, status,
        important.iter().map(|l| format!("- {}", l)).collect::<Vec<_>>().join("\n"),
        aref,
    );
    (digest, important)
}

#[cfg(test)]
fn build_generic_digest(output: &str, artifact_id: &str) -> (String, Vec<String>) {
    use compaction_consts::MAX_IMPORTANT_LINES;
    let lines: Vec<&str> = output.lines().collect();
    let status = lines.iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| truncate_line(l, 120))
        .unwrap_or_else(|| "unknown".to_string());
    let important: Vec<String> = lines.iter()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(MAX_IMPORTANT_LINES)
        .map(|l| truncate_line(l, 120))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let tok = output.len() / 4;
    let aref = format!("artifact://{}", artifact_id);
    let mut parts = vec![
        format!("Tool result: generic ({} tokens, compacted)", tok),
        format!("Status: {}", status),
        "Important lines:".to_string(),
    ];
    for line in &important { parts.push(format!("- {}", line)); }
    parts.push(format!("Full output: {}", aref));
    (parts.join("\n"), important)
}

#[cfg(test)]
fn truncate_line(line: &str, max_len: usize) -> String {
    if line.len() <= max_len { line.to_string() } else { format!("{}...", &line[..max_len - 3]) }
}

/// Collect live artifact references from checkpoint, recent messages,
/// inline references, and messages that reference critical file paths.
pub fn collect_live_refs(
    checkpoint: Option<&crate::context::distiller::schemas::Checkpoint>,
    messages: &[crate::agent::trajectory::Message],
    recent_count: usize,
    critical_file_paths: &HashSet<String>,
) -> HashSet<String> {
    let mut live_refs = HashSet::new();

    if let Some(cp) = checkpoint {
        live_refs.extend(cp.artifact_refs.iter().cloned());
    }

    for msg in messages.iter().rev().take(recent_count) {
        if let Some(ref id) = msg.tool_meta.artifact_ref {
            live_refs.insert(id.clone());
        }
        live_refs.extend(extract_artifact_refs(&msg.content.to_string()));
    }

    for msg in messages {
        let overlaps_critical = msg.tool_meta.paths_read.iter()
            .chain(msg.tool_meta.paths_written.iter())
            .any(|p| critical_file_paths.contains(p));
        if overlaps_critical {
            if let Some(ref id) = msg.tool_meta.artifact_ref {
                live_refs.insert(id.clone());
            }
        }
    }

    live_refs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::trajectory::{Message, Role, ToolMessageMeta};
    use chrono::Utc;

    #[test]
    fn checkpoint_artifact_refs_survive_gc() {
        let mut store = ArtifactStore::new();
        let result = serde_json::json!({"content": "x".repeat(5000)});
        let (_, art_id) = store.maybe_compact("bash", &result, 5000).unwrap();

        let mut live_refs = HashSet::new();
        live_refs.insert(art_id.clone());
        store.gc_unreferenced(&live_refs);

        assert!(store.get(&art_id).is_some());
    }

    #[test]
    fn unreferenced_artifact_is_collected() {
        let mut store = ArtifactStore::new();
        let result = serde_json::json!({"content": "x".repeat(5000)});
        let (_, art_id) = store.maybe_compact("bash", &result, 5000).unwrap();

        let empty_refs: HashSet<String> = HashSet::new();
        store.gc_unreferenced(&empty_refs);

        assert!(store.get(&art_id).is_none());
    }

    fn make_tool_message(tool_name: &str, artifact_ref: Option<String>, paths_read: Vec<&str>) -> Message {
        Message {
            id: uuid::Uuid::new_v4(),
            role: Role::Tool,
            content: serde_json::json!("output"),
            timestamp: Utc::now(),
            tokens: 100,
            is_compressed: false,
            tool_meta: ToolMessageMeta {
                tool_name: tool_name.to_string(),
                paths_read: paths_read.into_iter().map(|s| s.to_string()).collect(),
                paths_written: Vec::new(),
                is_compacted: false,
                artifact_ref,
            },
            tool_calls: Vec::new(),
            tool_call_id: None,
            thinking: None,
        }
    }

    #[test]
    fn collect_live_refs_preserves_checkpoint_refs() {
        let cp = crate::context::distiller::schemas::Checkpoint {
            progress_summary: "test".into(),
            completed: Vec::new(),
            remaining: Vec::new(),
            risks: Vec::new(),
            modified_files: Vec::new(),
            artifact_refs: vec!["tool/bash/1".into()],
            latest_failures: Vec::new(),
            decisions: Vec::new(),
            abandoned_approaches: Vec::new(),
            thematic_tags: Vec::new(),
            source_event_range: None,
        };
        let live = collect_live_refs(Some(&cp), &[], 10, &HashSet::new());
        assert!(live.contains("tool/bash/1"));
    }

    #[test]
    fn collect_live_refs_preserves_recent_message_refs() {
        let msgs = vec![
            make_tool_message("bash", Some("tool/bash/5".into()), vec![]),
            make_tool_message("read", Some("tool/read/6".into()), vec![]),
        ];
        let live = collect_live_refs(None, &msgs, 10, &HashSet::new());
        assert!(live.contains("tool/bash/5"));
        assert!(live.contains("tool/read/6"));
    }

    #[test]
    fn collect_live_refs_preserves_critical_file_artifacts() {
        let msgs = vec![
            make_tool_message("read", Some("tool/read/1".into()), vec!["src/important.rs"]),
            make_tool_message("read", Some("tool/read/2".into()), vec!["src/other.rs"]),
        ];
        let mut critical = HashSet::new();
        critical.insert("src/important.rs".to_string());

        let live = collect_live_refs(None, &msgs, 0, &critical);
        assert!(live.contains("tool/read/1"), "artifact for critical file must survive");
        assert!(!live.contains("tool/read/2"), "artifact for non-critical file not in recent window");
    }

    #[test]
    fn collect_live_refs_preserves_inline_refs() {
        let msg = Message {
            id: uuid::Uuid::new_v4(),
            role: Role::Assistant,
            content: serde_json::json!("See artifact://tool/bash/3 for details"),
            timestamp: Utc::now(),
            tokens: 50,
            is_compressed: false,
            tool_meta: ToolMessageMeta {
                tool_name: String::new(),
                paths_read: Vec::new(),
                paths_written: Vec::new(),
                is_compacted: false,
                artifact_ref: None,
            },
            tool_calls: Vec::new(),
            tool_call_id: None,
            thinking: None,
        };
        let live = collect_live_refs(None, &[msg], 10, &HashSet::new());
        assert!(live.contains("tool/bash/3"));
    }
}
