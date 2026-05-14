use crate::agent::trajectory::{Trajectory, Message, Role};
use crate::context::task_state::TaskStateReducer;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

pub mod token_estimator;
pub mod distiller;
pub mod task_state;
pub mod lifecycle_metrics;
pub mod adaptive_triggers;
pub mod lifecycle;
pub mod reranker;
#[cfg(test)]
pub mod eval;
pub use token_estimator::{TokenEstimator, ConservativeEstimator};
pub use lifecycle_metrics::{TurnMetrics, ToolCallRecord};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub id: Uuid,
    pub obs_type: String,
    pub content: String,
    pub timestamp: i64,
    pub tokens: usize,
    pub confidence: f32,
    pub apis: Option<HashSet<String>>,
}

pub struct MemoryVault {
    pub observations: Vec<Observation>,
}

impl MemoryVault {
    pub fn new() -> Self {
        Self { observations: Vec::new() }
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

pub struct ContextManager {
    #[allow(dead_code)]
    pub token_limit: usize,
    pub vault: MemoryVault,
}

impl ContextManager {
    pub fn new(token_limit: usize, _compression_threshold: usize) -> Self {
        Self {
            token_limit,
            vault: MemoryVault::new(),
        }
    }

    pub fn continuation_prompt(summary: &str) -> String {
        format!("This session is being continued from a previous conversation that ran out of context. The conversation is summarized below:\n\n{}\n\nContinue the task from where you left off. Do not ask the user for direction — they are waiting for you to make progress.", summary)
    }

    /// Build conversation messages with stale-read exclusion.
    /// `edited_files` is the set of files that have been edited during this session.
    /// `task_reducer` classifies user messages by semantic kind for priority assignment.
    /// Tool results containing reads of edited files are replaced with stale notices.
    pub fn build_prompt_with_stale_check(
        &self,
        trajectory: &Trajectory,
        edited_files: &HashSet<String>,
        task_reducer: Option<&TaskStateReducer>,
        history_budget: usize,
    ) -> Vec<Message> {
        let budget = history_budget;
        let msgs = &trajectory.messages;
        let mut oversized_first: Option<(usize, String, usize)> = None;

        if msgs.is_empty() {
            return Vec::new();
        }

        let first_user = msgs.iter().position(|m| matches!(m.role, Role::User));
        let last_assistant = msgs.iter().rposition(|m| matches!(m.role, Role::Assistant));

        let tool_indices: Vec<usize> = msgs.iter()
            .enumerate()
            .rev()
            .filter(|(_, m)| matches!(m.role, Role::Tool))
            .take(3)
            .map(|(i, _)| i)
            .collect();

        let recent_indices: Vec<usize> = {
            let mut indices = Vec::new();
            let mut pair_count = 0;
            for (i, _msg) in msgs.iter().enumerate().rev() {
                if matches!(msgs[i].role, Role::Assistant) {
                    pair_count += 1;
                    if pair_count > 5 { break; }
                }
                indices.push(i);
            }
            indices
        };

        // Build a priority map: higher = more important
        let mut priority = vec![0u8; msgs.len()];

        // Classify user messages by semantic kind
        for (i, msg) in msgs.iter().enumerate() {
            if matches!(msg.role, Role::User) {
                let is_first = first_user == Some(i);
                if let Some(reducer) = task_reducer {
                    let text = msg.content.to_string();
                    let kind = reducer.classify(&text, is_first);
                    priority[i] = TaskStateReducer::priority_for_kind(kind);
                } else {
                    // Fallback: first user = Critical, others = Important
                    priority[i] = if is_first { 4 } else { 3 };
                }
            }
        }

        // Critical: latest assistant intent
        if let Some(idx) = last_assistant {
            priority[idx] = 4;
        }
        // Important: latest tool results
        for &idx in &tool_indices {
            if priority[idx] < 3 {
                priority[idx] = 3;
            }
        }
        // Normal: recent turns
        for &idx in &recent_indices {
            if priority[idx] < 2 {
                priority[idx] = 2;
            }
        }

        // Fill budget by priority (4 → 3 → 2 → 1 → 0)
        let mut selected_indices: Vec<usize> = Vec::new();
        let mut used_tokens = 0usize;

        for level in (0..=4).rev() {
            for (i, _msg) in msgs.iter().enumerate() {
                if priority[i] != level {
                    continue;
                }
                if used_tokens + msgs[i].tokens <= budget {
                    selected_indices.push(i);
                    used_tokens += msgs[i].tokens;
                }
            }
        }

        // Guarantee: the initial task message is always included as backup for the TaskState summary.
        // If it was dropped, force-include it by evicting the lowest-priority non-first message.
        // If it's too large to fit even alone, use a truncated excerpt.
        if let Some(first_idx) = first_user {
            if !selected_indices.contains(&first_idx) {
                let first_tokens = msgs[first_idx].tokens;
                if first_tokens <= budget {
                    selected_indices.push(first_idx);
                    used_tokens += first_tokens;
                    while used_tokens > budget {
                        if let Some(drop_idx) = selected_indices.iter()
                            .filter(|&&i| i != first_idx)
                            .min_by_key(|&&i| (priority[i], std::cmp::Reverse(i)))
                        {
                            let drop_i = *drop_idx;
                            used_tokens -= msgs[drop_i].tokens;
                            selected_indices.retain(|&i| i != drop_i);
                        } else {
                            break;
                        }
                    }
                } else {
                    // First message is too large for the budget.
                    // Include a truncated excerpt. TaskState summary in the
                    // system prompt already carries the goal and constraints.
                    let content = msgs[first_idx].content.to_string();
                    let excerpt_len = (budget * 4).min(content.len()).max(0);
                    let excerpt = if content.len() > excerpt_len && excerpt_len > 0 {
                        format!("{}...\n\n[Initial task compacted: {} tokens originally. Task state available in system prompt.]",
                            &content[..excerpt_len], first_tokens)
                    } else {
                        format!("[Initial task compacted: {} tokens originally. Task state available in system prompt.]", first_tokens)
                    };
                    selected_indices.push(first_idx);
                    // Mark this index for special handling during result construction
                    oversized_first = Some((first_idx, excerpt, budget.min(500)));
                }
            }
        }

        // Sort by original order and rewrite stale reads
        selected_indices.sort();
        let result: Vec<Message> = selected_indices.into_iter().map(|i| {
            // Handle oversized first message: use truncated excerpt
            if let Some((fi, ref excerpt, excerpt_tokens)) = oversized_first {
                if i == fi {
                    let msg = &msgs[i];
                    return Message {
                        id: msg.id,
                        role: msg.role,
                        content: serde_json::json!(excerpt),
                        timestamp: msg.timestamp,
                        tokens: excerpt_tokens,
                        is_compressed: true,
                        tool_meta: msg.tool_meta.clone(),
                        tool_calls: msg.tool_calls.clone(),
                        tool_call_id: msg.tool_call_id.clone(),
                        thinking: msg.thinking.clone(),
                    };
                }
            }
            let msg = &msgs[i];
            // Only mark stale for "read" tool results — other tools (search, repo, symbols, bash)
            // don't provide complete file content, so staleness is less critical and false positives are common.
            if matches!(msg.role, Role::Tool) && msg.tool_meta.tool_name == "read" && !edited_files.is_empty() {
                let stale_paths: Vec<&str> = msg.tool_meta.paths_read.iter()
                    .filter(|p| edited_files.contains(*p))
                    .map(|s| s.as_str())
                    .collect();
                if !stale_paths.is_empty() {
                    return Message {
                        id: msg.id,
                        role: msg.role,
                        content: serde_json::json!(format!(
                            "[stale file read omitted: {} was edited after this read]",
                            stale_paths.join(", ")
                        )),
                        timestamp: msg.timestamp,
                        tokens: 20,
                        is_compressed: msg.is_compressed,
                        tool_meta: msg.tool_meta.clone(),
                        tool_calls: msg.tool_calls.clone(),
                        tool_call_id: msg.tool_call_id.clone(),
                        thinking: msg.thinking.clone(),
                    };
                }
            }
            msg.clone()
        }).collect();

        result
    }

    /// Build conversation messages using the reranking pipeline.
    ///
    /// Falls back to build_prompt_with_stale_check for small trajectories
    /// (<20 messages) where reranking overhead is not worthwhile.
    pub fn build_prompt_with_reranking(
        &self,
        trajectory: &Trajectory,
        edited_files: &HashSet<String>,
        task_reducer: Option<&TaskStateReducer>,
        history_budget: usize,
        active_files: &HashSet<String>,
        task_keywords: &[String],
    ) -> Vec<Message> {
        let msgs = &trajectory.messages;
        if msgs.is_empty() || msgs.len() < 20 {
            return self.build_prompt_with_stale_check(trajectory, edited_files, task_reducer, history_budget);
        }

        let budget = history_budget;
        let config = reranker::RerankerConfig::default();

        let candidates = reranker::prefilter_candidates(
            trajectory,
            active_files,
            task_keywords,
            &config,
            task_reducer,
        );

        let first_user = msgs.iter().position(|m| matches!(m.role, Role::User));
        let last_assistant = msgs.iter().rposition(|m| matches!(m.role, Role::Assistant));

        let mut selected_indices = reranker::budget_fit_select(
            &candidates,
            msgs,
            budget,
            first_user,
            last_assistant,
        );

        // Guarantee first user message (same as priority-bucket path)
        if let Some(first_idx) = first_user {
            if !selected_indices.contains(&first_idx) {
                let first_tokens = msgs[first_idx].tokens;
                if first_tokens <= budget {
                    selected_indices.push(first_idx);
                    let mut used = selected_indices.iter().map(|&i| msgs[i].tokens).sum::<usize>();
                    while used > budget {
                        if let Some(drop_idx) = selected_indices.iter()
                            .filter(|&&i| i != first_idx)
                            .min_by_key(|&&i| std::cmp::Reverse(i))
                        {
                            let drop_i = *drop_idx;
                            used -= msgs[drop_i].tokens;
                            selected_indices.retain(|&i| i != drop_i);
                        } else {
                            break;
                        }
                    }
                }
            }
        }

        selected_indices.sort();

        // Rewrite stale reads
        let result: Vec<Message> = selected_indices.into_iter().map(|i| {
            let msg = &msgs[i];
            if matches!(msg.role, Role::Tool) && msg.tool_meta.tool_name == "read" && !edited_files.is_empty() {
                let stale_paths: Vec<&str> = msg.tool_meta.paths_read.iter()
                    .filter(|p| edited_files.contains(*p))
                    .map(|s| s.as_str())
                    .collect();
                if !stale_paths.is_empty() {
                    return Message {
                        id: msg.id,
                        role: msg.role,
                        content: serde_json::json!(format!(
                            "[stale file read omitted: {} was edited after this read]",
                            stale_paths.join(", ")
                        )),
                        timestamp: msg.timestamp,
                        tokens: 20,
                        is_compressed: msg.is_compressed,
                        tool_meta: msg.tool_meta.clone(),
                        tool_calls: msg.tool_calls.clone(),
                        tool_call_id: msg.tool_call_id.clone(),
                        thinking: msg.thinking.clone(),
                    };
                }
            }
            msg.clone()
        }).collect();

        result
    }
}
