use serde::{Deserialize, Serialize};
use anyhow::{Result, anyhow};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream as AsyncUnixStream;
use tokio::sync::mpsc;

/// Error from `send_request_untimed`, distinguishing dead-daemon from application errors.
#[derive(Debug)]
pub enum UntimedError {
    /// Daemon process is dead (EOF on stdout, pipe broken). Restart required.
    Dead(String),
    /// Application-level error from daemon. Return to caller, no restart.
    App(anyhow::Error),
}

// ---------------------------------------------------------------------------
// Unix Daemon Client (synchronous UDS for C daemons)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub struct UnixDaemonClient {
    socket_path: String,
}

impl UnixDaemonClient {
    #[allow(dead_code)]
    pub fn new(socket_path: &str) -> Self {
        Self {
            socket_path: socket_path.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Analyzer daemon types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct AnalyzerRequest {
    pub command: String,
    pub file: Option<String>,
    pub content: Option<String>,
    pub language: Option<String>,
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subcommand: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnalyzerResponse {
    pub ok: bool,
    #[serde(flatten)]
    pub data: serde_json::Value,
}

// ---------------------------------------------------------------------------
// API Gateway types (NDJSON over UDS)
// Matches api-gateway/providers/provider.go and api-gateway/main.go
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ApiResponse {
    pub calls: Vec<String>,
    pub definitions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    pub model: String,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    /// Provider-specific parameters (temperature, top_p, max_tokens, etc.)
    #[serde(default)]
    pub params: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingConfig {
    #[serde(rename = "type")]
    pub thinking_type: String,
    #[serde(default)]
    pub budget_tokens: Option<i64>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GatewayMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_blocks: Option<Vec<ContentBlock>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl GatewayMessage {
    pub fn simple(role: &str, content: serde_json::Value) -> Self {
        Self {
            role: role.to_string(),
            content: Some(content),
            content_blocks: None,
            tool_calls: None,
            tool_use_id: None,
            thinking: None,
            name: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GatewayRequest {
    pub id: i64,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub timeout: Option<i64>,
    #[serde(default)]
    pub provider: Option<ProviderConfig>,
    pub messages: Vec<GatewayMessage>,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub max_tokens: Option<i64>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub thinking: Option<ThinkingConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<i64>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<i64>,
    #[serde(default)]
    pub reasoning_tokens: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
    /// Catch-all for gateway fields di-core doesn't use (tool_use, tool_result, etc.)
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    #[serde(rename = "type")]
    pub chunk_type: String,
    #[serde(default)]
    pub index: Option<i64>,
    #[serde(default)]
    pub text_delta: Option<String>,
    #[serde(default)]
    pub json_delta: Option<String>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_call_name: Option<String>,
    #[serde(default)]
    pub thinking: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(default)]
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub content: Option<serde_json::Value>,
    #[serde(default)]
    pub content_blocks: Option<Vec<ContentBlock>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GatewayResponse {
    pub id: i64,
    pub status: i64,
    #[serde(default)]
    pub body: Option<StreamChunk>,
    #[serde(default)]
    pub error: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Gateway Stream Client (async NDJSON over UDS)
// ---------------------------------------------------------------------------

pub struct GatewayStreamClient {
    socket_path: String,
}

impl GatewayStreamClient {
    pub fn with_socket(socket_path: &str) -> Self {
        Self { socket_path: socket_path.to_string() }
    }

    /// Validate that the socket path exists.
    /// Call at startup to fail early on misconfigured paths.
    /// Note: the socket may not exist if the api-gateway hasn't started yet —
    /// this is for early validation where the socket should already be present.
    pub fn validate_socket(&self) -> Result<()> {
        let path = std::path::Path::new(&self.socket_path);
        if !path.exists() {
            return Err(anyhow!("Gateway socket not found at '{}'. Check DIRAC_API_GATEWAY_SOCKET or ensure the api-gateway is running.", self.socket_path));
        }
        Ok(())
    }

    /// Send a streaming request to the api-gateway. Returns a channel
    /// receiver that yields StreamChunk values as they arrive.
    pub async fn stream_chat(
        &self,
        request: GatewayRequest,
    ) -> Result<mpsc::Receiver<Result<StreamChunk>>> {
        let (tx, rx) = mpsc::channel(16384);
        let socket_path = self.socket_path.clone();

        // Connect, write request, then read responses — fully async.
        tokio::spawn(async move {
            let mut stream = match AsyncUnixStream::connect(&socket_path).await {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(Err(anyhow!("Failed to connect to gateway at '{}': {}", socket_path, e))).await;
                    return;
                }
            };

            // Write request
            let json = match serde_json::to_string(&request) {
                Ok(j) => j,
                Err(e) => {
                    let _ = tx.send(Err(anyhow!("Failed to serialize request: {}", e))).await;
                    return;
                }
            };

            use tokio::io::AsyncWriteExt;
            if let Err(e) = stream.write_all(json.as_bytes()).await {
                let _ = tx.send(Err(anyhow!("Failed to write to gateway: {}", e))).await;
                return;
            }
            if let Err(e) = stream.write_all(b"\n").await {
                let _ = tx.send(Err(anyhow!("Failed to write to gateway: {}", e))).await;
                return;
            }
            if let Err(e) = stream.flush().await {
                let _ = tx.send(Err(anyhow!("Failed to flush gateway: {}", e))).await;
                return;
            }

            let buf_reader = BufReader::new(stream);
            let mut lines = buf_reader.lines();

            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        if line.trim().is_empty() {
                            continue;
                        }

                        match serde_json::from_str::<GatewayResponse>(&line) {
                            Ok(resp) => {
                                if resp.status != 200 {
                                    let code = resp.error.as_ref()
                                        .and_then(|e| e.get("code"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("UNKNOWN");
                                    let msg = resp.error.as_ref()
                                        .and_then(|e| e.get("message"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown error");
                                    let _ = tx.send(Err(anyhow!("{}: {}", code, msg))).await;
                                    break;
                                }

                                if let Some(chunk) = resp.body {
                                    let is_complete = chunk.chunk_type == "complete";
                                    if tx.send(Ok(chunk)).await.is_err() {
                                        break; // receiver dropped
                                    }
                                    if is_complete {
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(Err(anyhow!("Failed to parse gateway response: {}", e))).await;
                                break;
                            }
                        }
                    }
                    Ok(None) => break, // stream ended
                    Err(e) => {
                        let _ = tx.send(Err(anyhow!("Read error: {}", e))).await;
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }
}

// ---------------------------------------------------------------------------
// Command Daemon Client (piped child process — stdin/stdout)
// Spawns di-rvv-cmd as a child process, communicates via JSON lines.
// Mirrors the TS CommandClient in src/services/command/CommandClient.ts.
// ---------------------------------------------------------------------------

/// Result from the command daemon's execute endpoint.
/// Wire format: {"type":"result","id":"1","stdout":"...","stderr":"","exit_code":0,"meta":{...}}
#[derive(Debug, Deserialize)]
pub struct ExecuteResult {
    #[allow(dead_code)]
    pub id: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    #[allow(dead_code)]
    pub meta: ExecuteMeta,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ExecuteMeta {
    pub mode_used: String,
    pub cwd: String,
    pub truncated: bool,
    pub truncation_offset: i64,
    pub hint: Option<String>,
    pub blocked: Option<String>,
    pub timed_out: bool,
    #[serde(default)]
    pub detected_patterns: Vec<String>,
}

pub struct CommandDaemon {
    stdin: tokio::process::ChildStdin,
    stdout: tokio::io::BufReader<tokio::process::ChildStdout>,
    _child: tokio::process::Child,
    request_id: u32,
}

impl CommandDaemon {
    /// Spawn the command daemon as a child process.
    /// Waits for "ready" on stderr before returning.
    pub async fn spawn(binary_path: &str, workspace_root: &str) -> Result<Self> {
        if !std::path::Path::new(binary_path).exists() {
            return Err(anyhow!("Command daemon binary not found: {}", binary_path));
        }

        let mut child = tokio::process::Command::new(binary_path)
            .arg("--workspace-root")
            .arg(workspace_root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        let stdin = child.stdin.take().ok_or_else(|| anyhow!("Failed to get command daemon stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("Failed to get command daemon stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| anyhow!("Failed to get command daemon stderr"))?;

        // Wait for "ready" on stderr
        let mut stderr_reader = tokio::io::BufReader::new(stderr);
        let mut ready_line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stderr_reader.read_line(&mut ready_line),
        ).await??;

        let stdout_reader = tokio::io::BufReader::new(stdout);

        Ok(Self {
            stdin,
            stdout: stdout_reader,
            _child: child,
            request_id: 0,
        })
    }

    /// Execute a shell command via the command daemon.
    /// Sends {"id":"N","type":"execute","command":"..."} and waits for result.
    /// Timeout: 300s for long-running commands.
    #[allow(dead_code)]
    pub async fn execute(&mut self, command: &str) -> Result<ExecuteResult> {
        self.request_id += 1;
        let id = self.request_id.to_string();

        let request = serde_json::json!({
            "id": id,
            "type": "execute",
            "command": command,
        });

        let json_str = serde_json::to_string(&request)?;
        use tokio::io::AsyncWriteExt;
        self.stdin.write_all(json_str.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;

        let timeout = std::time::Duration::from_secs(300);
        let mut line = String::new();
        let mut bad_lines = 0u32;
        loop {
            line.clear();
            let read_future = self.stdout.read_line(&mut line);
            let n = tokio::time::timeout(timeout, read_future).await
                .map_err(|_| anyhow!("Command daemon execute timed out after 300s (id={}, cmd={})", id, &command[..command.len().min(80)]))??;
            if n == 0 {
                return Err(anyhow!("Command daemon closed stdout"));
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                bad_lines = 0;
                let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");

                match msg_type {
                    "error" => {
                        let msg = val.get("message").and_then(|v| v.as_str()).unwrap_or("unknown error");
                        return Err(anyhow!("Command daemon error: {}", msg));
                    }
                    "ack" | "progress" => continue,
                    "result" => {
                        return serde_json::from_str::<ExecuteResult>(trimmed)
                            .map_err(|e| anyhow!("Failed to parse execute result: {} — input: {}", e, &trimmed[..trimmed.len().min(200)]));
                    }
                    _ => continue,
                }
            } else {
                bad_lines += 1;
                if bad_lines >= 10 {
                    return Err(anyhow!(
                        "Command daemon: 10 consecutive unparseable lines (id={}, cmd={})",
                        id, &command[..command.len().min(80)]
                    ));
                }
                continue;
            }
        }
    }

    /// No-timeout variant of send_request for the analyzer daemon.
    /// Returns `UntimedError::Dead` when the process has exited (EOF/broken pipe)
    /// or `UntimedError::App` for application-level errors. No timeout wrapper —
    /// reads block indefinitely until the daemon responds or dies.
    pub async fn send_request_untimed<T: Serialize, R: for<'de> Deserialize<'de>>(
        &mut self,
        request: &T,
    ) -> Result<R, UntimedError> {
        self.request_id += 1;
        let id = self.request_id;

        let mut payload: serde_json::Map<String, serde_json::Value> = serde_json::to_value(request)
            .map_err(|e| UntimedError::App(anyhow!("Failed to serialize request: {}", e)))?
            .as_object()
            .ok_or_else(|| UntimedError::App(anyhow!("Request must be a JSON object")))?
            .clone();

        payload.insert("id".to_string(), serde_json::Value::Number(id.into()));

        let json = serde_json::to_string(&serde_json::Value::Object(payload))
            .map_err(|e| UntimedError::App(anyhow!("Failed to serialize payload: {}", e)))?;

        use tokio::io::AsyncWriteExt;
        if let Err(e) = self.stdin.write_all(json.as_bytes()).await {
            return Err(UntimedError::Dead(format!("Write failed (daemon dead?): {}", e)));
        }
        if let Err(e) = self.stdin.write_all(b"\n").await {
            return Err(UntimedError::Dead(format!("Write failed (daemon dead?): {}", e)));
        }
        if let Err(e) = self.stdin.flush().await {
            return Err(UntimedError::Dead(format!("Flush failed (daemon dead?): {}", e)));
        }

        let id_str = id.to_string();
        let mut line = String::new();
        let mut bad_lines = 0u32;
        loop {
            line.clear();
            let n = self.stdout.read_line(&mut line).await
                .map_err(|e| UntimedError::Dead(format!("Read error: {}", e)))?;
            if n == 0 {
                return Err(UntimedError::Dead("Daemon stdout EOF — process exited".to_string()));
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                bad_lines = 0;
                let resp_id = match val.get("id") {
                    Some(v) => v.as_str().map(String::from)
                        .or_else(|| v.as_u64().map(|n| n.to_string())),
                    None => None,
                };
                let resp_id_str = resp_id.as_deref().unwrap_or("");

                let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let ok = val.get("ok").and_then(|v| v.as_bool());

                // Application error — not a dead daemon, return to caller
                if msg_type == "error" || ok == Some(false) {
                    let msg = val.get("message").and_then(|v| v.as_str())
                        .or_else(|| val.get("error").and_then(|e| e.get("message")).and_then(|v| v.as_str()))
                        .unwrap_or("unknown daemon error");
                    return Err(UntimedError::App(anyhow!("Daemon error: {}", msg)));
                }

                if msg_type == "ack" || msg_type == "progress" {
                    continue;
                }

                if !resp_id_str.is_empty() && resp_id_str != id_str {
                    eprintln!("[di-core] send_request_untimed: skipping response for id={} (expecting {})", resp_id_str, id);
                    continue;
                }

                return serde_json::from_str::<R>(trimmed)
                    .map_err(|e| UntimedError::App(anyhow!(
                        "Failed to parse daemon response: {} — input: {}", e, &trimmed[..trimmed.len().min(200)]
                    )));
            } else {
                bad_lines += 1;
                if bad_lines >= 10 {
                    return Err(UntimedError::App(anyhow!(
                        "Daemon: 10 consecutive unparseable lines (request_id={})", id
                    )));
                }
                continue;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Resilient Daemon Wrapper (auto-restart on death, no timeout, panic on failure)
// Wraps CommandDaemon for analyzer daemon use. If the daemon dies (crash, hang),
// it is automatically restarted. If restart also fails, panics with a clear message.
// ---------------------------------------------------------------------------

pub struct ResilientDaemon {
    inner: Option<CommandDaemon>,
    binary_path: String,
    workspace_root: String,
    max_restart_attempts: usize,
}

impl ResilientDaemon {
    pub async fn spawn(binary_path: &str, workspace_root: &str) -> Result<Self> {
        let inner = CommandDaemon::spawn(binary_path, workspace_root).await?;
        Ok(Self {
            inner: Some(inner),
            binary_path: binary_path.to_string(),
            workspace_root: workspace_root.to_string(),
            max_restart_attempts: 2,
        })
    }

    async fn restart(&mut self) -> std::result::Result<(), String> {
        eprintln!("[di-core] ResilientDaemon: restarting analyzer daemon at {}", self.binary_path);
        self.inner = None;
        match CommandDaemon::spawn(&self.binary_path, &self.workspace_root).await {
            Ok(daemon) => {
                self.inner = Some(daemon);
                eprintln!("[di-core] ResilientDaemon: restart successful");
                Ok(())
            }
            Err(e) => {
                let msg = format!(
                    "analyzer daemon restart failed: {}. Binary: {}",
                    e, self.binary_path
                );
                eprintln!("[di-core] ResilientDaemon: {}", msg);
                Err(msg)
            }
        }
    }

    /// Send a request to the analyzer daemon with automatic restart on death.
    /// Wraps the untimed call in a 2-minute timeout to prevent indefinite hangs.
    /// Returns `Ok(R)` on success, `Err` for application or daemon errors.
    pub async fn send_request<T: Serialize, R: for<'de> Deserialize<'de>>(&mut self, request: T) -> Result<R> {
        let mut attempts = 0;
        loop {
            if self.inner.is_none() {
                if let Err(msg) = self.restart().await {
                    attempts += 1;
                    if attempts > self.max_restart_attempts {
                        return Err(anyhow::anyhow!("Analyzer daemon unavailable after {} attempts: {}", self.max_restart_attempts, msg));
                    }
                    continue;
                }
            }

            let timeout = tokio::time::Duration::from_secs(120);
            let result = tokio::time::timeout(
                timeout,
                self.inner.as_mut().unwrap().send_request_untimed(&request),
            ).await;

            match result {
                Ok(Ok(r)) => return Ok(r),
                Ok(Err(UntimedError::Dead(msg))) => {
                    attempts += 1;
                    if attempts > self.max_restart_attempts {
                        return Err(anyhow::anyhow!(
                            "Analyzer daemon failed after {} restart attempts: {}",
                            self.max_restart_attempts, msg
                        ));
                    }
                    eprintln!(
                        "[di-core] ResilientDaemon: daemon dead ({}), restarting {}/{}",
                        msg, attempts, self.max_restart_attempts
                    );
                    self.inner = None;
                    continue;
                }
                Ok(Err(UntimedError::App(e))) => return Err(e),
                Err(_) => {
                    // Timeout — treat as dead daemon
                    let msg = format!("Daemon timed out after 120s");
                    attempts += 1;
                    if attempts > self.max_restart_attempts {
                        return Err(anyhow::anyhow!(
                            "Analyzer daemon timed out after {} restart attempts: {}",
                            self.max_restart_attempts, msg
                        ));
                    }
                    eprintln!(
                        "[di-core] ResilientDaemon: {}, restarting {}/{}",
                        msg, attempts, self.max_restart_attempts
                    );
                    self.inner = None;
                    continue;
                }
            }
        }
    }
}
