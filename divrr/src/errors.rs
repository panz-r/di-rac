use std::fmt;

/// Errors from clipboard copy operations.
#[derive(Debug)]
pub enum ClipboardError {
    NoMethod,
    XclipFailed(String),
    ArboardFailed(String),
    WlCopyFailed(String),
    Osc52Failed(String),
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoMethod => write!(f, "No clipboard method available (tried xclip, arboard, wl-copy, OSC 52)"),
            Self::XclipFailed(e) => write!(f, "xclip failed: {}", e),
            Self::ArboardFailed(e) => write!(f, "arboard failed: {}", e),
            Self::WlCopyFailed(e) => write!(f, "wl-copy failed: {}", e),
            Self::Osc52Failed(e) => write!(f, "OSC 52 failed: {}", e),
        }
    }
}

impl std::error::Error for ClipboardError {}

/// Errors from sending messages to di-core.
#[derive(Debug)]
pub enum SendError {
    ChannelClosed,
    Io(std::io::Error),
    Timeout(String),
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChannelClosed => write!(f, "channel closed"),
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::Timeout(msg) => write!(f, "timeout: {}", msg),
        }
    }
}

impl std::error::Error for SendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

/// Errors from API gateway communication.
#[derive(Debug)]
pub enum GatewayError {
    ConnectionFailed(std::io::Error),
    RequestFailed(String),
    Timeout,
    ParseError(String),
}

impl fmt::Display for GatewayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectionFailed(e) => write!(f, "connection failed: {}", e),
            Self::RequestFailed(msg) => write!(f, "request failed: {}", msg),
            Self::Timeout => write!(f, "gateway request timed out"),
            Self::ParseError(msg) => write!(f, "parse error: {}", msg),
        }
    }
}

impl std::error::Error for GatewayError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ConnectionFailed(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for GatewayError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::TimedOut => Self::Timeout,
            _ => Self::ConnectionFailed(e),
        }
    }
}
