use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// ToolResponse — discriminated union replacing Result<Value>
// ---------------------------------------------------------------------------

/// The result of a tool execution.
///
/// `Success` means the tool performed its operation. The domain result may
/// still be negative (e.g., tests failed, search returned 0 matches).
///
/// `Failure` means the tool could not perform its operation (daemon down,
/// permission denied, timeout, malformed input).
#[derive(Debug, Clone)]
pub enum ToolResponse {
    Success {
        data: serde_json::Value,
        #[allow(dead_code)]
        metadata: Option<ResponseMetadata>,
    },
    Failure {
        error: ToolError,
        #[allow(dead_code)]
        metadata: Option<ResponseMetadata>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMetadata {
    pub tool_name: String,
    pub call_id: String,
    pub timestamp: DateTime<Utc>,
    pub input_hash: Option<String>,
}

// ---------------------------------------------------------------------------
// ToolError — structured error with code, severity, recoverability
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ToolError {
    pub code: ToolErrorCode,
    /// Human-readable message for logging. NOT shown to the LLM directly.
    pub message: String,
    pub severity: ErrorSeverity,
    pub recoverability: Recoverability,
    pub details: Option<serde_json::Value>,
    pub remediation: Option<Remediation>,
    pub metadata: ErrorMetadata,
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.code, self.severity, self.message)
    }
}

impl ToolError {
    pub fn recoverability_str(&self) -> &'static str {
        self.recoverability.as_str()
    }
}

// ---------------------------------------------------------------------------
// ToolErrorCode — categorized error taxonomy
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolErrorCode {
    // IO
    IoFileNotFound,
    IoFilePermissionDenied,
    IoFileChanged,

    // Editing
    AnchorNotFound,
    AnchorAmbiguous,
    PatchApplyFailed,
    PatchConflict,

    // Shell
    ShellSpawnFailed,
    ShellTimeout,
    ShellBlocked,

    // Daemon / Infrastructure
    DaemonUnavailable,
    DaemonTimeout,

    // Validation
    MissingArgument,
    InvalidInput,

    // Context
    ContextStale,

    // General
    ToolInternalError,
    RateLimited,
    Unknown,
}

impl ToolErrorCode {
    /// Dotted string representation for logging and LLM formatting lookup.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::IoFileNotFound => "io.file.notFound",
            Self::IoFilePermissionDenied => "io.file.permissionDenied",
            Self::IoFileChanged => "io.file.changed",
            Self::AnchorNotFound => "anchor.notFound",
            Self::AnchorAmbiguous => "anchor.ambiguous",
            Self::PatchApplyFailed => "patch.applyFailed",
            Self::PatchConflict => "patch.conflict",
            Self::ShellSpawnFailed => "shell.spawnFailed",
            Self::ShellTimeout => "shell.timeout",
            Self::ShellBlocked => "shell.blocked",
            Self::DaemonUnavailable => "daemon.unavailable",
            Self::DaemonTimeout => "daemon.timeout",
            Self::MissingArgument => "validation.missingArgument",
            Self::InvalidInput => "validation.invalidInput",
            Self::ContextStale => "context.stale",
            Self::ToolInternalError => "tool.internalError",
            Self::RateLimited => "tool.rateLimited",
            Self::Unknown => "unknown",
        }
    }
}

impl Recoverability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Retryable => "retryable",
            Self::RetryableAfterRefresh => "retryable_after_refresh",
            Self::RequiresReplan => "requires_replan",
            Self::RequiresUserInput => "requires_user_input",
            Self::NonRetryable => "non_retryable",
        }
    }
}

impl fmt::Display for ToolErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// ErrorSeverity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorSeverity {
    Warning,
    Error,
    Critical,
}

impl fmt::Display for ErrorSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Critical => "critical",
        }
        .fmt(f)
    }
}

// ---------------------------------------------------------------------------
// Recoverability
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Recoverability {
    /// Retry the same call.
    Retryable,
    /// Re-read file / refresh context first, then retry once.
    RetryableAfterRefresh,
    /// Current plan is stale, need to replan.
    RequiresReplan,
    /// Need user input to proceed.
    RequiresUserInput,
    /// Permanent failure, do not retry.
    NonRetryable,
}

// ---------------------------------------------------------------------------
// Remediation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Remediation {
    pub strategy: RemediationStrategy,
    pub suggested_tools: Vec<String>,
    pub retry_delay_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemediationStrategy {
    RetrySame,
    RefreshContext,
    ReadFile,
    SearchRepo,
    Replan,
    AskUser,
    Abort,
}

// ---------------------------------------------------------------------------
// ErrorMetadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorMetadata {
    pub tool_name: String,
    pub timestamp: DateTime<Utc>,
    pub retry_count: usize,
    pub input_hash: Option<String>,
}

// ---------------------------------------------------------------------------
// Constructors
// ---------------------------------------------------------------------------

impl ToolError {
    pub fn new(code: ToolErrorCode, message: impl Into<String>, tool_name: &str) -> Self {
        Self {
            code,
            message: message.into(),
            severity: code.default_severity(),
            recoverability: code.default_recoverability(),
            details: None,
            remediation: code.default_remediation(),
            metadata: ErrorMetadata {
                tool_name: tool_name.to_string(),
                timestamp: Utc::now(),
                retry_count: 0,
                input_hash: None,
            },
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    #[cfg(test)]
    pub fn with_recoverability(mut self, rec: Recoverability) -> Self {
        self.recoverability = rec;
        self
    }

    #[cfg(test)]
    pub fn with_input_hash(mut self, hash: String) -> Self {
        self.metadata.input_hash = Some(hash);
        self
    }
}

impl ToolErrorCode {
    fn default_severity(&self) -> ErrorSeverity {
        match self {
            Self::IoFileNotFound | Self::ContextStale => ErrorSeverity::Warning,
            Self::ToolInternalError | Self::DaemonUnavailable => ErrorSeverity::Critical,
            _ => ErrorSeverity::Error,
        }
    }

    fn default_recoverability(&self) -> Recoverability {
        match self {
            Self::ShellTimeout | Self::DaemonTimeout | Self::RateLimited | Self::DaemonUnavailable => Recoverability::Retryable,
            Self::AnchorNotFound | Self::PatchConflict | Self::IoFileChanged => Recoverability::RetryableAfterRefresh,
            Self::MissingArgument | Self::InvalidInput | Self::ToolInternalError | Self::IoFilePermissionDenied => Recoverability::NonRetryable,
            Self::ShellSpawnFailed | Self::IoFileNotFound | Self::ShellBlocked | Self::PatchApplyFailed | Self::AnchorAmbiguous | Self::ContextStale | Self::Unknown => Recoverability::NonRetryable,
        }
    }

    fn default_remediation(&self) -> Option<Remediation> {
        match self {
            Self::AnchorNotFound => Some(Remediation {
                strategy: RemediationStrategy::ReadFile,
                suggested_tools: vec!["read".into()],
                retry_delay_ms: None,
            }),
            Self::IoFileNotFound => Some(Remediation {
                strategy: RemediationStrategy::SearchRepo,
                suggested_tools: vec!["search".into(), "repo".into()],
                retry_delay_ms: None,
            }),
            Self::ShellTimeout => Some(Remediation {
                strategy: RemediationStrategy::RetrySame,
                suggested_tools: vec![],
                retry_delay_ms: Some(2000),
            }),
            Self::DaemonUnavailable => Some(Remediation {
                strategy: RemediationStrategy::RetrySame,
                suggested_tools: vec![],
                retry_delay_ms: Some(1000),
            }),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Legacy conversion
// ---------------------------------------------------------------------------

impl ToolResponse {
    /// Wrap a legacy `Result<Value>` into a `ToolResponse`.
    /// `Err` becomes `Failure` with `ToolInternalError`.
    /// `Ok` becomes `Success`.
    pub fn from_result(result: anyhow::Result<serde_json::Value>, tool_name: &str) -> Self {
        match result {
            Ok(data) => Self::Success { data, metadata: None },
            Err(e) => {
                let msg = e.to_string();
                let code = classify_legacy_error(&msg);
                Self::Failure {
                    error: ToolError::new(code, msg, tool_name),
                    metadata: None,
                }
            }
        }
    }

    /// Create a failure from a known error code.
    pub fn fail(code: ToolErrorCode, message: impl Into<String>, tool_name: &str) -> Self {
        Self::Failure {
            error: ToolError::new(code, message, tool_name),
            metadata: None,
        }
    }

    /// Create a success with data.
    pub fn ok(data: serde_json::Value) -> Self {
        Self::Success { data, metadata: None }
    }

}

/// Classify a legacy `anyhow` error message into a structured error code.
fn classify_legacy_error(msg: &str) -> ToolErrorCode {
    let lower = msg.to_lowercase();
    if lower.contains("file not found") || lower.contains("enoent") {
        ToolErrorCode::IoFileNotFound
    } else if lower.contains("permission denied") {
        ToolErrorCode::IoFilePermissionDenied
    } else if lower.contains("timeout") || lower.contains("timed out") {
        ToolErrorCode::DaemonTimeout
    } else if lower.contains("rate limit") {
        ToolErrorCode::RateLimited
    } else if lower.contains("missing") && (lower.contains("argument") || lower.contains("param")) {
        ToolErrorCode::MissingArgument
    } else if lower.contains("anchor") && lower.contains("not found") {
        ToolErrorCode::AnchorNotFound
    } else if lower.contains("connection refused") || lower.contains("daemon") {
        ToolErrorCode::DaemonUnavailable
    } else {
        ToolErrorCode::ToolInternalError
    }
}
