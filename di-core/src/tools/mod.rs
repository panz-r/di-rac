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

use crate::daemons::{AnalyzerRequest, AnalyzerResponse, ResilientDaemon};
use response::{ToolResponse, ToolErrorCode};
use routing::{ErrorRouter, RoutingContext, ToolErrorRoute};
use format::{format_error_for_llm, format_error_for_log};
use serde::{Deserialize, Serialize};
use anyhow::{Result, anyhow};
use std::collections::HashMap;
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

pub struct ToolCoordinator {
    cache: HashMap<String, String>,
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

    pub async fn execute_with_coordination(
        &mut self,
        call: &ToolCall,
        executor: &ToolExecutor,
    ) -> Result<serde_json::Value> {
        let is_cacheable = CACHEABLE_TOOLS.contains(&call.name.as_str());
        let cache_key = self.make_cache_key(call);

        // 1. Cache check
        if is_cacheable {
            if let Some(cached) = self.cache.get(&cache_key) {
                return Ok(serde_json::Value::String(format!("[Cache Hit]{}", cached)));
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

        // 4. Cache store with bounded eviction
        if is_cacheable {
            if self.cache.len() >= MAX_CACHE_ENTRIES {
                // Remove oldest 25% to avoid unbounded growth
                let evict_count = MAX_CACHE_ENTRIES / 4;
                let keys: Vec<_> = self.cache.keys().take(evict_count).cloned().collect();
                for k in keys { self.cache.remove(&k); }
            }
            let serialized = value.to_string();
            self.cache.insert(cache_key, serialized);
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
        format!("{}:{}", call.name, args)
    }

    async fn auto_correct_truncated(
        &self,
        call: &ToolCall,
        result: &serde_json::Value,
        executor: &ToolExecutor,
    ) -> Option<serde_json::Value> {
        let s = result.to_string();
        if !s.contains("[truncated]") && !s.contains("... [Content reduced") {
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
        // Remove CLI command string so execute_raw uses structured fields instead of re-parsing
        args_map.remove("command");
        let degraded = ToolCall { name: call.name.clone(), args };
        match executor.execute_raw(&degraded).await {
            ToolResponse::Success { data, .. } => Some(data),
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
                    "get_outputs",
                ];
                let list: Vec<&str> = if let Some(filter) = parsed_call.args.get("filter").and_then(|v| v.as_str()) {
                    tool_names.iter().filter(|t| t.contains(&filter.to_lowercase())).copied().collect()
                } else {
                    tool_names.to_vec()
                };
                ToolResponse::ok(json!({ "tools": list, "count": list.len() }))
            }
            "get_outputs" => {
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
                            None => return ToolResponse::fail(ToolErrorCode::MissingArgument, "Missing file argument for get_outputs read".to_string(), "get_outputs"),
                        };
                        let path = {
                            let om = self.output_manager.lock().unwrap();
                            om.output_dir().join(&filename)
                        };
                        match tokio::fs::read_to_string(&path).await {
                            Ok(content) => ToolResponse::ok(json!({
                                "file": filename,
                                "content": content,
                                "lines": content.lines().count(),
                            })),
                            Err(e) => ToolResponse::fail(
                                ToolErrorCode::IoFileNotFound,
                                format!("Failed to read {}: {}", filename, e),
                                "get_outputs",
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
                        format!("Unknown get_outputs action: {}. Use list, read, or clear.", action),
                        "get_outputs",
                    ),
                }
            }
            _ => ToolResponse::fail(ToolErrorCode::InvalidInput, format!("Unknown tool: {}", call.name), name),
        }
    }

    async fn read_file(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let path = call.args.get("path").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing path argument"))?;

        if let Some(artifact_id) = path.strip_prefix("artifact://") {
            return Err(anyhow!("Artifact references are no longer supported: {}", artifact_id));
        }

        let detail = call.args.get("detail").and_then(|v| v.as_str());

        // Read raw content from disk for hash computation and auto-detail
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| anyhow!("Failed to read {}: {}", path, e))?;
        let file_size = content.len();
        let total_lines = content.lines().count();

        let effective_detail = read_file::auto_detail(file_size, detail);

        match effective_detail.as_str() {
            "outline" => {
                let resp: AnalyzerResponse = self.analyzer_daemon.lock().await.send_request(AnalyzerRequest {
                    command: "outline".to_string(),
                    file: Some(path.to_string()),
                    content: None,
                    language: None,
                    query: None,
                }).await?;
                if resp.ok {
                    Ok(json!({ "_read_raw": true, "path": path, "detail": "outline", "analyzer_data": resp.data }))
                } else {
                    Err(anyhow!("Analyzer outline failed: {:?}", resp.data))
                }
            }
            "skeleton" => {
                let resp: AnalyzerResponse = self.analyzer_daemon.lock().await.send_request(AnalyzerRequest {
                    command: "skeleton".to_string(),
                    file: Some(path.to_string()),
                    content: None,
                    language: None,
                    query: None,
                }).await?;
                if resp.ok {
                    Ok(json!({ "_read_raw": true, "path": path, "detail": "skeleton", "analyzer_data": resp.data }))
                } else {
                    Err(anyhow!("Analyzer skeleton failed: {:?}", resp.data))
                }
            }
            _ => {
                // full/preview: return raw content for engine-side formatting
                let range = parse_range(call);
                Ok(json!({
                    "_read_raw": true,
                    "path": path,
                    "detail": effective_detail,
                    "content": content,
                    "lines": total_lines,
                    "range": range.map(|(s, e)| json!([s, e])),
                }))
            }
        }
    }

    async fn write_file(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let path = call.args.get("path").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing path argument"))?;
        let content = call.args.get("content").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing content argument"))?;

        let dry_run = call.args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);
        if dry_run {
            let line_count = content.lines().count();
            return Ok(json!({
                "path": path,
                "status": "dry_run",
                "lines": line_count,
                "message": format!("Would write {} lines to {}", line_count, path)
            }));
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

        Ok(json!({
            "path": path,
            "status": "success",
            "lines": content.lines().count()
        }))
    }

    /// Execute bash command via the command daemon's execute endpoint.
    async fn bash(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let command = call.args.get("command").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing command argument"))?;

        let _timeout_ms: i64 = call.args.get("timeout")
            .and_then(|v| v.as_i64())
            .unwrap_or(300_000);

        let request = json!({
            "type": "execute",
            "command": command,
        });
        let resp: crate::daemons::ExecuteResult = self.command_daemon.lock().await.send_request(request).await?;

        Ok(json!({
            "exit_code": resp.exit_code,
            "stdout": resp.stdout,
            "stderr": resp.stderr,
        }))
    }

    async fn compact(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let context = call.args.get("context").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing context parameter for compact tool"))?;

        Ok(json!({
            "compact": true,
            "summary": context
        }))
    }
}

/// Parse range argument like "10-50" into (start, end).
/// Returns None if range is malformed or start < 1.
fn parse_range(call: &ToolCall) -> Option<(usize, usize)> {
    call.args.get("range").and_then(|v| v.as_str()).and_then(|r| {
        let parts: Vec<&str> = r.split('-').collect();
        if parts.len() == 2 {
            let start: usize = parts[0].parse().ok()?;
            let end: usize = parts[1].parse().ok()?;
            if start >= 1 && end >= start {
                Some((start, end))
            } else {
                None
            }
        } else {
            None
        }
    })
}
