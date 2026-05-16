use serde::{Deserialize, Serialize};
use anyhow::{Result, anyhow};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
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

// API extraction response (used in engine.rs for Python sandbox)
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
    /// Serde rename matches Go's api-gateway ProviderConfig.Extra field.
    #[serde(default, rename = "extra")]
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
            return Err(anyhow!("Gateway socket not found at '{}'. Check DI_API_GATEWAY_SOCKET or ensure the api-gateway is running.", self.socket_path));
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
        // Client-side read timeout: 60s beyond the gateway timeout, to protect
        // against the gateway hanging without responding.  The gateway enforces
        // its own timeout (typically 240s) but this is a safety net.
        let read_timeout = request.timeout
            .map(|ms| std::time::Duration::from_millis(ms as u64 + 60_000))
            .unwrap_or(std::time::Duration::from_secs(300));

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

            let timeout_ref = &read_timeout;
            loop {
                match tokio::time::timeout(*timeout_ref, lines.next_line()).await {
                    Ok(Ok(Some(line))) => {
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
                    Ok(Ok(None)) => {
                        // Stream ended without a "complete" chunk — the gateway
                        // dropped the connection mid-stream. Signal this as an
                        // error so the engine knows the stream was interrupted.
                        let _ = tx.send(Err(anyhow!("STREAM_EOF: gateway closed connection without complete signal"))).await;
                        break;
                    }
                    Ok(Err(e)) => {
                        let _ = tx.send(Err(anyhow!("Read error: {}", e))).await;
                        break;
                    }
                    Err(_) => {
                        // Client-side timeout: gateway didn't respond within the deadline.
                        let _ = tx.send(Err(anyhow!("CLIENT_TIMEOUT: gateway did not respond within read deadline"))).await;
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
#[derive(Debug, Clone, Deserialize)]
pub struct ExecuteResult {
    #[allow(dead_code)]
    pub id: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    #[allow(dead_code)]
    pub meta: ExecuteMeta,
}

#[derive(Debug, Clone, Deserialize)]
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

// ---------------------------------------------------------------------------
// DaemonClient — single-task IPC with the daemon child process.
// Owns stdin/stdout, runs a select! loop over three event sources:
//   • stdout lines   → parse & dispatch to pending request by id
//   • new submissions → register in pending map, write to stdin
//   • timer tick      → expire timed-out requests
// No shared mutexes, no separate reader task — everything lives in one task.
// ---------------------------------------------------------------------------

/// Internal message sent from `DaemonClient` to the daemon task.
struct DaemonSubmit {
    id: u32,
    json: String,          // pre-serialized request with id injected
    response_tx: tokio::sync::oneshot::Sender<Result<String, UntimedError>>,
    deadline: tokio::time::Instant,
}

/// Handle to the daemon task. Drop kills the child.
pub struct CommandDaemon {
    submit_tx: tokio::sync::mpsc::UnboundedSender<DaemonSubmit>,
    _child: tokio::process::Child,
    next_id: std::sync::atomic::AtomicU32,
}

impl CommandDaemon {
    pub async fn spawn(binary_path: &str, workspace_root: &str) -> Result<Self> {
        if !std::path::Path::new(binary_path).exists() {
            return Err(anyhow!("Daemon binary not found: {}", binary_path));
        }
        let mut child = tokio::process::Command::new(binary_path)
            .arg("--workspace-root")
            .arg(workspace_root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("No stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("No stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| anyhow!("No stderr"))?;

        // Wait for "ready" on stderr
        let mut stderr_reader = tokio::io::BufReader::new(stderr);
        let mut ready = String::new();
        tokio::time::timeout(Duration::from_secs(5), stderr_reader.read_line(&mut ready)).await??;

        // Drain stderr in background (unused but must be read to avoid SIGPIPE).
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut r = stderr_reader.into_inner();
            let mut buf = [0u8; 4096];
            while let Ok(n) = r.read(&mut buf).await { if n == 0 { break; } }
        });

        let (submit_tx, submit_rx): (tokio::sync::mpsc::UnboundedSender<DaemonSubmit>, tokio::sync::mpsc::UnboundedReceiver<DaemonSubmit>) = tokio::sync::mpsc::unbounded_channel();
        let reader = tokio::io::BufReader::new(stdout);
        let mut buf_stdin = tokio::io::BufWriter::new(stdin);

        // Single task: owns the pipes, pending map, and timeout loop.
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = reader.lines();
            let mut pending: std::collections::HashMap<u32, DaemonSubmit> = std::collections::HashMap::new();
            let mut submit_rx = submit_rx;

            loop {
                // Timeout for the next timer tick.
                let tick = tokio::time::sleep(Duration::from_millis(200));
                tokio::pin!(tick);

                tokio::select! {
                    biased; // check submissions first, then responses, then timeouts

                    req = submit_rx.recv() => {
                        let req = req;
                        if let Some(r) = req {
                            let id = r.id;
                            let json = r.json.clone();
                            pending.insert(r.id, r);
                            // Write request to stdin. If the pipe breaks, fail all pending.
                            let write_ok = buf_stdin.write_all(json.as_bytes()).await.is_ok()
                                && buf_stdin.write_all(b"\n").await.is_ok()
                                && buf_stdin.flush().await.is_ok();
                            if !write_ok {
                                for (_, p) in pending.drain() {
                                    let _ = p.response_tx.send(Err(UntimedError::Dead(
                                        "Write failed: pipe broken".to_string()
                                    )));
                                }
                                break;
                            }
                        } else {
                            break;
                        }
                    }

                    line = lines.next_line() => {
                        match line {
                            Ok(Some(raw)) => {
                                let trimmed = raw.trim();
                                if trimmed.is_empty() { continue; }
                                let val: serde_json::Value = match serde_json::from_str(trimmed) {
                                    Ok(v) => v,
                                    Err(_) => continue,
                                };
                                let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                if msg_type == "ack" || msg_type == "progress" { continue; }
                                let resp_id = val.get("id")
                                    .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok())))
                                    .unwrap_or(0) as u32;
                                if let Some(p) = pending.remove(&resp_id) {
                                    let result = if msg_type == "error" || val.get("ok").and_then(|v| v.as_bool()) == Some(false) {
                                        Err(UntimedError::App(anyhow!("{}", trimmed)))
                                    } else {
                                        Ok(trimmed.to_string())
                                    };
                                    let _ = p.response_tx.send(result);
                                }
                            }
                            Ok(None) | Err(_) => {
                                // Daemon died
                                for (_, p) in pending.drain() {
                                    let _ = p.response_tx.send(Err(UntimedError::Dead(
                                        "Daemon stdout closed".to_string()
                                    )));
                                }
                                break;
                            }
                        }
                    }

                    // Timer tick — expire timed-out requests
                    _ = tick => {
                        let now = tokio::time::Instant::now();
                        let expired: Vec<u32> = pending.iter()
                            .filter(|(_, p)| p.deadline <= now)
                            .map(|(id, _)| *id)
                            .collect();
                        for id in &expired {
                            if let Some(p) = pending.remove(id) {
                                let _ = p.response_tx.send(Err(UntimedError::Dead(
                                    format!("Daemon timed out (request_id={})", p.id)
                                )));
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            submit_tx,
            _child: child,
            next_id: std::sync::atomic::AtomicU32::new(0),
        })
    }

    /// Submit a request and wait for the response. Multiple callers can have
    /// in-flight requests simultaneously — the daemon task dispatches by id.
    pub async fn send_request_untimed<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        request: &T,
    ) -> Result<R, UntimedError> {
        let raw = self.next_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let id = raw.wrapping_add(1);
        let id = if id == 0 { 1 } else { id };

        let mut payload: serde_json::Map<String, serde_json::Value> = serde_json::to_value(request)
            .map_err(|e| UntimedError::App(anyhow!("Serialize: {}", e)))?
            .as_object()
            .ok_or_else(|| UntimedError::App(anyhow!("Not an object")))?
            .clone();
        payload.insert("id".to_string(), serde_json::Value::Number(id.into()));

        let json = serde_json::to_string(&serde_json::Value::Object(payload))
            .map_err(|e| UntimedError::App(anyhow!("Serialize: {}", e)))?;

        let (response_tx, rx) = tokio::sync::oneshot::channel();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(600);

        self.submit_tx.send(DaemonSubmit { id, json, response_tx, deadline })
            .map_err(|_| UntimedError::Dead("Daemon task died".to_string()))?;

        match rx.await {
            Ok(Ok(raw)) => {
                let val: serde_json::Value = serde_json::from_str(&raw)
                    .map_err(|e| UntimedError::App(anyhow!("Parse: {}", e)))?;
                let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let ok = val.get("ok").and_then(|v| v.as_bool());

                if msg_type == "error" || ok == Some(false) {
                    let code = val.get("code").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
                    let error_type = val.get("error_type").and_then(|v| v.as_str()).unwrap_or("ToolInternalError");
                    let msg = val.get("message").and_then(|v| v.as_str())
                        .or_else(|| val.get("error").and_then(|e| e.get("message")).and_then(|v| v.as_str()))
                        .unwrap_or("unknown error");
                    return Err(UntimedError::App(anyhow!("[{}][{}] {}", code, error_type, msg)));
                }

                serde_json::from_str::<R>(&raw)
                    .map_err(|e| UntimedError::App(anyhow!("Parse response: {} — input: {}", e, &raw[..raw.len().min(200)])))
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(UntimedError::Dead("Daemon task died".to_string())),
        }
    }
}

impl Drop for CommandDaemon {
    fn drop(&mut self) {
        let _ = self._child.start_kill();
    }
}

// ---------------------------------------------------------------------------
// Resilient Daemon Wrapper (auto-restart on death, no timeout, panic on failure)
// Wraps CommandDaemon for analyzer daemon use. If the daemon dies (crash, hang),
// it is automatically restarted. If restart also fails, panics with a clear message.
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
    /// Send a request with restart and retry.
    /// Safe default: no retry on daemon death (prevents duplicate mutations).
    pub async fn send_request<T: Serialize, R: for<'de> Deserialize<'de>>(&mut self, request: T) -> Result<R> {
        self.send_request_impl(request, false).await
    }

    /// Send a request with restart and retry enabled.
    /// Use for read-only operations where retry is safe (e.g. analyzer queries).
    pub async fn send_request_retry<T: Serialize, R: for<'de> Deserialize<'de>>(&mut self, request: T) -> Result<R> {
        self.send_request_impl(request, true).await
    }

    async fn send_request_impl<T: Serialize, R: for<'de> Deserialize<'de>>(&mut self, request: T, retry_on_death: bool) -> Result<R> {
        let max_attempts = if retry_on_death { self.max_restart_attempts } else { 1 };
        let mut attempts = 0;
        loop {
            if self.inner.is_none() {
                if let Err(msg) = self.restart().await {
                    attempts += 1;
                    if attempts >= max_attempts {
                        return Err(anyhow::anyhow!("Analyzer daemon unavailable after {} attempts: {}", max_attempts, msg));
                    }
                    continue;
                }
            }

            // Timeout covers both analyzer (fast) and command (slow, up to 10min) daemons.
            let timeout = tokio::time::Duration::from_secs(600);
            let result = tokio::time::timeout(
                timeout,
                self.inner.as_mut().unwrap().send_request_untimed(&request),
            ).await;

            match result {
                Ok(Ok(r)) => return Ok(r),
                Ok(Err(UntimedError::Dead(msg))) => {
                    attempts += 1;
                    if attempts >= max_attempts {
                        return Err(anyhow::anyhow!(
                            "Analyzer daemon failed after {} attempt{}: {}",
                            max_attempts, if max_attempts == 1 { "" } else { "s" }, msg
                        ));
                    }
                    eprintln!(
                        "[di-core] ResilientDaemon: daemon dead ({}), restarting {}/{}",
                        msg, attempts, max_attempts
                    );
                    self.inner = None;
                    continue;
                }
                Ok(Err(UntimedError::App(e))) => return Err(e),
                Err(_) => {
                    let msg = "Daemon timed out after 600s";
                    attempts += 1;
                    if attempts >= max_attempts {
                        return Err(anyhow::anyhow!(
                            "Analyzer daemon timed out after {} attempt{}",
                            max_attempts, if max_attempts == 1 { "" } else { "s" }
                        ));
                    }
                    eprintln!(
                        "[di-core] ResilientDaemon: {}, restarting {}/{}",
                        msg, attempts, max_attempts
                    );
                    self.inner = None;
                    continue;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ProviderConfig serialization ---

    #[test]
    fn provider_config_roundtrip() {
        let cfg = ProviderConfig {
            id: "anthropic".to_string(),
            api_key: Some("sk-test".to_string()),
            base_url: None,
            model: "claude-3".to_string(),
            region: None,
            project_id: None,
            params: [("temperature".to_string(), serde_json::json!(0.7))].into(),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains(r#""id":"anthropic""#));
        assert!(json.contains(r#""api_key":"sk-test""#));
        assert!(json.contains(r#""extra""#));

        let deserialized: ProviderConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "anthropic");
        assert_eq!(deserialized.api_key, Some("sk-test".to_string()));
        assert_eq!(deserialized.model, "claude-3");
        assert_eq!(deserialized.params.get("temperature").and_then(|v| v.as_f64()), Some(0.7));
    }

    #[test]
    fn provider_config_deserializes_from_gateway_format() {
        // This is the format the Go api-gateway sends
        let json = r#"{"id":"anthropic","api_key":"sk-test","model":"claude-3","extra":{"temperature":0.7}}"#;
        let cfg: ProviderConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.id, "anthropic");
        assert_eq!(cfg.api_key.unwrap(), "sk-test");
        assert_eq!(cfg.params.get("temperature").and_then(|v| v.as_f64()), Some(0.7));
    }

    // --- GatewayMessage ---

    #[test]
    fn gateway_message_simple_creates_correctly() {
        let msg = GatewayMessage::simple("user", serde_json::json!("hello"));
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, Some(serde_json::json!("hello")));
        assert!(msg.tool_calls.is_none());
        assert!(msg.thinking.is_none());
    }

    #[test]
    fn gateway_message_roundtrip() {
        let msg = GatewayMessage {
            role: "assistant".to_string(),
            content: Some(serde_json::json!("Hello!")),
            content_blocks: None,
            tool_calls: Some(vec![
                serde_json::json!({
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "bash", "arguments": r#"{"cmd":"ls"}"#}
                })
            ]),
            tool_use_id: None,
            thinking: Some("Let me think...".to_string()),
            name: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: GatewayMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.role, "assistant");
        assert!(deserialized.tool_calls.is_some());
        assert_eq!(deserialized.tool_calls.unwrap().len(), 1);
    }

    // --- GatewayRequest ---

    #[test]
    fn gateway_request_roundtrip() {
        let req = GatewayRequest {
            id: 42,
            stream: true,
            timeout: Some(120000),
            provider: Some(ProviderConfig {
                id: "openai".to_string(),
                api_key: Some("sk-test".to_string()),
                base_url: None,
                model: "gpt-4".to_string(),
                region: None,
                project_id: None,
                params: [("temperature".to_string(), serde_json::json!(0.5))].into(),
            }),
            messages: vec![
                GatewayMessage::simple("user", serde_json::json!("hello")),
                GatewayMessage::simple("assistant", serde_json::json!("world")),
            ],
            system: Some("You are a helpful assistant.".to_string()),
            tools: Some(vec![
                serde_json::json!({"name": "bash", "description": "Run a command"}),
            ]),
            max_tokens: Some(4096),
            temperature: None,
            thinking: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: GatewayRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, 42);
        assert_eq!(deserialized.messages.len(), 2);
        assert_eq!(deserialized.provider.unwrap().id, "openai");
        assert_eq!(deserialized.tools.unwrap().len(), 1);
    }

    #[test]
    fn gateway_request_provider_absent_when_none() {
        // When provider is None, the JSON should not contain a "provider" key
        let req = GatewayRequest {
            id: 1,
            stream: false,
            timeout: None,
            provider: None,
            messages: vec![GatewayMessage::simple("user", serde_json::json!("hi"))],
            system: None,
            tools: None,
            max_tokens: None,
            temperature: None,
            thinking: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        // Provider is Option<ProviderConfig> with #[serde(default)] — it serializes as null/absent
        let deserialized: GatewayRequest = serde_json::from_str(&json).unwrap();
        assert!(deserialized.provider.is_none());
        assert_eq!(deserialized.messages.len(), 1);
    }

    // --- StreamChunk ---

    #[test]
    fn stream_chunk_text_delta_roundtrip() {
        let chunk = StreamChunk {
            chunk_type: "delta".to_string(), index: None, text_delta: Some("Hello".to_string()),
            json_delta: None, tool_call_id: None, tool_call_name: None, thinking: None,
            usage: None, finish_reason: None, content: None, content_blocks: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let deserialized: StreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.chunk_type, "delta");
        assert_eq!(deserialized.text_delta.unwrap(), "Hello");
    }

    #[test]
    fn stream_chunk_tool_call_roundtrip() {
        let chunk = StreamChunk {
            chunk_type: "delta".to_string(), index: Some(0),
            json_delta: Some(r#"{"cmd":"ls"}"#.to_string()),
            tool_call_id: Some("call_1".to_string()),
            tool_call_name: Some("bash".to_string()),
            text_delta: None, thinking: None, usage: None, finish_reason: None,
            content: None, content_blocks: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let deserialized: StreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tool_call_id.unwrap(), "call_1");
        assert_eq!(deserialized.tool_call_name.unwrap(), "bash");
    }

    #[test]
    fn stream_chunk_stop_with_usage() {
        let chunk = StreamChunk {
            chunk_type: "stop".to_string(), index: None,
            usage: Some(Usage {
                input_tokens: 10, output_tokens: 20, total_tokens: 30,
                cache_creation_input_tokens: None, cache_read_input_tokens: None,
                reasoning_tokens: Some(5),
            }),
            finish_reason: Some("stop".to_string()),
            text_delta: None, json_delta: None, tool_call_id: None, tool_call_name: None,
            thinking: None, content: None, content_blocks: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let deserialized: StreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.finish_reason.unwrap(), "stop");
        assert_eq!(deserialized.usage.unwrap().total_tokens, 30);
    }

    #[test]
    fn stream_chunk_roundtrip_all_none() {
        let chunk = StreamChunk {
            chunk_type: "complete".to_string(),
            index: None, text_delta: None, json_delta: None, tool_call_id: None,
            tool_call_name: None, thinking: None, usage: None, finish_reason: None,
            content: None, content_blocks: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let deserialized: StreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.chunk_type, "complete");
        assert!(deserialized.text_delta.is_none());
    }

    // --- GatewayResponse ---

    #[test]
    fn gateway_response_roundtrip_with_body() {
        let resp = GatewayResponse {
            id: 1, status: 200,
            body: Some(StreamChunk {
                chunk_type: "delta".to_string(), index: None,
                text_delta: Some("Hello".to_string()), json_delta: None,
                tool_call_id: None, tool_call_name: None, thinking: None,
                usage: None, finish_reason: None, content: None, content_blocks: None,
            }),
            error: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: GatewayResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, 1);
        assert_eq!(deserialized.status, 200);
        assert!(deserialized.body.is_some());
        assert!(deserialized.error.is_none());
    }

    #[test]
    fn gateway_response_deserializes_ack() {
        // The gateway sends acks with just id + status (no body, no error)
        let json = r#"{"id":1,"status":200}"#;
        let resp: GatewayResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, 1);
        assert_eq!(resp.status, 200);
        assert!(resp.body.is_none());
        assert!(resp.error.is_none());
    }

    #[test]
    fn gateway_response_deserializes_error() {
        let json = r#"{"id":0,"status":400,"error":{"code":"INVALID_REQUEST","message":"bad request"}}"#;
        let resp: GatewayResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, 400);
        assert!(resp.body.is_none());
        assert!(resp.error.is_some());
    }

    #[test]
    fn gateway_response_deserializes_with_null_body() {
        // Some providers/clients send null body explicitly
        let json = r#"{"id":1,"status":200,"body":null}"#;
        let resp: GatewayResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, 200);
        assert!(resp.body.is_none());
    }

    // --- ContentBlock ---

    #[test]
    fn content_block_text_roundtrip() {
        let block = ContentBlock {
            block_type: "text".to_string(), text: Some("Hello".to_string()),
            id: None, name: None, input: None, extra: serde_json::Map::new(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let deserialized: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.block_type, "text");
        assert_eq!(deserialized.text.unwrap(), "Hello");
    }

    #[test]
    fn content_block_tool_use_roundtrip() {
        let block = ContentBlock {
            block_type: "tool_use".to_string(), text: None,
            id: Some("call_1".to_string()), name: Some("bash".to_string()),
            input: Some(serde_json::json!({"cmd":"ls"})),
            extra: serde_json::Map::new(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let deserialized: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.block_type, "tool_use");
        assert_eq!(deserialized.id.unwrap(), "call_1");
    }

    // --- ThinkingConfig ---

    #[test]
    fn thinking_config_roundtrip() {
        let tc = ThinkingConfig {
            thinking_type: "enabled".to_string(),
            budget_tokens: Some(16000),
            reasoning_effort: Some("high".to_string()),
        };
        let json = serde_json::to_string(&tc).unwrap();
        let deserialized: ThinkingConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.thinking_type, "enabled");
        assert_eq!(deserialized.budget_tokens.unwrap(), 16000);
    }

    #[test]
    fn thinking_config_defaults() {
        let tc = ThinkingConfig {
            thinking_type: "disabled".to_string(),
            budget_tokens: None,
            reasoning_effort: None,
        };
        let json = serde_json::to_string(&tc).unwrap();
        let deserialized: ThinkingConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.thinking_type, "disabled");
        assert!(deserialized.budget_tokens.is_none());
    }

    // --- GatewayStreamClient ---

    #[test]
    fn validate_socket_returns_err_for_nonexistent() {
        let client = GatewayStreamClient::with_socket("/tmp/nonexistent-test-socket-12345.sock");
        assert!(client.validate_socket().is_err());
    }
}
