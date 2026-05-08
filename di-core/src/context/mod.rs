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
                    true // Always include summaries if no APIs tagged
                }
            })
            .collect()
    }
}

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

    /// Implement the MEMENTO Memory Pyramid (Wang et al. 2025).
    pub fn build_prompt(&self, system_prompt: &str, trajectory: &Trajectory, current_apis: &HashSet<String>) -> Vec<Message> {
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

        // 3. Conversation History (Layer 0: Raw Recency)
        // Keep the last ~20 messages or within token budget
        let mut budget = self.token_limit - 4000; // Leave headroom for system prompt and observations
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
