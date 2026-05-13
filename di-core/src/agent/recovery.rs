//! Deterministic recovery engine for tool execution.
//!
//! Provides pre-flight checks, post-execution analysis, and circuit breakers
//! matching the TS recovery.ts behavior.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

// ---------------------------------------------------------------------------
// Truncation detection
// ---------------------------------------------------------------------------

/// Patterns that indicate the model truncated its own output.
const TRUNCATION_PATTERNS: &[&str] = &[
    "... rest",
    "... remaining",
    "... everything above",
    "... same as above",
    "... (no changes)",
    // ... rest (in any comment style)
];

/// Check if content contains truncation placeholders.
pub fn detect_truncation(content: &str) -> bool {
    let lower = content.to_lowercase();
    TRUNCATION_PATTERNS.iter().any(|p| lower.contains(p))
        || content.contains("/* ... */")
        || content.contains("/* ... rest")
}

// ---------------------------------------------------------------------------
// Bash mutation detection
// ---------------------------------------------------------------------------

/// Patterns indicating a bash command mutates the filesystem or installs packages.
const BASH_MUTATION_PATTERNS: &[(&str, &str)] = &[
    (r"\bsed\s+.*-i", "sed in-place edit"),
    (r"\brm\s+-", "remove files"),
    (r"\bmv\s+", "move/rename files"),
    (r"\bcp\s+", "copy files"),
    (r"\bmkdir\s+", "create directory"),
    (r"\btouch\s+", "create/touch file"),
    (r"\bchmod\s+", "change permissions"),
    (r"\bchown\s+", "change ownership"),
    (r"\bnpm\s+(install|uninstall|update|add|remove)", "npm package mutation"),
    (r"\bpip\s+(install|uninstall)", "pip package mutation"),
    (r"\bcargo\s+(add|remove|install)", "cargo package mutation"),
    (r"\bgo\s+(get|install|mod\s+tidy)", "go package mutation"),
    (r"\bmake\b", "make build"),
    (r">\s*\S", "redirect to file"),
    (r">>\s*\S", "append to file"),
    (r"\|\s*tee\b", "pipe to tee"),
];

/// Check if a bash command contains mutation patterns.
/// Returns descriptions of detected mutations.
pub fn detect_bash_mutations(command: &str) -> Vec<&'static str> {
    BASH_MUTATION_PATTERNS.iter()
        .filter_map(|(pattern, label)| {
            if regex::Regex::new(pattern).ok().map(|re| re.is_match(command)).unwrap_or(false) {
                Some(*label)
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Excessive deletion guard
// ---------------------------------------------------------------------------

/// Maximum number of lines that can be deleted in a single edit.
const MAX_ABSOLUTE_DELETION: usize = 150;

/// Check if an edit exceeds the excessive deletion threshold.
pub fn is_excessive_deletion(lines_removed: usize) -> bool {
    lines_removed > MAX_ABSOLUTE_DELETION
}

// ---------------------------------------------------------------------------
// Circuit breaker
// ---------------------------------------------------------------------------

/// Per-tool circuit breaker state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

/// Per-tool circuit breaker.
pub struct CircuitBreaker {
    state: CircuitState,
    consecutive_failures: u32,
    opened_at: Option<Instant>,
    threshold: u32,
    cooldown_secs: u64,
}

impl CircuitBreaker {
    pub fn new() -> Self {
        Self {
            state: CircuitState::Closed,
            consecutive_failures: 0,
            opened_at: None,
            threshold: 3,
            cooldown_secs: 30,
        }
    }

    /// Record a successful tool execution.
    pub fn record_success(&mut self) {
        self.state = CircuitState::Closed;
        self.consecutive_failures = 0;
    }

    /// Record a failed tool execution.
    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= self.threshold {
            self.state = CircuitState::Open;
            self.opened_at = Some(Instant::now());
        }
    }

    /// Check if the circuit allows execution.
    /// Open circuits transition to HalfOpen after cooldown.
    pub fn allow_execution(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true,
            CircuitState::Open => {
                if let Some(opened) = self.opened_at {
                    if opened.elapsed().as_secs() >= self.cooldown_secs {
                        self.state = CircuitState::HalfOpen;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
        }
    }

    pub fn state(&self) -> CircuitState {
        self.state
    }

    pub fn failures(&self) -> u32 {
        self.consecutive_failures
    }
}

/// Registry of per-tool circuit breakers.
pub struct CircuitBreakerRegistry {
    breakers: HashMap<String, CircuitBreaker>,
}

impl CircuitBreakerRegistry {
    pub fn new() -> Self {
        Self {
            breakers: HashMap::new(),
        }
    }

    pub fn record_success(&mut self, tool: &str) {
        self.breakers.entry(tool.to_string())
            .or_insert_with(CircuitBreaker::new)
            .record_success();
    }

    pub fn record_failure(&mut self, tool: &str) {
        self.breakers.entry(tool.to_string())
            .or_insert_with(CircuitBreaker::new)
            .record_failure();
    }

    pub fn allow_execution(&mut self, tool: &str) -> bool {
        self.breakers.entry(tool.to_string())
            .or_insert_with(CircuitBreaker::new)
            .allow_execution()
    }

    pub fn state(&self, tool: &str) -> CircuitState {
        self.breakers.get(tool).map(|b| b.state()).unwrap_or(CircuitState::Closed)
    }
}

// ---------------------------------------------------------------------------
// Stagnation detection
// ---------------------------------------------------------------------------

/// Track recent tool calls for loop detection.
#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub tool: String,
    pub args_hash: String,
}

/// Detect stagnation patterns in recent tool calls.
pub struct StagnationDetector {
    history: Vec<ToolCallRecord>,
    max_history: usize,
}

impl StagnationDetector {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            max_history: 20,
        }
    }

    /// Record a tool call and return a stagnation warning if detected.
    pub fn record(&mut self, tool: &str, args_hash: &str) -> Option<String> {
        self.history.push(ToolCallRecord {
            tool: tool.to_string(),
            args_hash: args_hash.to_string(),
        });
        if self.history.len() > self.max_history {
            self.history.remove(0);
        }

        let n = self.history.len();
        if n < 3 {
            return None;
        }

        // Repeated identical calls (3x)
        let last = &self.history[n - 1];
        let count = self.history.iter().rev().take(6)
            .filter(|r| r.tool == last.tool && r.args_hash == last.args_hash)
            .count();
        if count >= 3 {
            return Some(format!(
                "Repeated identical call to {} ({}x). Loop broken.",
                last.tool, count
            ));
        }

        // Alternating A-B-A-B pattern
        if n >= 4 {
            let a = &self.history[n - 1];
            let b = &self.history[n - 2];
            let c = &self.history[n - 3];
            let d = &self.history[n - 4];
            if a.tool == c.tool && a.args_hash == c.args_hash
                && b.tool == d.tool && b.args_hash == d.args_hash
                && a.tool != b.tool
            {
                return Some(format!(
                    "Alternating loop detected between {} and {}. Strategy thrashing.",
                    a.tool, b.tool
                ));
            }
        }

        // Circular A-B-C-A-B-C pattern
        if n >= 6 {
            let is_circular = (0..3).all(|i| {
                let x = &self.history[n - 1 - i];
                let y = &self.history[n - 4 - i];
                x.tool == y.tool && x.args_hash == y.args_hash
            });
            if is_circular {
                let tools: Vec<&str> = self.history.iter().rev().take(3).map(|r| r.tool.as_str()).collect();
                return Some(format!(
                    "Circular strategy loop detected ({}). Consider a different approach.",
                    tools.join(" -> ")
                ));
            }
        }

        None
    }
}

// ---------------------------------------------------------------------------
// Telemetry
// ---------------------------------------------------------------------------

/// Recovery engine telemetry counters.
pub struct RecoveryTelemetry {
    pub intercepted_count: AtomicU32,
    pub escalated_count: AtomicU32,
    pub blocked_count: AtomicU32,
    pub recovered_count: AtomicU32,
    pub turn_savings: AtomicU32,
}

impl RecoveryTelemetry {
    pub fn new() -> Self {
        Self {
            intercepted_count: AtomicU32::new(0),
            escalated_count: AtomicU32::new(0),
            blocked_count: AtomicU32::new(0),
            recovered_count: AtomicU32::new(0),
            turn_savings: AtomicU32::new(0),
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "[Deterministic Recovery Summary]\n- Intercepted: {} errors\n- Escalated: {} errors\n- Blocked: {} errors\n- Recovered: {} errors",
            self.intercepted_count.load(Ordering::Relaxed),
            self.escalated_count.load(Ordering::Relaxed),
            self.blocked_count.load(Ordering::Relaxed),
            self.recovered_count.load(Ordering::Relaxed),
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncation_detection() {
        assert!(detect_truncation("// ... rest of the code"));
        assert!(detect_truncation("# ... remaining code"));
        assert!(detect_truncation("/* ... */"));
        assert!(!detect_truncation("fn main() {}"));
    }

    #[test]
    fn test_bash_mutation_detection() {
        let mutations = detect_bash_mutations("sed -i 's/old/new/g' file.txt");
        assert!(mutations.contains(&"sed in-place edit"));

        let mutations = detect_bash_mutations("npm install express");
        assert!(mutations.contains(&"npm package mutation"));

        let mutations = detect_bash_mutations("echo hello");
        assert!(mutations.is_empty());
    }

    #[test]
    fn test_excessive_deletion() {
        assert!(!is_excessive_deletion(100));
        assert!(is_excessive_deletion(200));
    }

    #[test]
    fn test_circuit_breaker() {
        let mut cb = CircuitBreaker::new();
        assert!(cb.allow_execution());
        assert_eq!(cb.state(), CircuitState::Closed);

        cb.record_failure();
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow_execution());

        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_stagnation_repeated() {
        let mut det = StagnationDetector::new();
        assert!(det.record("read", "hash1").is_none());
        assert!(det.record("read", "hash1").is_none());
        let warning = det.record("read", "hash1");
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("Repeated identical"));
    }

    #[test]
    fn test_stagnation_alternating() {
        let mut det = StagnationDetector::new();
        det.record("read", "a");
        det.record("search", "b");
        det.record("read", "a");
        let warning = det.record("search", "b");
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("Alternating"));
    }
}
