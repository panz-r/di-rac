use super::response::{Recoverability, ToolError, ToolErrorCode};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Routing decision — what the agent loop should do with an error
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum ToolErrorRoute {
    Retry {
        backoff_ms: u64,
        reason: String,
    },
    Abort {
        reason: String,
    },
    Continue {
        reason: String,
    },
    Escalate {
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Routing context — state the router needs to make a decision
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[derive(Default)]
pub struct RoutingContext {
    pub retry_count_for_error: usize,
}


// ---------------------------------------------------------------------------
// ErrorRouter — produces routing decisions from structured errors
// ---------------------------------------------------------------------------

pub struct ErrorRouter {
    /// Tracks retry count per (tool_name, error_code) to prevent infinite loops.
    retry_counters: HashMap<String, usize>,
}

impl ErrorRouter {
    pub fn new() -> Self {
        Self {
            retry_counters: HashMap::new(),
        }
    }

    /// Route a tool error to a decision.
    pub fn route(&mut self, error: &ToolError, ctx: &RoutingContext) -> ToolErrorRoute {
        let input_hash = error.metadata.input_hash.as_deref().unwrap_or("");
        let key = format!("{}:{}:{}", error.metadata.tool_name, error.code.as_str(), input_hash);

        // Same-input guard: if same error + same tool hit ≥2 times, route by recoverability
        let total_retries = *self.retry_counters.get(&key).unwrap_or(&0) + ctx.retry_count_for_error;
        if total_retries >= 2 {
            self.retry_counters.remove(&key);
            return match error.recoverability {
                Recoverability::Retryable => ToolErrorRoute::Continue {
                    reason: format!("Same input failed {} times with retryable error. Reporting to model for replanning.", total_retries),
                },
                Recoverability::RequiresUserInput => ToolErrorRoute::Escalate {
                    reason: format!("Requires user input (repeated {} times): {}", total_retries, error.message),
                },
                _ => ToolErrorRoute::Abort {
                    reason: format!("Same error repeated {} times: {}", total_retries, error.code.as_str()),
                },
            };
        }

        let route = self.route_inner(error, ctx);

        // Track retry decisions
        if matches!(route, ToolErrorRoute::Retry { .. }) {
            *self.retry_counters.entry(key.clone()).or_insert(0) += 1;
        } else {
            self.retry_counters.remove(&key);
        }

        route
    }

    fn route_inner(&self, error: &ToolError, ctx: &RoutingContext) -> ToolErrorRoute {
        // Route by recoverability first (the most important axis)
        match error.recoverability {
            Recoverability::NonRetryable => ToolErrorRoute::Continue {
                reason: format!("Non-retryable error: {}", error.code.as_str()),
            },

            Recoverability::RequiresUserInput => ToolErrorRoute::Escalate {
                reason: format!("Requires user input: {}", error.message),
            },

            Recoverability::RequiresReplan => ToolErrorRoute::Abort {
                reason: format!("Plan is stale: {}", error.message),
            },

            Recoverability::Retryable => {
                let max = default_max_retries(&error.code);
                if ctx.retry_count_for_error < max {
                    let backoff = default_backoff_ms(&error.code, ctx.retry_count_for_error);
                    ToolErrorRoute::Retry {
                        backoff_ms: backoff,
                        reason: format!("Retryable: {}", error.code.as_str()),
                    }
                } else {
                    ToolErrorRoute::Abort {
                        reason: format!("Retry limit ({}) exceeded for {}", max, error.code.as_str()),
                    }
                }
            }

            Recoverability::RetryableAfterRefresh => {
                ToolErrorRoute::Continue {
                    reason: format!("Context may be stale, returning to model for re-read: {}", error.code.as_str()),
                }
            }
        }
    }

}

fn default_max_retries(code: &ToolErrorCode) -> usize {
    match code {
        ToolErrorCode::DaemonUnavailable => 3,
        ToolErrorCode::DaemonTimeout => 2,
        ToolErrorCode::ShellTimeout => 2,
        ToolErrorCode::RateLimited => 3,
        _ => 1,
    }
}

fn default_backoff_ms(code: &ToolErrorCode, attempt: usize) -> u64 {
    let base = match code {
        ToolErrorCode::RateLimited => 2000,
        ToolErrorCode::DaemonUnavailable => 1000,
        _ => 500,
    };
    // Exponential backoff: base * 2^attempt, capped at 4000
    std::cmp::min(base * 2u64.pow(attempt as u32), 4000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::response::*;

    fn make_error(recoverability: Recoverability, input_hash: &str) -> ToolError {
        ToolError::new(ToolErrorCode::DaemonTimeout, "test error", "test_tool")
            .with_recoverability(recoverability)
            .with_input_hash(input_hash.to_string())
    }

    #[test]
    fn same_input_retryable_continues_not_aborts() {
        let mut router = ErrorRouter::new();
        let error = make_error(Recoverability::Retryable, "hash1");

        // First call: normal retry
        let ctx = RoutingContext { retry_count_for_error: 0 };
        let route1 = router.route(&error, &ctx);
        assert!(matches!(route1, ToolErrorRoute::Retry { .. }));

        // Second call with high retry count: same-input guard triggers
        let ctx2 = RoutingContext { retry_count_for_error: 2 };
        let route2 = router.route(&error, &ctx2);
        match route2 {
            ToolErrorRoute::Continue { .. } => {},
            other => panic!("Expected Continue for retryable same-input, got {:?}", other),
        }
    }

    #[test]
    fn same_input_user_input_escalates() {
        let mut router = ErrorRouter::new();
        let error = make_error(Recoverability::RequiresUserInput, "hash2");

        let ctx = RoutingContext { retry_count_for_error: 2 };
        let route = router.route(&error, &ctx);
        match route {
            ToolErrorRoute::Escalate { .. } => {},
            other => panic!("Expected Escalate for RequiresUserInput same-input, got {:?}", other),
        }
    }

    #[test]
    fn same_input_non_retryable_aborts() {
        let mut router = ErrorRouter::new();
        let error = make_error(Recoverability::NonRetryable, "hash3");

        let ctx = RoutingContext { retry_count_for_error: 2 };
        let route = router.route(&error, &ctx);
        match route {
            ToolErrorRoute::Abort { .. } => {},
            other => panic!("Expected Abort for NonRetryable same-input, got {:?}", other),
        }
    }
}
