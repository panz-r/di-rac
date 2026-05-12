use crate::agent::trajectory::{Message, Role, Trajectory};
use crate::context::task_state::TaskStateReducer;
use std::collections::HashSet;

/// A scored candidate for inclusion in the context window.
#[derive(Debug, Clone)]
pub struct ScoredCandidate {
    pub index: usize,
    pub score: f64,
    pub priority: u8,
    pub tokens: usize,
}

/// Configuration for the reranker.
pub struct RerankerConfig {
    pub max_candidates: usize,
    pub min_score: f64,
}

impl Default for RerankerConfig {
    fn default() -> Self {
        Self {
            max_candidates: 50,
            min_score: 0.1,
        }
    }
}

/// Deterministic prefilter: score each message using heuristics and return
/// the top candidates sorted by descending score.
pub fn prefilter_candidates(
    trajectory: &Trajectory,
    active_files: &HashSet<String>,
    task_keywords: &[String],
    config: &RerankerConfig,
    task_reducer: Option<&TaskStateReducer>,
) -> Vec<ScoredCandidate> {
    let msgs = &trajectory.messages;
    if msgs.is_empty() {
        return Vec::new();
    }

    let total = msgs.len();
    let first_user = msgs.iter().position(|m| matches!(m.role, Role::User));
    let last_assistant = msgs.iter().rposition(|m| matches!(m.role, Role::Assistant));

    let mut candidates: Vec<ScoredCandidate> = Vec::with_capacity(total);

    for (i, msg) in msgs.iter().enumerate() {
        let mut score: f64 = 0.0;

        // Factor 1: Recency (0.0 to 0.3)
        let recency = if total > 1 { i as f64 / (total - 1) as f64 } else { 1.0 };
        score += recency * 0.3;

        // Factor 2: Role weight (0.0 to 0.2)
        match msg.role {
            Role::User => {
                let is_first = first_user == Some(i);
                score += if is_first { 0.2 } else { 0.15 };
            }
            Role::Assistant => {
                let is_last = last_assistant == Some(i);
                score += if is_last { 0.2 } else { 0.1 };
            }
            Role::Tool => {
                score += 0.05;
            }
            Role::System => {
                score += 0.1;
            }
        }

        // Factor 3: File overlap (0.0 to 0.3)
        let file_overlap_count = msg.tool_meta.paths_read.iter()
            .chain(msg.tool_meta.paths_written.iter())
            .filter(|p| active_files.contains(*p))
            .count();
        score += (file_overlap_count as f64 * 0.1).min(0.3);

        // Factor 4: Tool type relevance (0.0 to 0.2)
        match msg.tool_meta.tool_name.as_str() {
            "read" => score += 0.15,
            "search" | "symbols" => score += 0.1,
            "bash" if file_overlap_count > 0 => score += 0.1,
            "bash" => score += 0.05,
            _ => {}
        }

        // Factor 5: Keyword match in content (0.0 to 0.1)
        if !task_keywords.is_empty() {
            let content_lower = msg.content.to_string().to_lowercase();
            let match_count = task_keywords.iter()
                .filter(|kw| content_lower.contains(&kw.to_lowercase()))
                .count();
            score += (match_count as f64 / task_keywords.len().max(1) as f64) * 0.1;
        }

        let priority = compute_message_priority(i, msg, first_user, last_assistant, task_reducer);

        candidates.push(ScoredCandidate {
            index: i,
            score,
            priority,
            tokens: msg.tokens,
        });
    }

    candidates.sort_by(|a, b| {
        b.score.partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.priority.cmp(&a.priority))
    });
    candidates.truncate(config.max_candidates);
    candidates.retain(|c| c.score >= config.min_score);

    candidates
}

/// Final budget-fit selection from scored candidates.
pub fn budget_fit_select(
    candidates: &[ScoredCandidate],
    messages: &[Message],
    budget: usize,
    first_user_idx: Option<usize>,
    last_assistant_idx: Option<usize>,
) -> Vec<usize> {
    let mut selected: Vec<usize> = Vec::new();
    let mut used_tokens = 0usize;

    // Phase 1: Force-include first user message
    if let Some(fui) = first_user_idx {
        if candidates.iter().any(|c| c.index == fui) {
            selected.push(fui);
            used_tokens += messages[fui].tokens;
        }
    }

    // Phase 2: Force-include last assistant message
    if let Some(lai) = last_assistant_idx {
        if candidates.iter().any(|c| c.index == lai) && !selected.contains(&lai) {
            if used_tokens + messages[lai].tokens <= budget {
                selected.push(lai);
                used_tokens += messages[lai].tokens;
            }
        }
    }

    // Phase 3: Fill by score
    for candidate in candidates {
        if selected.contains(&candidate.index) {
            continue;
        }
        if used_tokens + candidate.tokens <= budget {
            selected.push(candidate.index);
            used_tokens += candidate.tokens;
        }
    }

    selected.sort();
    selected
}

/// Extract keyword tokens from a task description for relevance matching.
pub fn extract_task_keywords(task_description: &str) -> Vec<String> {
    let stop_words: HashSet<&str> = [
        "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "will", "would", "could",
        "should", "may", "might", "must", "shall", "can", "need", "dare",
        "to", "of", "in", "for", "on", "with", "at", "by", "from", "as",
        "into", "through", "during", "before", "after", "above", "below",
        "and", "but", "or", "nor", "not", "so", "yet", "both", "either",
        "it", "its", "this", "that", "these", "those", "i", "me", "my",
        "we", "our", "you", "your", "he", "she", "they", "them", "their",
        "what", "which", "who", "whom", "how", "when", "where", "why",
    ].iter().cloned().collect();

    task_description.to_lowercase()
        .split_whitespace()
        .filter(|w| w.len() > 2 && !stop_words.contains(w))
        .filter(|w| w.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-'))
        .take(20)
        .map(|w| w.to_string())
        .collect()
}

fn compute_message_priority(
    index: usize,
    msg: &Message,
    first_user: Option<usize>,
    last_assistant: Option<usize>,
    task_reducer: Option<&TaskStateReducer>,
) -> u8 {
    let mut priority = 0u8;

    if matches!(msg.role, Role::User) {
        let is_first = first_user == Some(index);
        if let Some(reducer) = task_reducer {
            let text = msg.content.to_string();
            let kind = reducer.classify(&text, is_first);
            priority = TaskStateReducer::priority_for_kind(kind);
        } else {
            priority = 4;
        }
    }

    if let Some(idx) = last_assistant {
        if idx == index {
            priority = priority.max(4);
        }
    }

    priority
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::trajectory::ToolMessageMeta;
    use chrono::Utc;

    #[allow(dead_code)]
    fn make_msg(role: Role, content: &str, tokens: usize, tool_name: &str, paths_read: Vec<&str>) -> Message {
        Message {
            id: uuid::Uuid::new_v4(),
            role,
            content: serde_json::json!(content),
            timestamp: Utc::now(),
            tokens,
            is_compressed: false,
            tool_meta: ToolMessageMeta {
                tool_name: tool_name.to_string(),
                paths_read: paths_read.into_iter().map(|s| s.to_string()).collect(),
                paths_written: Vec::new(),
                is_compacted: false,
                artifact_ref: None,
            },
            tool_calls: Vec::new(),
            tool_call_id: None,
            thinking: None,
        }
    }

    fn make_trajectory(count: usize) -> Trajectory {
        let mut traj = Trajectory::new();
        for i in 0..count {
            let role = if i == 0 { Role::User } else if i % 3 == 0 { Role::Tool } else { Role::Assistant };
            traj.add_message(role, serde_json::json!(format!("msg {}", i)), 100);
        }
        traj
    }

    #[test]
    fn prefilter_scores_recent_higher() {
        let traj = make_trajectory(30);
        let config = RerankerConfig::default();
        let candidates = prefilter_candidates(&traj, &HashSet::new(), &[], &config, None);

        // Last message should have highest score
        let last = candidates.iter().max_by_key(|c| c.index).unwrap();
        let first = candidates.iter().min_by_key(|c| c.index).unwrap();
        assert!(last.score >= first.score, "recent messages should score >= older messages");
    }

    #[test]
    fn prefilter_file_overlap_boost() {
        let mut traj = Trajectory::new();
        // Build enough messages so two adjacent tools have similar recency
        for i in 0..20 {
            let role = if i == 0 { Role::User } else if i % 3 == 0 { Role::Assistant } else { Role::Tool };
            traj.add_message(role, serde_json::json!(format!("msg {}", i)), 100);
        }
        // Replace messages 18 and 19 with tool messages that have different file overlap
        traj.messages[18] = Message {
            id: uuid::Uuid::new_v4(),
            role: Role::Tool,
            content: serde_json::json!("output"),
            timestamp: chrono::Utc::now(),
            tokens: 100,
            is_compressed: false,
            tool_meta: ToolMessageMeta {
                tool_name: "read".to_string(),
                paths_read: vec!["src/important.rs".to_string()],
                paths_written: Vec::new(),
                is_compacted: false,
                artifact_ref: None,
            },
            tool_calls: Vec::new(),
            tool_call_id: None,
            thinking: None,
        };
        traj.messages[19] = Message {
            id: uuid::Uuid::new_v4(),
            role: Role::Tool,
            content: serde_json::json!("output"),
            timestamp: chrono::Utc::now(),
            tokens: 100,
            is_compressed: false,
            tool_meta: ToolMessageMeta {
                tool_name: "read".to_string(),
                paths_read: vec!["src/other.rs".to_string()],
                paths_written: Vec::new(),
                is_compacted: false,
                artifact_ref: None,
            },
            tool_calls: Vec::new(),
            tool_call_id: None,
            thinking: None,
        };

        let mut active = HashSet::new();
        active.insert("src/important.rs".to_string());

        let config = RerankerConfig::default();
        let candidates = prefilter_candidates(&traj, &active, &[], &config, None);

        let overlap = candidates.iter().find(|c| c.index == 18).unwrap();
        let no_overlap = candidates.iter().find(|c| c.index == 19).unwrap();
        assert!(overlap.score > no_overlap.score, "file overlap should boost score: overlap={}, no_overlap={}", overlap.score, no_overlap.score);
    }

    #[test]
    fn prefilter_respects_max_candidates() {
        let traj = make_trajectory(100);
        let config = RerankerConfig { max_candidates: 20, min_score: 0.0 };
        let candidates = prefilter_candidates(&traj, &HashSet::new(), &[], &config, None);
        assert!(candidates.len() <= 20, "should cap at max_candidates");
    }

    #[test]
    fn budget_fit_respects_budget() {
        let mut traj = Trajectory::new();
        for i in 0..20 {
            traj.add_message(Role::Assistant, serde_json::json!(format!("msg {}", i)), 100);
        }
        let config = RerankerConfig::default();
        let candidates = prefilter_candidates(&traj, &HashSet::new(), &[], &config, None);

        let selected = budget_fit_select(&candidates, &traj.messages, 500, None, None);
        let total: usize = selected.iter().map(|i| traj.messages[*i].tokens).sum();
        assert!(total <= 500, "total tokens {} exceeds budget 500", total);
    }

    #[test]
    fn budget_fit_force_includes_first_user() {
        let mut traj = Trajectory::new();
        traj.add_message(Role::User, serde_json::json!("task"), 100);
        for _ in 0..30 {
            traj.add_message(Role::Assistant, serde_json::json!("long output"), 200);
        }

        let config = RerankerConfig { max_candidates: 50, min_score: 0.0 };
        let candidates = prefilter_candidates(&traj, &HashSet::new(), &[], &config, None);
        let selected = budget_fit_select(&candidates, &traj.messages, 1000, Some(0), None);

        assert!(selected.contains(&0), "first user message must be force-included");
    }

    #[test]
    fn budget_fit_force_includes_last_assistant() {
        let mut traj = Trajectory::new();
        for _ in 0..30 {
            traj.add_message(Role::Assistant, serde_json::json!("long output"), 200);
        }

        let config = RerankerConfig { max_candidates: 50, min_score: 0.0 };
        let candidates = prefilter_candidates(&traj, &HashSet::new(), &[], &config, None);
        let selected = budget_fit_select(&candidates, &traj.messages, 1000, None, Some(29));

        assert!(selected.contains(&29), "last assistant message must be force-included");
    }

    #[test]
    fn extract_keywords_filters_stop_words() {
        let keywords = extract_task_keywords("Fix the bug in the authentication module");
        assert!(!keywords.iter().any(|k| k == "the" || k == "in"), "stop words should be filtered");
        assert!(keywords.iter().any(|k| k == "fix"), "relevant words should be kept");
        assert!(keywords.iter().any(|k| k == "authentication"), "relevant words should be kept");
    }

    #[test]
    fn extract_keywords_limits_to_20() {
        let long_task = (0..50).map(|i| format!("keyword{}", i)).collect::<Vec<_>>().join(" ");
        let keywords = extract_task_keywords(&long_task);
        assert!(keywords.len() <= 20, "should limit to 20 keywords, got {}", keywords.len());
    }
}
