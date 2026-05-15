pub mod cli_parse;
pub mod list_files;
pub mod search_files;
pub mod edit_file;
pub mod ask_followup;
pub mod attempt_completion;
pub mod approval;
pub mod symbols;
pub mod tool_defs;
pub mod response;
pub mod routing;
pub mod format;
pub mod output_manager;
pub mod read_file;

use crate::daemons::{AnalyzerRequest, AnalyzerResponse, ExecuteResult, ResilientDaemon};
use response::{ToolResponse, ToolErrorCode};
use std::collections::HashMap;
use std::sync::LazyLock;

const MAX_BACKGROUND_JOBS: usize = 128;

/// Stores results of bash commands executed with --await for later retrieval.
/// Bounded at MAX_BACKGROUND_JOBS; oldest entry evicted on insert when full.
static BACKGROUND_JOBS: LazyLock<std::sync::Mutex<BackgroundJobStore>> =
    LazyLock::new(|| std::sync::Mutex::new(BackgroundJobStore::new()));

struct BackgroundJobStore {
    jobs: HashMap<String, ExecuteResult>,
    order: Vec<String>,
}

impl BackgroundJobStore {
    fn new() -> Self {
        Self { jobs: HashMap::new(), order: Vec::new() }
    }

    fn get(&self, id: &str) -> Option<&ExecuteResult> {
        self.jobs.get(id)
    }

    fn insert(&mut self, id: String, result: ExecuteResult) {
        if self.jobs.len() >= MAX_BACKGROUND_JOBS {
            if let Some(old) = self.order.first().cloned() {
                self.order.remove(0);
                self.jobs.remove(&old);
            }
        }
        if !self.jobs.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.jobs.insert(id, result);
    }
}

use routing::{ErrorRouter, RoutingContext, ToolErrorRoute};
use format::{format_error_for_llm, format_error_for_log};
use serde::{Deserialize, Serialize};
use anyhow::{Result, anyhow};
use std::sync::Arc;
use serde_json::json;

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub args: serde_json::Value,
}

/// Read-only tools whose results can be cached across turns.
const CACHEABLE_TOOLS: &[&str] = &["read", "search", "repo", "symbols"];

/// Meta-keys stripped from cache key computation.
const CACHE_STRIP_KEYS: &[&str] = &["clarify", "retry", "command", "call_id", "autoCorrect", "dryRun", "verify"];

// ---------------------------------------------------------------------------
// Tool Coordinator (caching, retry, auto-correction)
// ---------------------------------------------------------------------------

/// Cached tool result with freshness metadata for staleness detection.
struct CacheEntry {
    result: String,
    mtime: Option<std::time::SystemTime>,
    size: Option<u64>,
    /// blake3 hash of first+last 4KB of file content (when applicable).
    hash_prefix: Option<u64>,
}

pub struct ToolCoordinator {
    cache: HashMap<String, CacheEntry>,
    router: ErrorRouter,
}

/// Maximum number of cache entries to prevent unbounded memory growth.
const MAX_CACHE_ENTRIES: usize = 256;

impl ToolCoordinator {
    pub fn new() -> Self {
        Self { cache: HashMap::new(), router: ErrorRouter::new() }
    }

    /// Invalidate all cached results that reference a specific file path.
    pub fn invalidate_for_path(&mut self, path: &str) {
        self.cache.retain(|key, _| !key.contains(path));
    }

    /// Invalidate all cached search and repo results (structural changes).
    pub fn invalidate_search_and_repo(&mut self) {
        self.cache.retain(|key, _| !key.starts_with("search:") && !key.starts_with("repo:"));
    }

    /// Reset retry counters (e.g., after context compaction).
    pub fn reset_router(&mut self) {
        self.router.reset();
    }

    /// Compute freshness metadata for a file (mtime, size, partial hash).
    /// Used to detect external modifications in the read cache.
    fn file_metadata(path: &std::path::Path) -> Option<(std::time::SystemTime, u64, u64)> {
        let meta = std::fs::metadata(path).ok()?;
        let mtime = meta.modified().ok()?;
        let size = meta.len();
        // Quick partial hash of first and last 4KB
        let mut buf = Vec::with_capacity(8192);
        let mut file = std::fs::File::open(path).ok()?;
        use std::io::Read;
        let mut head = [0u8; 4096];
        let head_len = file.read(&mut head).unwrap_or(0);
        let tail_offset = size.saturating_sub(4096);
        let mut tail = [0u8; 4096];
        let tail_len = if tail_offset > 0 {
            let _ = std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(tail_offset));
            file.read(&mut tail).unwrap_or(0)
        } else {
            0
        };
        buf.extend_from_slice(&head[..head_len]);
        buf.extend_from_slice(&tail[..tail_len]);
        let hash = blake3::hash(&buf);
        // Use the first 8 bytes as a u64 prefix
        let prefix = u64::from_le_bytes(hash.as_bytes()[..8].try_into().unwrap());
        Some((mtime, size, prefix))
    }

    pub async fn execute_with_coordination(
        &mut self,
        call: &ToolCall,
        executor: &ToolExecutor,
    ) -> Result<serde_json::Value> {
        let is_cacheable = CACHEABLE_TOOLS.contains(&call.name.as_str());
        let cache_key = self.make_cache_key(call);

        // 1. Cache check with freshness validation for file-based tools
        if is_cacheable {
            if let Some(entry) = self.cache.get(&cache_key) {
                let mut fresh = true;
                if call.name == "read" {
                    if let Some(path_str) = call.args.get("path").and_then(|v| v.as_str()) {
                        if let Some((mtime, size, hash)) = Self::file_metadata(std::path::Path::new(path_str)) {
                            fresh = entry.mtime == Some(mtime) && entry.size == Some(size)
                                || entry.hash_prefix == Some(hash);
                        }
                    }
                }
                if fresh {
                    let cached_val: serde_json::Value = serde_json::from_str(&entry.result)
                        .unwrap_or(serde_json::Value::String(entry.result.clone()));
                    let mut result = cached_val;
                    if let Some(obj) = result.as_object_mut() {
                        obj.insert("_cached".to_string(), serde_json::Value::Bool(true));
                    } else {
                        result = serde_json::json!({
                            "_cached": true,
                            "value": result,
                        });
                    }
                    return Ok(result);
                }
            }
        }

        // 2. Execute and route errors
        let user_max_retries = call.args.get("retry")
            .and_then(|v| v.as_u64())
            .map(|n| std::cmp::min(n as usize, 5))
            .unwrap_or(0);

        // Compute input hash from normalized args for same-input guard
        let input_hash = crate::util::fast_hash(cache_key.as_bytes());

        let mut response = executor.execute_raw(call).await;
        let mut retry_count = 0;
        let total_budget = std::cmp::max(user_max_retries, 2);

        while let ToolResponse::Failure { ref mut error, .. } = response {
            // Stamp input hash onto error for same-input guard in router
            error.metadata.input_hash = Some(input_hash.clone());

            if retry_count >= total_budget {
                break;
            }

            let ctx = RoutingContext {
                retry_count_for_error: retry_count,
            };

            let route = self.router.route(error, &ctx);
            eprintln!("[di-core] tool error routed: {} → {:?}", error.code.as_str(), route);

            match route {
                ToolErrorRoute::Retry { backoff_ms, reason, .. } => {
                    eprintln!("[di-core] retrying tool {} (attempt {}): {}", call.name, retry_count + 1, reason);
                    tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                    response = executor.execute_raw(call).await;
                    retry_count += 1;
                }
                ToolErrorRoute::Continue { reason } => {
                    eprintln!("[di-core] continuing after error: {}", reason);
                    break;
                }
                ToolErrorRoute::Abort { reason } => {
                    eprintln!("[di-core] aborting tool {}: {}", call.name, reason);
                    break;
                }
                ToolErrorRoute::Escalate { reason } => {
                    eprintln!("[di-core] escalating tool error: {}", reason);
                    break;
                }
            }
        }

        // 3. Convert ToolResponse back to Result<Value> for backward compat
        let value = match response {
            ToolResponse::Success { data, .. } => data,
            ToolResponse::Failure { error, .. } => {
                let llm_msg = format_error_for_llm(&error);
                eprintln!("[di-core] tool error for LLM: {}", format_error_for_log(&error));
                return Ok(json!({ "error": llm_msg, "status": "error", "code": error.code.as_str() }));
            }
        };

        // 4. Auto-correction for truncated output
        let value = if let Some(corrected) = self.auto_correct_truncated(call, &value, executor).await {
            corrected
        } else {
            value
        };

        // 4. Cache store with bounded eviction (skip error results and pre-format read blobs)
        if is_cacheable
            && value.get("status").and_then(|v| v.as_str()) != Some("error")
            && value.get("_read_raw").is_none()
        {
            if self.cache.len() >= MAX_CACHE_ENTRIES {
                // Remove oldest 25% to avoid unbounded growth
                let evict_count = MAX_CACHE_ENTRIES / 4;
                let keys: Vec<_> = self.cache.keys().take(evict_count).cloned().collect();
                for k in keys { self.cache.remove(&k); }
            }
            let (mtime, size, hash_prefix) = if call.name == "read" {
                if let Some(path_str) = call.args.get("path").and_then(|v| v.as_str()) {
                    Self::file_metadata(std::path::Path::new(path_str))
                        .map(|(mt, sz, hp)| (Some(mt), Some(sz), Some(hp)))
                        .unwrap_or((None, None, None))
                } else {
                    (None, None, None)
                }
            } else {
                (None, None, None)
            };
            let entry = CacheEntry {
                result: value.to_string(),
                mtime,
                size,
                hash_prefix,
            };
            self.cache.insert(cache_key, entry);
        }

        if retry_count > 0 {
            if let serde_json::Value::String(s) = &value {
                return Ok(serde_json::Value::String(format!("[Retry] {} attempts\n{}", retry_count, s)));
            }
        }

        Ok(value)
    }

    fn make_cache_key(&self, call: &ToolCall) -> String {
        let mut args = call.args.clone();
        if let serde_json::Value::Object(ref mut map) = args {
            for key in CACHE_STRIP_KEYS {
                map.remove(*key);
            }
            if let Some(path) = map.get_mut("path") {
                if let Some(s) = path.as_str() {
                    let normalized = s.trim_start_matches("./");
                    *path = serde_json::Value::String(normalized.to_string());
                }
            }
        }
        // Canonicalize arguments by sorting object keys for deterministic cache keys
        let canonical = Self::canonicalize_value(&args);
        format!("{}:{}", call.name, canonical)
    }

    /// Recursively sort JSON object keys for deterministic serialization.
    fn canonicalize_value(v: &serde_json::Value) -> serde_json::Value {
        match v {
            serde_json::Value::Object(map) => {
                let mut sorted = serde_json::Map::new();
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                for k in keys {
                    sorted.insert(k.clone(), Self::canonicalize_value(&map[k]));
                }
                serde_json::Value::Object(sorted)
            }
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(Self::canonicalize_value).collect())
            }
            _ => v.clone(),
        }
    }

    async fn auto_correct_truncated(
        &self,
        call: &ToolCall,
        result: &serde_json::Value,
        executor: &ToolExecutor,
    ) -> Option<serde_json::Value> {
        let s = result.to_string();
        if !s.contains("[truncated]") && !s.contains("[Content reduced") {
            return None;
        }

        let mut args = call.args.clone();
        let args_map = match args.as_object_mut() {
            Some(m) => m,
            None => return None,
        };

        match call.name.as_str() {
            "read" => {
                args_map.insert("detail".to_string(), json!("skeleton"));
            }
            "search" => {
                args_map.insert("context_lines".to_string(), json!(0));
            }
            _ => return None,
        }

        args_map.remove("retry");
        args_map.remove("command");
        let degraded = ToolCall { name: call.name.clone(), args };
        match executor.execute_raw(&degraded).await {
            ToolResponse::Success { data, .. } => {
                // Append note so LLM knows degradation occurred
                if let serde_json::Value::String(s) = &data {
                    Some(serde_json::Value::String(format!(
                        "{}\n[Auto-corrected: output was truncated, degraded params applied]",
                        s
                    )))
                } else {
                    Some(data)
                }
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tool Executor (raw dispatch)
// ---------------------------------------------------------------------------

pub struct ToolExecutor {
    analyzer_daemon: Arc<tokio::sync::Mutex<ResilientDaemon>>,
    command_daemon: Arc<tokio::sync::Mutex<ResilientDaemon>>,
    output_manager: Arc<std::sync::Mutex<output_manager::OutputManager>>,
}

impl ToolExecutor {
    pub fn new(
        analyzer_daemon: Arc<tokio::sync::Mutex<ResilientDaemon>>,
        command_daemon: Arc<tokio::sync::Mutex<ResilientDaemon>>,
        output_manager: Arc<std::sync::Mutex<output_manager::OutputManager>>,
    ) -> Self {
        Self {
            analyzer_daemon,
            command_daemon,
            output_manager,
        }
    }

    pub fn analyzer_daemon(&self) -> &Arc<tokio::sync::Mutex<ResilientDaemon>> {
        &self.analyzer_daemon
    }

    pub async fn execute(&self, call: &ToolCall, coordinator: &mut ToolCoordinator) -> Result<serde_json::Value> {
        coordinator.execute_with_coordination(call, self).await
    }

    /// Raw tool dispatch. Returns structured ToolResponse.
    /// Wire names match v9.5.1 spec.
    pub async fn execute_raw(&self, call: &ToolCall) -> ToolResponse {
        let name = call.name.as_str();

        // Parse CLI-style command string into structured args
        let parsed_args = cli_parse::parse_command_args(name, &call.args);
        let parsed_call = ToolCall { name: call.name.clone(), args: parsed_args };

        match name {
            "read" => ToolResponse::from_result(self.read_file(&parsed_call).await, name),
            "write" => ToolResponse::from_result(self.write_file(&parsed_call).await, name),
            "edit" => edit_file::edit_file(&self.command_daemon, &parsed_call).await,
            "search" => search_files::search_files(&self.command_daemon, &parsed_call).await,
            "repo" => list_files::list_files(&self.analyzer_daemon, &parsed_call).await,
            "bash" => ToolResponse::from_result(self.bash(&parsed_call).await, name),
            "compact" => ToolResponse::from_result(self.compact(&parsed_call).await, name),
            "ask" => {
                let result = ask_followup::parse_followup_question(&parsed_call)
                    .map(|(question, options)| json!({ "_frontend_action": "followup_question", "question": question, "options": options }));
                ToolResponse::from_result(result, name)
            }
            "done" => {
                let result = attempt_completion::parse_completion(&parsed_call)
                    .map(|(result, command)| json!({ "_frontend_action": "attempt_completion", "result": result, "command": command }));
                ToolResponse::from_result(result, name)
            }
            "symbols" => ToolResponse::from_result(symbols::symbols(&self.analyzer_daemon, &parsed_call).await, name),
            "plan" => {
                let plan = parsed_call.args.get("plan").and_then(|v| v.as_str())
                    .or_else(|| parsed_call.args.get("text").and_then(|v| v.as_str()))
                    .unwrap_or("");
                ToolResponse::ok(json!({ "_frontend_action": "plan_response", "plan": plan }))
            }
            "task" => {
                let task = parsed_call.args.get("task").and_then(|v| v.as_str())
                    .or_else(|| parsed_call.args.get("text").and_then(|v| v.as_str()))
                    .unwrap_or("");
                ToolResponse::ok(json!({ "_frontend_action": "new_task", "task": task }))
            }
            "tools" => {
                let tool_names = [
                    "read", "write", "edit", "search", "repo", "bash",
                    "compact", "ask", "done", "symbols", "plan", "task", "tools",
                    "memory",
                ];
                let list: Vec<&str> = if let Some(filter) = parsed_call.args.get("filter").and_then(|v| v.as_str()) {
                    tool_names.iter().filter(|t| t.contains(&filter.to_lowercase())).copied().collect()
                } else {
                    tool_names.to_vec()
                };
                ToolResponse::ok(json!({ "tools": list, "count": list.len() }))
            }
            "memory" => {
                let action = parsed_call.args.get("action").and_then(|v| v.as_str()).unwrap_or("list");
                match action {
                    "list" => {
                        let om = self.output_manager.lock().unwrap();
                        let outputs = om.list_outputs();
                        ToolResponse::ok(json!({ "outputs": outputs, "count": outputs.len() }))
                    }
                    "read" => {
                        let filename = match parsed_call.args.get("file").and_then(|v| v.as_str()) {
                            Some(f) => f.to_string(),
                            None => return ToolResponse::fail(ToolErrorCode::MissingArgument, "Missing file argument for memory read".to_string(), "memory"),
                        };
                        let path = {
                            let om = self.output_manager.lock().unwrap();
                            om.output_dir().join(&filename)
                        };
                        // Canonicalize to prevent path traversal (e.g. "../../etc/passwd")
                        let canonical = match path.canonicalize() {
                            Ok(p) => p,
                            Err(_) => return ToolResponse::fail(
                                ToolErrorCode::IoFileNotFound,
                                format!("File not found: {}", filename),
                                "memory",
                            ),
                        };
                        let output_dir_canonical = {
                            let om = self.output_manager.lock().unwrap();
                            om.output_dir().canonicalize().unwrap_or_else(|_| om.output_dir().to_path_buf())
                        };
                        if !canonical.starts_with(&output_dir_canonical) {
                            return ToolResponse::fail(
                                ToolErrorCode::InvalidInput,
                                "Access denied: path is outside the output directory".to_string(),
                                "memory",
                            );
                        }
                        match tokio::fs::read_to_string(&canonical).await {
                            Ok(content) => ToolResponse::ok(json!({
                                "file": filename,
                                "content": content,
                                "lines": read_file::count_lines(&content),
                            })),
                            Err(e) => ToolResponse::fail(
                                ToolErrorCode::IoFileNotFound,
                                format!("Failed to read {}: {}", filename, e),
                                "memory",
                            ),
                        }
                    }
                    "clear" => {
                        let paths: Vec<_> = {
                            let om = self.output_manager.lock().unwrap();
                            let outputs = om.list_outputs();
                            outputs.iter().map(|n| om.output_dir().join(n)).collect()
                        };
                        let mut removed = 0;
                        for path in &paths {
                            if path.exists() {
                                let _ = tokio::fs::remove_file(path).await;
                                removed += 1;
                            }
                        }
                        ToolResponse::ok(json!({ "cleared": removed }))
                    }
                    _ => ToolResponse::fail(
                        ToolErrorCode::InvalidInput,
                        format!("Unknown memory action: {}. Use list, read, or clear.", action),
                        "memory",
                    ),
                }
            }
            _ => ToolResponse::fail(ToolErrorCode::InvalidInput, format!("Unknown tool: {}", call.name), name),
        }
    }

    async fn read_file(&self, call: &ToolCall) -> Result<serde_json::Value> {
        // Multi-file support: accept `paths` array or single `path`
        let paths: Vec<String> = if let Some(paths_val) = call.args.get("paths") {
            match paths_val {
                serde_json::Value::Array(arr) => {
                    arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
                }
                serde_json::Value::String(s) => {
                    s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
                }
                _ => vec![],
            }
        } else if let Some(path) = call.args.get("path").and_then(|v| v.as_str()) {
            vec![path.to_string()]
        } else {
            return Err(anyhow!("Missing path argument"));
        };

        if paths.is_empty() {
            return Err(anyhow!("Missing path argument"));
        }

        // Single-file fast path (preserves existing behavior)
        if paths.len() == 1 {
            return self.read_single_file(&paths[0], call).await;
        }

        // Multi-file: process each and collect results
        let mut results = Vec::new();
        for p in &paths {
            match self.read_single_file(p, call).await {
                Ok(result) => results.push(result),
                Err(e) => {
                    results.push(json!({
                        "_read_raw": true,
                        "path": p,
                        "detail": "full",
                        "error": format!("Error reading file: {}", e)
                    }));
                }
            }
        }

        Ok(json!({ "_read_raw": true, "_multi_file": true, "results": results }))
    }

    async fn read_single_file(&self, path: &str, call: &ToolCall) -> Result<serde_json::Value> {
        // Handle artifact:// references — retrieve from store
        if let Some(artifact_id) = path.strip_prefix("artifact://") {
            return Err(anyhow!("Artifact references are no longer supported: {}", artifact_id));
        }

        let detail = call.args.get("detail").and_then(|v| v.as_str());
        let is_md = read_file::is_markdown(path);

        // Read raw content from disk
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("Failed to read {}: {}", path, e))?;
        let file_size = content.len();
        let total_lines = read_file::count_lines(&content);

        let has_range = call.args.get("range").is_some() || call.args.get("ranges").is_some();
        let has_page = call.args.get("page").is_some();
        let has_section = call.args.get("section").is_some();
        let effective_detail = read_file::auto_detail(file_size, detail, has_range, has_page, has_section);

        // Markdown outline/hint: skip analyzer, handle inline
        if is_md && matches!(effective_detail.as_str(), "outline" | "hint") {
            let md_output = match effective_detail.as_str() {
                "hint" => read_file::format_markdown_hint(&content),
                _ => read_file::format_markdown_outline(&content),
            };
            return Ok(json!({
                "_read_raw": true,
                "path": path,
                "detail": effective_detail,
                "_markdown": true,
                "md_output": md_output,
            }));
        }

        // Section jump: resolve symbol handle to line range
        if let Some(section) = call.args.get("section").and_then(|v| v.as_str()) {
            let resp: AnalyzerResponse = self.analyzer_daemon.lock().await.send_request_retry(AnalyzerRequest {
                command: "outline".to_string(),
                file: Some(path.to_string()),
                content: None,
                language: None,
                query: None,
                subcommand: None,
            }).await?;
            if resp.ok {
                if let Some(symbols) = resp.data.get("symbols").and_then(|s| s.as_array()) {
                    for sym in symbols {
                        let handle = sym.get("handle").and_then(|v| v.as_str()).unwrap_or("");
                        if handle == section {
                            // Analyzer returns 0-based line numbers; convert to 1-based
                            let start = sym.get("start_line").and_then(|v| v.as_u64()).unwrap_or(0) as usize + 1;
                            let end = sym.get("end_line").and_then(|v| v.as_u64()).unwrap_or(start as u64 - 1) as usize + 1;
                            return Ok(json!({
                                "_read_raw": true,
                                "path": path,
                                "detail": effective_detail,
                                "content": content,
                                "lines": total_lines,
                                "range": json!([start, end]),
                                "_section": section,
                            }));
                        }
                    }
                }
            }
            return Ok(json!({
                "_read_raw": true,
                "path": path,
                "detail": effective_detail,
                "content": content,
                "lines": total_lines,
                "_section_not_found": section,
            }));
        }

        // Analyzer-based detail levels
        match effective_detail.as_str() {
            "outline" | "hint" => {
                let resp: AnalyzerResponse = self.analyzer_daemon.lock().await.send_request_retry(AnalyzerRequest {
                    command: "outline".to_string(),
                    file: Some(path.to_string()),
                    content: None,
                    language: None,
                    query: None,
                    subcommand: None,
                }).await?;
                if resp.ok {
                    Ok(json!({ "_read_raw": true, "path": path, "detail": effective_detail, "analyzer_data": resp.data }))
                } else {
                    Err(anyhow!("Analyzer outline failed: {:?}", resp.data))
                }
            }
            "skeleton" => {
                let resp: AnalyzerResponse = self.analyzer_daemon.lock().await.send_request_retry(AnalyzerRequest {
                    command: "skeleton".to_string(),
                    file: Some(path.to_string()),
                    content: None,
                    language: None,
                    query: None,
                    subcommand: None,
                }).await?;
                if resp.ok {
                    Ok(json!({ "_read_raw": true, "path": path, "detail": "skeleton", "analyzer_data": resp.data }))
                } else {
                    Err(anyhow!("Analyzer skeleton failed: {:?}", resp.data))
                }
            }
            "preview" => {
                let range = parse_range(call);
                let ranges = call.args.get("ranges").and_then(|v| read_file::parse_ranges(v));
                let page = call.args.get("page").and_then(|v| v.as_str()).map(String::from);

                let analyzer_data: Option<serde_json::Value> = match self.analyzer_daemon.lock().await.send_request::<_, AnalyzerResponse>(AnalyzerRequest {
                    command: "outline".to_string(),
                    file: Some(path.to_string()),
                    content: None,
                    language: None,
                    query: None,
                    subcommand: None,
                }).await {
                    Ok(resp) if resp.ok => Some(resp.data),
                    _ => None,
                };

                Ok(json!({
                    "_read_raw": true,
                    "path": path,
                    "detail": "preview",
                    "content": content,
                    "lines": total_lines,
                    "range": range.map(|(s, e)| json!([s, e])),
                    "ranges": ranges.map(|r| r.iter().map(|(s, e)| json!([s, e])).collect::<Vec<_>>()),
                    "page": page,
                    "analyzer_data": analyzer_data,
                }))
            }
            _ => {
                let range = parse_range(call);
                let ranges = call.args.get("ranges").and_then(|v| read_file::parse_ranges(v));
                let page = call.args.get("page").and_then(|v| v.as_str()).map(String::from);

                // Fetch analyzer data for budget degradation cascade (full→skeleton/outline/hint)
                let mut analyzer_data: Option<serde_json::Value> = match self.analyzer_daemon.lock().await.send_request::<_, AnalyzerResponse>(AnalyzerRequest {
                    command: "outline".to_string(),
                    file: Some(path.to_string()),
                    content: None,
                    language: None,
                    query: None,
                    subcommand: None,
                }).await {
                    Ok(resp) if resp.ok => Some(resp.data),
                    _ => None,
                };

                // Also fetch skeleton data so budget degradation to "skeleton" level works correctly
                if let Some(ref mut data) = analyzer_data {
                    if data.get("skeleton").is_none() {
                        if let Ok(skel_resp) = self.analyzer_daemon.lock().await.send_request::<_, AnalyzerResponse>(AnalyzerRequest {
                            command: "skeleton".to_string(),
                            file: Some(path.to_string()),
                            content: None,
                            language: None,
                            query: None,
                            subcommand: None,
                        }).await {
                            if let Some(skel) = skel_resp.data.get("skeleton").and_then(|v| v.as_str()) {
                                if let Some(obj) = data.as_object_mut() {
                                    obj.insert("skeleton".to_string(), serde_json::json!(skel));
                                }
                            }
                        }
                    }
                }

                Ok(json!({
                    "_read_raw": true,
                    "path": path,
                    "detail": effective_detail,
                    "content": content,
                    "lines": total_lines,
                    "range": range.map(|(s, e)| json!([s, e])),
                    "ranges": ranges.map(|r| r.iter().map(|(s, e)| json!([s, e])).collect::<Vec<_>>()),
                    "page": page,
                    "analyzer_data": analyzer_data,
                }))
            }
        }
    }

    async fn write_file(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let path = call.args.get("path").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing path argument"))?;
        let raw_content = call.args.get("content").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing content argument"))?;

        // Strip wrapping markdown code fences (```lang\n...\n```)
        let content_stripped = strip_code_fences(raw_content);
        let content: &str = &content_stripped;

        // Security scanning: detect dangerous patterns and sensitive paths
        let security_violations = scan_write_security(path, content);

        let dry_run = call.args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);
        let line_count = read_file::count_lines(&content);

        if dry_run {
            let mut output = format!("Would write {} lines to {}.", line_count, path);
            if !security_violations.is_empty() {
                output.push_str(&format!("\n[SECURITY_CONSTRAINTS]\n{}",
                    serde_json::to_string(&security_violations).unwrap_or_default()));
            }
            return Ok(json!(output));
        }

        let create_dirs = call.args.get("create_dirs")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        if create_dirs {
            if let Some(parent) = std::path::Path::new(path).parent() {
                if !parent.as_os_str().is_empty() {
                    tokio::fs::create_dir_all(parent).await?;
                }
            }
        }

        tokio::fs::write(path, content).await?;

        let line_count = read_file::count_lines(content);
        let mut output = format!("Created: {} ({} lines)", path, line_count);
        if !security_violations.is_empty() {
            output.push_str(&format!("\n[SECURITY_CONSTRAINTS]\n{}",
                serde_json::to_string(&security_violations).unwrap_or_default()));
        }
        Ok(json!({ "_display": output, "path": path, "lines": line_count }))
    }

    /// Execute bash command via the command daemon's execute endpoint.
    /// Output is formatted as text matching TS BashToolHandler format:
    /// exit:N\nstdout\n[stderr]\nstderr\n[truncated]\n[timed out]\n[blocked: reason]
    async fn bash(&self, call: &ToolCall) -> Result<serde_json::Value> {
        // Handle --await <id>: retrieve result of a previously-started background command.
        // The await argument is passed as a separate flag by the CLI parser.
        let await_id = call.args.get("await").and_then(|v| v.as_str());

        let command = call.args.get("command").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing command argument"))?;

        // If the command is ONLY `--await <id>` (retrieve mode), look up stored result.
        // In the CLI-parsed path, --await and command are separate args, so this
        // branch fires when --await is given without a meaningful command string.
        // We check if command starts with "--await" as a secondary indicator.
        if let Some(aid) = await_id {
            if command.trim().starts_with("--await") || command.trim().is_empty() {
                // Retrieve mode: try to get stored result, or return informative error
                let store = BACKGROUND_JOBS.lock().unwrap();
                if let Some(stored) = store.get(aid) {
                    return Ok(Self::format_bash_result(stored));
                }
                return Ok(json!({
                    "status": "error",
                    "error": format!("No background command found for ID '{}'. Use a regular bash command first.", aid)
                }));
            }
        }

        let timeout_ms: i64 = call.args.get("timeout")
            .and_then(|v| v.as_i64())
            .unwrap_or(30_000);

        let agent_cwd = call.args.get("_cwd").and_then(|v| v.as_str()).unwrap_or("");
        let request = json!({
            "type": "execute",
            "command": command,
            "cwd": agent_cwd,
            "timeout_ms": timeout_ms,
        });
        let resp: crate::daemons::ExecuteResult = self.command_daemon.lock().await.send_request(request).await?;

        // If --await was provided with this command, store the result for later retrieval
        if let Some(aid) = await_id {
            let mut store = BACKGROUND_JOBS.lock().unwrap();
            store.insert(aid.to_string(), resp.clone());
        }

        Ok(Self::format_bash_result(&resp))
    }

    fn format_bash_result(resp: &crate::daemons::ExecuteResult) -> serde_json::Value {
        let mut output = format!("exit:{}", resp.exit_code);
        if !resp.stdout.is_empty() {
            output.push_str(&format!("\n{}", resp.stdout));
        }
        if !resp.stderr.is_empty() {
            output.push_str(&format!("\n[stderr]\n{}", resp.stderr));
        }
        if resp.meta.truncated {
            output.push_str("\n[truncated]");
        }
        if resp.meta.timed_out {
            output.push_str("\n[timed out]");
        }
        if let Some(ref blocked) = resp.meta.blocked {
            output.push_str(&format!("\n[blocked: {}]", blocked));
        }
        if !resp.meta.detected_patterns.is_empty() {
            output.push_str(&format!("\n[detected_patterns: {}]", resp.meta.detected_patterns.join(", ")));
        }
        if let Some(ref hint) = resp.meta.hint {
            output.push_str(&format!("\n[hint: {}]", hint));
        }

        json!({
            "_output_str": output,
            "_cwd": resp.meta.cwd,
        })
    }

    async fn compact(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let context = call.args.get("context").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing context parameter for compact tool"))?;

        let required_files: Vec<String> = call.args.get("required_files")
            .or_else(|| call.args.get("keep"))
            .and_then(|v| {
                if let Some(arr) = v.as_array() {
                    Some(arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                } else if let Some(s) = v.as_str() {
                    // Comma-separated string
                    Some(s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let mut result = json!({
            "compact": true,
            "summary": context
        });
        if !required_files.is_empty() {
            result["required_files"] = json!(required_files);
        }
        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// Write security scanning (matching TS WriteToFileToolHandler.scanContentSecurity)
// ---------------------------------------------------------------------------

struct SecurityViolation {
    path: &'static str,
    constraint: &'static str,
    detected_pattern: String,
}

/// Strip wrapping markdown code fences from content.
/// Handles ```lang\n...\n``` patterns where the LLM wraps file content.
fn strip_code_fences(content: &str) -> std::borrow::Cow<'_, str> {
    let trimmed = content.trim();
    if !trimmed.starts_with("```") {
        return std::borrow::Cow::Borrowed(content);
    }
    // Find the end of the opening fence line
    let after_open = trimmed.get(3..).unwrap_or("");
    let newline_pos = after_open.find('\n').unwrap_or(after_open.len());
    // Skip the opening ```lang\n
    let inner_start = 3 + newline_pos + 1;
    // Check for closing ```
    if inner_start >= trimmed.len() {
        return std::borrow::Cow::Borrowed(content);
    }
    let inner = &trimmed[inner_start..];
    if let Some(rest) = inner.strip_suffix("```") {
        let result = rest.trim_end();
        return std::borrow::Cow::Owned(result.to_string());
    }
    std::borrow::Cow::Borrowed(content)
}

fn scan_write_security(file_path: &str, content: &str) -> Vec<serde_json::Value> {
    let mut violations: Vec<SecurityViolation> = Vec::new();

    static DANGEROUS_PATTERNS: LazyLock<Vec<(regex::Regex, &'static str)>> = LazyLock::new(|| {
        vec![
            (regex::Regex::new(r"(?i)curl\s*\|.*(?:bash|sh)").unwrap(), "curl piped to shell"),
            (regex::Regex::new(r"(?i)wget\s*\|.*(?:bash|sh)").unwrap(), "wget piped to shell"),
            (regex::Regex::new(r"(?i)rm\s+-rf\s+/").unwrap(), "recursive root delete"),
            (regex::Regex::new(r"(?i)nc\s+-.*-e\s+/bin/(?:bash|sh)").unwrap(), "reverse shell"),
        ]
    });

    for (re, label) in DANGEROUS_PATTERNS.iter() {
        if let Some(m) = re.find(content) {
            violations.push(SecurityViolation {
                path: "$.content",
                constraint: label,
                detected_pattern: m.as_str().to_string(),
            });
        }
    }

    // Sensitive file extensions
    let sensitive_suffixes = [".env", "id_rsa", "credentials"];
    for suffix in &sensitive_suffixes {
        if file_path.ends_with(suffix) || file_path.ends_with(&format!("{}.json", suffix)) {
            violations.push(SecurityViolation {
                path: "$.path",
                constraint: "sensitive file extension",
                detected_pattern: file_path.to_string(),
            });
            break;
        }
    }

    violations.iter().map(|v| {
        json!({
            "path": v.path,
            "constraint": v.constraint,
            "detected_pattern": v.detected_pattern,
        })
    }).collect()
}

/// Parse range argument like "10-50" into (start, end).
/// Returns None if range is malformed or start < 1.
fn parse_range(call: &ToolCall) -> Option<(usize, usize)> {
    // Priority 1: --range "start-end"
    if let Some(range) = call.args.get("range").and_then(|v| v.as_str()) {
        let parts: Vec<&str> = range.split('-').collect();
        if parts.len() == 2 {
            let start: usize = parts[0].parse().ok()?;
            let end: usize = parts[1].parse().ok()?;
            if start >= 1 && start <= end { return Some((start, end)); }
        } else if parts.len() == 1 {
            let n: usize = parts[0].parse().ok()?;
            if n >= 1 { return Some((n, n)); }
        }
    }
    // Priority 2: --start-line X --end-line Y (or just one of them)
    let start = call.args.get("start_line").and_then(|v| v.as_u64()).map(|n| n as usize);
    let end = call.args.get("end_line").and_then(|v| v.as_u64()).map(|n| n as usize);
    match (start, end) {
        (Some(s), Some(e)) if s >= 1 && s <= e => Some((s, e)),
        (Some(s), None) if s >= 1 => Some((s, s)),
        (None, Some(e)) if e >= 1 => Some((1, e)),
        _ => None,
    }
}
