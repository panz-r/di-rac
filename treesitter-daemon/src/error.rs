use serde::Serialize;
use std::fmt;

/// Structured error codes as defined in the API contract.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    ParseError,
    UnsupportedLanguage,
    FileNotFound,
    InvalidCommand,
    InternalError,
    PathOutsideWorkspace,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorCode::ParseError => write!(f, "PARSE_ERROR"),
            ErrorCode::UnsupportedLanguage => write!(f, "UNSUPPORTED_LANGUAGE"),
            ErrorCode::FileNotFound => write!(f, "FILE_NOT_FOUND"),
            ErrorCode::InvalidCommand => write!(f, "INVALID_COMMAND"),
            ErrorCode::InternalError => write!(f, "INTERNAL_ERROR"),
            ErrorCode::PathOutsideWorkspace => write!(f, "PATH_OUTSIDE_WORKSPACE"),
        }
    }
}

/// Structured error details, serializable to JSON.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
}

/// The top-level error structure returned in the JSON response.
#[derive(Debug, Clone, Serialize)]
pub struct AnalyzerError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<ErrorDetails>,
}

impl AnalyzerError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), details: None }
    }

    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ParseError, message)
    }

    pub fn unsupported_language(lang: impl Into<String>) -> Self {
        Self::new(ErrorCode::UnsupportedLanguage, format!("Unsupported language: {}", lang.into()))
    }

    pub fn file_not_found(path: impl Into<String>) -> Self {
        Self::new(ErrorCode::FileNotFound, format!("File not found: {}", path.into()))
    }

    pub fn invalid_command(cmd: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidCommand, format!("Invalid command: {}", cmd.into()))
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InternalError, message)
    }

    pub fn path_outside_workspace(path: impl Into<String>) -> Self {
        Self::new(ErrorCode::PathOutsideWorkspace, format!("Path outside workspace root: {}", path.into()))
    }

    /// Serialize to a JSON error response, optionally including a request id.
    pub fn to_json_response(&self, request_id: Option<&serde_json::Value>) -> String {
        let mut map = serde_json::json!({
            "ok": false,
            "error": {
                "code": self.code,
                "message": self.message,
                "details": self.details,
            }
        });
        if let Some(id) = request_id {
            map["id"] = id.clone();
        }
        serde_json::to_string(&map).unwrap_or_else(|_| {
            r#"{"ok":false,"error":{"code":"INTERNAL_ERROR","message":"Failed to serialize error"}}"#.to_string()
        })
    }
}

impl fmt::Display for AnalyzerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for AnalyzerError {}

impl From<anyhow::Error> for AnalyzerError {
    fn from(err: anyhow::Error) -> Self {
        if let Some(ae) = err.downcast_ref::<AnalyzerError>() {
            return ae.clone();
        }
        AnalyzerError::internal_error(format!("{:#}", err))
    }
}
