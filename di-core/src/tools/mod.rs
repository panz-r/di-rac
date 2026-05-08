pub mod background;
pub mod list_files;
pub mod search_files;
pub mod edit_file;
pub mod write_to_file;
pub mod ask_followup;
pub mod attempt_completion;
pub mod approval;
pub mod symbols;

use crate::daemons::{UnixDaemonClient, AnalyzerRequest, AnalyzerResponse, CommandRequest, CommandResponse};
use crate::agent::recovery::{RecoveryEngine, RecoveryAction};
use background::{BackgroundCommand, BackgroundCommandTracker, CommandStatus};
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
    recovery: RecoveryEngine,
}

impl ToolCoordinator {
    pub fn new() -> Self {
        Self { cache: HashMap::new(), recovery: RecoveryEngine::new() }
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

        // 2. Execute with retry
        let max_retries = call.args.get("retry")
            .and_then(|v| v.as_u64())
            .map(|n| std::cmp::min(n as usize, 5))
            .unwrap_or(0);

        let mut result = executor.execute_raw(call).await;
        let mut attempts = 0;
        let mut errors: Vec<String> = Vec::new();

        while attempts < max_retries {
            match &result {
                Ok(val) => {
                    let s = val.to_string();
                    if s.starts_with("<tool_error") {
                        errors.push(s.clone());
                        attempts += 1;
                        let delay = std::cmp::min(500 * 2i64.pow(attempts as u32 - 1), 4000);
                        tokio::time::sleep(std::time::Duration::from_millis(delay as u64)).await;
                        result = executor.execute_raw(call).await;
                        continue;
                    }
                    break;
                }
                Err(e) => {
                    errors.push(e.to_string());
                    attempts += 1;
                    let delay = std::cmp::min(500 * 2i64.pow(attempts as u32 - 1), 4000);
                    tokio::time::sleep(std::time::Duration::from_millis(delay as u64)).await;
                    result = executor.execute_raw(call).await;
                    continue;
                }
            }
        }

        let value = match result {
            Ok(v) => v,
            Err(ref e) => {
                let retry_history = if errors.len() > 1 {
                    Some(format!("{} prior attempts failed: {}", errors.len(), errors.join("; ")))
                } else {
                    None
                };
                let error_context = retry_history.as_deref().unwrap_or("");
                match self.recovery.handle_error(&call.name, &format!("{}{}", e, error_context)) {
                    RecoveryAction::Retry { max_attempts, delay } => {
                        let mut recovery_result = executor.execute_raw(call).await;
                        for _ in 0..max_attempts {
                            if recovery_result.is_ok() { break; }
                            tokio::time::sleep(delay).await;
                            recovery_result = executor.execute_raw(call).await;
                        }
                        match recovery_result {
                            Ok(v) => v,
                            Err(e) => return Err(e),
                        }
                    }
                    RecoveryAction::Escalate(msg) => {
                        return Ok(json!({ "error": msg, "status": "escalated" }));
                    }
                    RecoveryAction::Fail(msg) => {
                        return Err(anyhow!("{}", msg));
                    }
                }
            }
        };

        // 3. Auto-correction for truncated output
        let value = if let Some(corrected) = self.auto_correct_truncated(call, &value, executor).await {
            corrected
        } else {
            value
        };

        // 4. Cache store
        if is_cacheable {
            let serialized = value.to_string();
            self.cache.insert(cache_key, serialized);
        }

        if attempts > 0 {
            if let serde_json::Value::String(s) = &value {
                return Ok(serde_json::Value::String(format!("[Retry] {} attempts\n{}", attempts, s)));
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
        let degraded = ToolCall { name: call.name.clone(), args };
        executor.execute_raw(&degraded).await.ok()
    }
}

// ---------------------------------------------------------------------------
// Tool Executor (raw dispatch)
// ---------------------------------------------------------------------------

pub struct ToolExecutor {
    analyzer_client: Arc<UnixDaemonClient>,
    command_client: Arc<UnixDaemonClient>,
    background_tracker: Arc<BackgroundCommandTracker>,
}

impl ToolExecutor {
    pub fn new(
        analyzer_client: Arc<UnixDaemonClient>,
        command_client: Arc<UnixDaemonClient>,
        background_tracker: Arc<BackgroundCommandTracker>,
    ) -> Self {
        Self {
            analyzer_client,
            command_client,
            background_tracker,
        }
    }

    pub async fn execute(&self, call: &ToolCall, coordinator: &mut ToolCoordinator) -> Result<serde_json::Value> {
        coordinator.execute_with_coordination(call, self).await
    }

    /// Raw tool dispatch. Wire names match v9.5.1 spec.
    pub async fn execute_raw(&self, call: &ToolCall) -> Result<serde_json::Value> {
        match call.name.as_str() {
            "read" => self.read_file(call).await,
            "write" => self.write_file(call).await,
            "edit" => edit_file::edit_file(&self.command_client, call).await,
            "search" => search_files::search_files(&self.analyzer_client, call).await,
            "repo" => list_files::list_files(&self.analyzer_client, call).await,
            "bash" => self.bash(call).await,
            "compact" => self.compact(call).await,
            "ask" => {
                let (question, options) = ask_followup::parse_followup_question(call)?;
                Ok(json!({ "_frontend_action": "followup_question", "question": question, "options": options }))
            }
            "done" => {
                let (result, command) = attempt_completion::parse_completion(call)?;
                Ok(json!({ "_frontend_action": "attempt_completion", "result": result, "command": command }))
            }
            "symbols" => symbols::symbols(&self.analyzer_client, call).await,
            "plan" => {
                let plan = call.args.get("plan").and_then(|v| v.as_str())
                    .or_else(|| call.args.get("text").and_then(|v| v.as_str()))
                    .unwrap_or("");
                Ok(json!({ "_frontend_action": "plan_response", "plan": plan }))
            }
            "task" => {
                let task = call.args.get("task").and_then(|v| v.as_str())
                    .or_else(|| call.args.get("text").and_then(|v| v.as_str()))
                    .unwrap_or("");
                Ok(json!({ "_frontend_action": "new_task", "task": task }))
            }
            "tools" => {
                let tool_names = [
                    "read", "write", "edit", "search", "repo", "bash",
                    "compact", "ask", "done", "symbols", "plan", "task", "tools",
                ];
                let list: Vec<&str> = if let Some(filter) = call.args.get("filter").and_then(|v| v.as_str()) {
                    tool_names.iter().filter(|t| t.contains(&filter.to_lowercase())).copied().collect()
                } else {
                    tool_names.to_vec()
                };
                Ok(json!({ "tools": list, "count": list.len() }))
            }
            _ => Err(anyhow!("Unknown tool: {}", call.name)),
        }
    }

    async fn read_file(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let path = call.args.get("path").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing path argument"))?;

        let resp: AnalyzerResponse = self.analyzer_client.send_request(AnalyzerRequest {
            command: "read-file".to_string(),
            file: Some(path.to_string()),
            content: None,
            language: None,
            query: None,
        })?;

        if resp.ok {
            Ok(resp.data)
        } else {
            Err(anyhow!("Failed to read file: {:?}", resp.data))
        }
    }

    async fn write_file(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let path = call.args.get("path").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing path argument"))?;
        let content = call.args.get("content").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing content argument"))?;

        let create_dirs = call.args.get("create_dirs")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        if create_dirs {
            let parent = std::path::Path::new(path).parent()
                .and_then(|p| p.to_str())
                .map(String::from);
            if let Some(dir) = parent {
                if !dir.is_empty() {
                    let _mkdir_resp: Result<CommandResponse> = self.command_client.send_request(CommandRequest {
                        command: "shell".to_string(),
                        args: vec![format!("mkdir -p {}", dir)],
                        cwd: None,
                    });
                }
            }
        }

        let resp: CommandResponse = self.command_client.send_request(CommandRequest {
            command: "write-file".to_string(),
            args: vec![path.to_string(), content.to_string()],
            cwd: None,
        })?;

        if resp.ok {
            Ok(json!({
                "path": path,
                "status": "success",
                "lines": content.lines().count()
            }))
        } else {
            Err(anyhow!("Failed to write file: {}", resp.stderr))
        }
    }

    /// Execute bash command. If --await <id> is specified, return background command result.
    /// Otherwise, if the command runs long, spawn it in the background.
    async fn bash(&self, call: &ToolCall) -> Result<serde_json::Value> {
        // Check for --await to retrieve background command result
        if let Some(await_id) = call.args.get("await").and_then(|v| v.as_str()) {
            return self.await_background_command(await_id).await;
        }

        let command = call.args.get("command").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing command argument"))?;

        let timeout_ms: i64 = call.args.get("timeout")
            .and_then(|v| v.as_i64())
            .unwrap_or(300_000);

        // Check if we're at background command capacity
        let running = self.background_tracker.count_running().await;
        if running >= 8 {
            return Ok(json!({
                "status": "error",
                "exit_code": -1,
                "stderr": format!("Background command limit (8) reached. Await a running command first."),
            }));
        }

        let resp: CommandResponse = self.command_client.send_request(CommandRequest {
            command: "shell".to_string(),
            args: vec![command.to_string()],
            cwd: call.args.get("cwd").and_then(|v| v.as_str()).map(String::from),
        })?;

        // Check if still running (daemon returns special exit code for background)
        if resp.exit_code == -1 {
            let id = resp.stdout.trim().to_string();
            let log_path = format!("/tmp/di-core-{}.log", id);

            self.background_tracker.track(BackgroundCommand {
                id: id.clone(),
                command: command.to_string(),
                start_time: chrono::Utc::now(),
                status: CommandStatus::Running,
                log_path: log_path.clone(),
                exit_code: None,
            }).await;

            return Ok(json!({
                "id": id,
                "status": "running",
                "log_path": log_path,
                "hint": format!("Use bash --await {} to get the result", id)
            }));
        }

        Ok(json!({
            "exit_code": resp.exit_code,
            "stdout": resp.stdout,
            "stderr": resp.stderr,
        }))
    }

    async fn await_background_command(&self, id: &str) -> Result<serde_json::Value> {
        match self.background_tracker.get(id).await {
            Some(cmd) => {
                match cmd.status {
                    CommandStatus::Running => {
                        let log = std::fs::read_to_string(&cmd.log_path).unwrap_or_default();
                        Ok(json!({
                            "id": id,
                            "status": "running",
                            "stdout": log,
                            "hint": format!("Command still running. Use bash --await {} later.", id)
                        }))
                    }
                    CommandStatus::Completed => {
                        let log = std::fs::read_to_string(&cmd.log_path).unwrap_or_default();
                        Ok(json!({
                            "id": id,
                            "status": "completed",
                            "exit_code": cmd.exit_code,
                            "stdout": log,
                        }))
                    }
                    CommandStatus::Failed => {
                        let log = std::fs::read_to_string(&cmd.log_path).unwrap_or_default();
                        Ok(json!({
                            "id": id,
                            "status": "failed",
                            "exit_code": cmd.exit_code,
                            "stdout": log,
                        }))
                    }
                    _ => Ok(json!({ "id": id, "status": "unknown" })),
                }
            }
            None => Err(anyhow!("Background command {} not found", id)),
        }
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
