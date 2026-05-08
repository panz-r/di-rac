use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorCategory {
    Transient,
    Permanent,
}

#[derive(Debug, Clone)]
pub enum RecoveryAction {
    Retry { max_attempts: usize, delay: Duration },
    Escalate(&'static str),
    Fail(String),
}

struct RecoveryRule {
    pattern: &'static str,
    category: ErrorCategory,
    max_attempts: usize,
    delay_ms: u64,
    escalate_msg: Option<&'static str>,
}

const RECOVERY_RULES: &[RecoveryRule] = &[
    RecoveryRule {
        pattern: "file_locked",
        category: ErrorCategory::Transient,
        max_attempts: 3,
        delay_ms: 1000,
        escalate_msg: None,
    },
    RecoveryRule {
        pattern: "rate_limited",
        category: ErrorCategory::Transient,
        max_attempts: 3,
        delay_ms: 500,
        escalate_msg: None,
    },
    RecoveryRule {
        pattern: "timeout",
        category: ErrorCategory::Transient,
        max_attempts: 1,
        delay_ms: 2000,
        escalate_msg: None,
    },
    RecoveryRule {
        pattern: "econnreset",
        category: ErrorCategory::Transient,
        max_attempts: 2,
        delay_ms: 1000,
        escalate_msg: None,
    },
    RecoveryRule {
        pattern: "file_not_found",
        category: ErrorCategory::Permanent,
        max_attempts: 0,
        delay_ms: 0,
        escalate_msg: Some("File not found — check path and retry"),
    },
    RecoveryRule {
        pattern: "permission_denied",
        category: ErrorCategory::Permanent,
        max_attempts: 0,
        delay_ms: 0,
        escalate_msg: Some("Permission denied — check file permissions"),
    },
    RecoveryRule {
        pattern: "enoent",
        category: ErrorCategory::Permanent,
        max_attempts: 0,
        delay_ms: 0,
        escalate_msg: Some("Path does not exist"),
    },
];

pub struct RecoveryEngine {
    retry_counts: std::collections::HashMap<String, usize>,
}

impl RecoveryEngine {
    pub fn new() -> Self {
        Self {
            retry_counts: std::collections::HashMap::new(),
        }
    }

    pub fn handle_error(&mut self, tool: &str, error: &str) -> RecoveryAction {
        let error_lower = error.to_lowercase();

        for rule in RECOVERY_RULES {
            if error_lower.contains(rule.pattern) {
                if rule.category == ErrorCategory::Transient {
                    let key = format!("{}:{}", tool, rule.pattern);
                    let count = self.retry_counts.entry(key.clone()).or_insert(0);
                    if *count < rule.max_attempts {
                        *count += 1;
                        return RecoveryAction::Retry {
                            max_attempts: rule.max_attempts,
                            delay: Duration::from_millis(rule.delay_ms),
                        };
                    } else {
                        self.retry_counts.remove(&key);
                        return RecoveryAction::Fail(format!(
                            "Retry limit ({}) exceeded for {}: {}", rule.max_attempts, tool, error
                        ));
                    }
                }
                if let Some(msg) = rule.escalate_msg {
                    return RecoveryAction::Escalate(msg);
                }
                return RecoveryAction::Fail(error.to_string());
            }
        }

        RecoveryAction::Fail(error.to_string())
    }

    pub fn reset(&mut self) {
        self.retry_counts.clear();
    }
}
