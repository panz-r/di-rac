use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use anyhow::{Result, anyhow};
use reqwest::Client;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream as AsyncUnixStream;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Unix Daemon Client (synchronous UDS for C daemons)
// ---------------------------------------------------------------------------

pub struct UnixDaemonClient {
    socket_path: String,
}

impl UnixDaemonClient {
    pub fn new(socket_path: &str) -> Self {
        Self {
            socket_path: socket_path.to_string(),
        }
    }

    pub fn send_request<T: Serialize, R: for<'de> Deserialize<'de>>(&self, request: T) -> Result<R> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| anyhow!("Failed to connect to socket {}: {}", self.socket_path, e))?;

        let json = serde_json::to_string(&request)?;
        stream.write_all(json.as_bytes())?;
        stream.write_all(b"\n")?;

        let mut response = String::new();
        stream.read_to_string(&mut response)?;

        if response.is_empty() {
            return Err(anyhow!("Empty response from daemon {}", self.socket_path));
        }

        let result = serde_json::from_str(&response)
            .map_err(|e| anyhow!("Failed to parse response from {}: {}. Response: {}", self.socket_path, e, response))?;
        Ok(result)
    }

    pub fn extract_apis(&self, content: &str, language: &str) -> Result<ApiResponse> {
        self.send_request(AnalyzerRequest {
            command: "extract-apis".to_string(),
            file: None,
            content: Some(content.to_string()),
            language: Some(language.to_string()),
            query: None,
        })
    }
}

// ---------------------------------------------------------------------------
// HTTP Daemon Client (for non-streaming API calls)
// ---------------------------------------------------------------------------

pub struct HttpDaemonClient {
    url: String,
    client: Client,
}

impl HttpDaemonClient {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            client: Client::new(),
        }
    }

    pub async fn post<T: Serialize, R: for<'de> Deserialize<'de>>(&self, endpoint: &str, body: T) -> Result<R> {
        let full_url = format!("{}/{}", self.url, endpoint.trim_start_matches('/'));
        let resp = self.client.post(&full_url)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("HTTP error {}: {}", resp.status(), resp.text().await?));
        }

        let result = resp.json().await?;
        Ok(result)
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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnalyzerResponse {
    pub ok: bool,
    #[serde(flatten)]
    pub data: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Command daemon types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct CommandRequest {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommandResponse {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

// ---------------------------------------------------------------------------
// Central daemon types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct CentralRequest {
    pub command: String,
    pub key: Option<String>,
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CentralResponse {
    pub ok: bool,
    pub value: Option<serde_json::Value>,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// API Gateway types (NDJSON over UDS)
// Matches api-gateway/providers/provider.go and api-gateway/main.go
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
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
    pub content: serde_json::Value,
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
    pub content_blocks: Option<Vec<ContentBlock>>,
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GatewayResponse {
    pub id: i64,
    pub status: i64,
    #[serde(default)]
    pub body: Option<StreamChunk>,
    #[serde(default)]
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Gateway Stream Client (async NDJSON over UDS)
// ---------------------------------------------------------------------------

pub struct GatewayStreamClient {
    socket_path: String,
}

impl GatewayStreamClient {
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let socket_path = std::env::var("DIRAC_API_GATEWAY_SOCKET")
            .unwrap_or_else(|_| format!("{}/.dirac/api-gateway.sock", home));
        Self { socket_path }
    }

    pub fn with_socket(socket_path: &str) -> Self {
        Self { socket_path: socket_path.to_string() }
    }

    /// Send a streaming request to the api-gateway. Returns a channel
    /// receiver that yields StreamChunk values as they arrive.
    pub async fn stream_chat(
        &self,
        request: GatewayRequest,
    ) -> Result<mpsc::Receiver<Result<StreamChunk>>> {
        let (tx, rx) = mpsc::channel(usize::MAX);
        let socket_path = self.socket_path.clone();

        // Connect, write request, then hand off to async reader — single connection.
        tokio::spawn(async move {
            // Synchronous connect + write, then convert to async for reading.
            let stream = match std::os::unix::net::UnixStream::connect(&socket_path) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(Err(anyhow!("Failed to connect to gateway: {}", e))).await;
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

            use std::io::Write;
            let mut stream_ref = &stream;
            if let Err(e) = stream_ref.write_all(json.as_bytes()) {
                let _ = tx.send(Err(anyhow!("Failed to write to gateway: {}", e))).await;
                return;
            }
            if let Err(e) = stream_ref.write_all(b"\n") {
                let _ = tx.send(Err(anyhow!("Failed to write to gateway: {}", e))).await;
                return;
            }
            if let Err(e) = (&stream).flush() {
                let _ = tx.send(Err(anyhow!("Failed to flush gateway: {}", e))).await;
                return;
            }

            // Convert to async for line-by-line reading
            let async_stream = match AsyncUnixStream::from_std(stream) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(Err(anyhow!("Failed to convert to async: {}", e))).await;
                    return;
                }
            };

            let buf_reader = BufReader::new(async_stream);
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
                                    let _ = tx.send(Err(anyhow!("Gateway error {}: {}", resp.status, resp.error.unwrap_or_default()))).await;
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
