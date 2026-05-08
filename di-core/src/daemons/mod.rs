use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use anyhow::{Result, anyhow};
use reqwest::Client;

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

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiGatewayRequest {
    pub model: String,
    pub messages: Vec<serde_json::Value>,
    pub stream: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse {
    pub calls: Vec<String>,
    pub definitions: Vec<String>,
}
