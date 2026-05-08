use crate::daemons::{UnixDaemonClient, AnalyzerRequest, AnalyzerResponse, CommandRequest, CommandResponse};
use serde::{Deserialize, Serialize};
use anyhow::{Result, anyhow};
use std::sync::Arc;
use serde_json::json;

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub args: serde_json::Value,
}

pub struct ToolExecutor {
    analyzer_client: Arc<UnixDaemonClient>,
    command_client: Arc<UnixDaemonClient>,
}

impl ToolExecutor {
    pub fn new(analyzer_client: Arc<UnixDaemonClient>, command_client: Arc<UnixDaemonClient>) -> Self {
        Self {
            analyzer_client,
            command_client,
        }
    }

    pub async fn execute(&self, call: &ToolCall) -> Result<serde_json::Value> {
        match call.name.as_str() {
            "read_file" => self.read_file(call).await,
            "write_file" => self.write_file(call).await,
            "replace" => self.replace(call).await,
            "grep_search" => self.grep_search(call).await,
            "run_shell_command" => self.run_shell_command(call).await,
            _ => Err(anyhow!("Unknown tool: {}", call.name)),
        }
    }

    async fn read_file(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let path = call.args.get("path").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing path argument for read_file"))?;
        
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
            .ok_or_else(|| anyhow!("Missing path argument for write_file"))?;
        let content = call.args.get("content").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing content argument for write_file"))?;

        let resp: CommandResponse = self.command_client.send_request(CommandRequest {
            command: "write-file".to_string(),
            args: vec![path.to_string(), content.to_string()],
            cwd: None,
        })?;

        if resp.ok {
            Ok(json!({ "status": "success" }))
        } else {
            Err(anyhow!("Failed to write file: {}", resp.stderr))
        }
    }

    async fn replace(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let path = call.args.get("path").and_then(|v| v.as_str()).ok_or_else(|| anyhow!("Missing path"))?;
        let old = call.args.get("old_string").and_then(|v| v.as_str()).ok_or_else(|| anyhow!("Missing old_string"))?;
        let new = call.args.get("new_string").and_then(|v| v.as_str()).ok_or_else(|| anyhow!("Missing new_string"))?;

        let resp: CommandResponse = self.command_client.send_request(CommandRequest {
            command: "replace".to_string(),
            args: vec![path.to_string(), old.to_string(), new.to_string()],
            cwd: None,
        })?;

        if resp.ok {
            Ok(json!({ "status": "success" }))
        } else {
            Err(anyhow!("Replace failed: {}", resp.stderr))
        }
    }

    async fn grep_search(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let pattern = call.args.get("pattern").and_then(|v| v.as_str()).ok_or_else(|| anyhow!("Missing pattern"))?;
        
        let resp: AnalyzerResponse = self.analyzer_client.send_request(AnalyzerRequest {
            command: "grep".to_string(),
            query: Some(pattern.to_string()),
            file: None,
            content: None,
            language: None,
        })?;

        Ok(resp.data)
    }

    async fn run_shell_command(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let command = call.args.get("command").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing command argument"))?;

        let resp: CommandResponse = self.command_client.send_request(CommandRequest {
            command: "shell".to_string(),
            args: vec![command.to_string()],
            cwd: None,
        })?;

        Ok(json!({
            "stdout": resp.stdout,
            "stderr": resp.stderr,
            "exit_code": resp.exit_code
        }))
    }
}
