use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum UserMessageKind {
    InitialTask,
    Correction,
    Constraint,
    GoalChange,
    Clarification,
    Approval,
    Casual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Supersession {
    pub source_message_id: Uuid,
    pub kind: UserMessageKind,
    pub timestamp: DateTime<Utc>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub original_goal: String,
    pub current_goal: String,
    pub active_constraints: Vec<String>,
    pub superseded_by: Vec<Supersession>,
}

impl Default for TaskState {
    fn default() -> Self {
        Self {
            original_goal: String::new(),
            current_goal: String::new(),
            active_constraints: Vec::new(),
            superseded_by: Vec::new(),
        }
    }
}

pub struct TaskStateReducer {
    pub state: TaskState,
    message_count: usize,
}

impl TaskStateReducer {
    pub fn new() -> Self {
        Self {
            state: TaskState::default(),
            message_count: 0,
        }
    }

    /// Classify a user message by heuristic keyword matching.
    ///
    /// NOTE: Classification is currently English-only. Non-English messages
    /// will fall through to Clarification (priority 2) unless they are short
    /// enough to match Casual (priority 0). A future improvement would use
    /// the distiller model for classification when available.
    pub fn classify(&self, text: &str, is_first_user: bool) -> UserMessageKind {
        if is_first_user && self.message_count == 0 {
            return UserMessageKind::InitialTask;
        }

        let lower = text.to_lowercase();
        let word_count = lower.split_whitespace().count();

        // Priority-ordered checks
        if is_correction(&lower) { return UserMessageKind::Correction; }
        if is_goal_change(&lower) { return UserMessageKind::GoalChange; }
        if is_constraint(&lower) { return UserMessageKind::Constraint; }
        if is_approval(&lower, word_count) { return UserMessageKind::Approval; }
        if is_casual(&lower, word_count) { return UserMessageKind::Casual; }

        // Never mark longer unknown messages as expendable — they may be non-English
        // corrections or constraints that the English keyword matcher missed (fixes 2.1).
        // Constraint is additive and safer than GoalChange (which overwrites the goal).
        if word_count > 5 {
            UserMessageKind::Constraint
        } else {
            UserMessageKind::Clarification
        }
    }

    /// Update task state based on a classified message.
    pub fn update(&mut self, kind: UserMessageKind, text: &str, message_id: Uuid) {
        match kind {
            UserMessageKind::InitialTask => {
                self.state.original_goal = text.to_string();
                self.state.current_goal = text.to_string();
            }
            UserMessageKind::Correction => {
                self.state.active_constraints.push(format!("(correction) {}", text));
                self.state.superseded_by.push(Supersession {
                    source_message_id: message_id,
                    kind,
                    timestamp: Utc::now(),
                    summary: truncate_to(text, 200),
                });
            }
            UserMessageKind::Constraint => {
                self.state.active_constraints.push(text.to_string());
            }
            UserMessageKind::GoalChange => {
                self.state.current_goal = text.to_string();
                self.state.superseded_by.push(Supersession {
                    source_message_id: message_id,
                    kind,
                    timestamp: Utc::now(),
                    summary: truncate_to(text, 200),
                });
            }
            _ => {}
        }
        self.message_count += 1;
    }

    /// Classify and update in one call.
    pub fn process(&mut self, text: &str, is_first_user: bool) -> UserMessageKind {
        let kind = self.classify(text, is_first_user);
        let id = Uuid::new_v4();
        self.update(kind, text, id);
        kind
    }

    /// Produce a deterministic critical summary for injection as a System message.
    pub fn to_critical_summary(&self) -> String {
        if self.state.current_goal.is_empty() {
            return String::new();
        }

        let mut parts = Vec::new();
        parts.push(format!("Current Goal: {}", self.state.current_goal));

        if !self.state.active_constraints.is_empty() {
            parts.push("Active Constraints:".to_string());
            for c in &self.state.active_constraints {
                parts.push(format!("- {}", c));
            }
        }

        if self.state.original_goal != self.state.current_goal && !self.state.original_goal.is_empty() {
            let superseded = if self.state.superseded_by.len() > 1 {
                format!(" (superseded {} times)", self.state.superseded_by.len())
            } else {
                " (superseded)".to_string()
            };
            parts.push(format!("Original Goal: {}{}", self.state.original_goal, superseded));
        }

        parts.join("\n")
    }

    /// Produce a short tail reminder (200-500 tokens) for placement at the end
    /// of the system prompt. Counters "Lost in the Middle" positional fragility
    /// by duplicating goal, constraints, stale files, and latest failure.
    pub fn to_tail_reminder(&self, stale_files: &[String], latest_failure: Option<&str>) -> String {
        if self.state.current_goal.is_empty() {
            return String::new();
        }

        let mut parts = vec!["[REMINDER]".to_string()];
        parts.push(format!("Goal: {}", self.state.current_goal));

        // Truncate constraints to fit budget
        let mut constraint_text = String::new();
        for c in &self.state.active_constraints {
            if constraint_text.len() + c.len() + 2 > 300 { break; }
            constraint_text.push_str(&format!("\n- {}", c));
        }
        if !constraint_text.is_empty() {
            parts.push(format!("Constraints:{}", constraint_text));
        }

        if !stale_files.is_empty() {
            let files_str = stale_files.iter()
                .take(5)
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            parts.push(format!("Stale files (re-read before editing): {}", files_str));
        }

        if let Some(failure) = latest_failure {
            let truncated = if failure.len() > 150 {
                format!("{}...", &failure[..147])
            } else {
                failure.to_string()
            };
            parts.push(format!("Latest failure: {}", truncated));
        }

        parts.join("\n")
    }

    /// Priority for history selection based on message kind.
    pub fn priority_for_kind(kind: UserMessageKind) -> u8 {
        match kind {
            UserMessageKind::Correction | UserMessageKind::Constraint | UserMessageKind::GoalChange => 4,
            UserMessageKind::InitialTask => 3,
            UserMessageKind::Clarification => 2,
            UserMessageKind::Approval => 1, // Approvals don't contain task information
            UserMessageKind::Casual => 0,
        }
    }
}

fn is_correction(lower: &str) -> bool {
    let patterns = ["don't", "do not", "no not", "stop", "wrong", "incorrect",
        "not that", "i said", "i meant", "that's wrong", "actually no",
        "that's not", "bad approach", "terrible", "horrible", "awful"];
    patterns.iter().any(|p| lower.starts_with(p) || lower.contains(&format!(" {}", p)))
}

fn is_goal_change(lower: &str) -> bool {
    let patterns = ["instead", "actually i want", "change to", "forget about",
        "let's do", "new task", "pivot to", "now do", "switch to",
        "i want", "i need", "actually, let's"];
    patterns.iter().any(|p| lower.starts_with(p) || lower.contains(&format!(" {}", p)))
}

fn is_constraint(lower: &str) -> bool {
    let patterns = ["must use", "no external", "don't use", "must not",
        "require", "constraint", "only use", "make sure", "ensure that",
        "has to be", "needs to be", "cannot use", "forbidden", "not allowed",
        "no third", "no library", "important that"];
    patterns.iter().any(|p| lower.starts_with(p) || lower.contains(&format!(" {}", p)))
}

fn is_approval(lower: &str, word_count: usize) -> bool {
    if word_count > 5 { return false; }
    let approvals = ["yes", "ok", "okay", "go ahead", "approved", "do it",
        "sure", "yep", "correct", "right", "looks good", "proceed",
        "continue", "perfect", "exactly", "great", "fine"];
    approvals.iter().any(|p| lower == *p || lower.starts_with(p))
}

fn is_casual(lower: &str, word_count: usize) -> bool {
    if word_count > 3 { return false; }
    let casual = ["thanks", "thx", "cool", "nice", "great", "got it",
        "acknowledged", "uh huh", "mm", "np", "no problem"];
    casual.iter().any(|p| lower == *p || lower.starts_with(p))
}

fn truncate_to(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { format!("{}...", &s[..max - 3]) }
}
