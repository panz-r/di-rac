use crate::agent::trajectory::{Trajectory, Message, Role};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub id: Uuid,
    pub obs_type: String, // "summary" | "watcher" | "critic" | "skeleton"
    pub content: String,
    pub timestamp: i64,
    pub tokens: usize,
    pub confidence: f32,
    pub apis: Option<HashSet<String>>, // For API-intersection filtering
}

pub struct MemoryVault {
    pub observations: Vec<Observation>,
}

impl MemoryVault {
    pub fn new() -> Self {
        Self { observations: Vec::new() }
    }

    pub fn add_observation(&mut self, obs: Observation) {
        self.observations.push(obs);
    }

    pub fn get_relevant_observations(&self, current_apis: &HashSet<String>, min_confidence: f32) -> Vec<&Observation> {
        self.observations.iter()
            .filter(|o| o.confidence >= min_confidence)
            .filter(|o| {
                if let Some(ref apis) = o.apis {
                    !apis.is_empty() && !apis.is_disjoint(current_apis)
                } else {
                    true
                }
            })
            .collect()
    }
}

/// The prompt injected when the context is nearing capacity, instructing the
/// model to call the compact tool with a comprehensive summary.
const AUTO_COMPACT_INSTRUCTION: &str = "The conversation is nearing its context limit. You must now call the compact tool to create a comprehensive summary of the task's progress.\n\nYour summary must capture:\n- All user intents and requirements\n- Every technical finding, architectural decision, and code pattern\n- All files examined or modified, including critical code snippets\n- The precise current status and exact next steps\n\nThis summary will be your only context moving forward. You MUST ONLY respond by calling compact.";

pub struct ContextManager {
    pub token_limit: usize,
    pub compression_threshold: usize,
    pub vault: MemoryVault,
}

impl ContextManager {
    pub fn new(token_limit: usize, compression_threshold: usize) -> Self {
        Self {
            token_limit,
            compression_threshold,
            vault: MemoryVault::new(),
        }
    }

    /// Check if the trajectory exceeds the compression threshold and
    /// auto-compaction should be triggered.
    pub fn should_auto_compact(&self, trajectory: &Trajectory) -> bool {
        trajectory.get_total_tokens() >= self.compression_threshold
    }

    /// Build the auto-compact instruction message.
    pub fn auto_compact_instruction() -> &'static str {
        AUTO_COMPACT_INSTRUCTION
    }

    /// Build the continuation prompt from a compact summary.
    pub fn continuation_prompt(summary: &str) -> String {
        format!("This session is being continued from a previous conversation that ran out of context. The conversation is summarized below:\n\n{}\n\nPlease continue the conversation from where we left off without asking the user any further questions.", summary)
    }

    /// Implement the MEMENTO Memory Pyramid (Wang et al. 2025).
    /// `background_summary` is injected as a system message when background
    /// commands are running, ensuring IDs survive any context management.
    pub fn build_prompt(
        &self,
        system_prompt: &str,
        trajectory: &Trajectory,
        current_apis: &HashSet<String>,
        background_summary: Option<&str>,
    ) -> Vec<Message> {
        let mut messages = Vec::new();

        // 1. System Prompt (Always first)
        messages.push(Message {
            id: Uuid::new_v4(),
            role: Role::System,
            content: serde_json::json!(system_prompt),
            timestamp: chrono::Utc::now(),
            tokens: system_prompt.len() / 4,
            is_compressed: false,
        });

        // 2. Memory Pyramid - Layer 1 & 2 (Relevant Observations)
        let relevant_obs = self.vault.get_relevant_observations(current_apis, 0.5);
        if !relevant_obs.is_empty() {
            let obs_block = relevant_obs.iter()
                .map(|o| format!("[{}] {}", o.obs_type.to_uppercase(), o.content))
                .collect::<Vec<_>>()
                .join("\n\n");

            messages.push(Message {
                id: Uuid::new_v4(),
                role: Role::System,
                content: serde_json::json!(format!("# Past Observations\n\n{}", obs_block)),
                timestamp: chrono::Utc::now(),
                tokens: obs_block.len() / 4,
                is_compressed: false,
            });
        }

        // 3. Background Commands — always injected when present
        if let Some(summary) = background_summary {
            messages.push(Message {
                id: Uuid::new_v4(),
                role: Role::System,
                content: serde_json::json!(summary),
                timestamp: chrono::Utc::now(),
                tokens: summary.len() / 4,
                is_compressed: false,
            });
        }

        // 4. Conversation History (Layer 0: Raw Recency)
        // Keep the last ~20 messages or within token budget
        let mut budget = self.token_limit - 4000; // Leave headroom
        let mut history: Vec<Message> = trajectory.messages.iter()
            .rev()
            .take_while(|m| {
                if budget >= m.tokens {
                    budget -= m.tokens;
                    true
                } else {
                    false
                }
            })
            .cloned()
            .collect();

        history.reverse();
        messages.extend(history);

        messages
    }
}
