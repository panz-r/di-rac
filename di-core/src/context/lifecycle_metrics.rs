/// Per-turn metrics collected for adaptive compaction trigger evaluation.

/// Record of a single tool call within a turn.
#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub args_hash: u64,
}

/// Metrics snapshot for a single turn.
#[derive(Debug, Clone)]
pub struct TurnMetrics {
    pub total_tokens: usize,
    pub tool_result_tokens: usize,
    pub active_message_count: usize,
    pub stale_read_count: usize,
    pub tool_calls: Vec<ToolCallRecord>,
}

/// Bounded history of turn metrics, used by adaptive triggers.
pub struct LifecycleMetricsCollector {
    history: Vec<TurnMetrics>,
}

const MAX_HISTORY: usize = 20;

impl LifecycleMetricsCollector {
    pub fn new() -> Self {
        Self { history: Vec::new() }
    }

    /// Record a turn's metrics. Trims history to MAX_HISTORY.
    pub fn record_turn(&mut self, metrics: TurnMetrics) {
        self.history.push(metrics);
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }
    }

    /// Tool bloat rate over the last N turns: tool_result_tokens / total_tokens.
    /// Returns 0.0 if no data.
    pub fn tool_bloat_rate(&self, window: usize) -> f64 {
        let turns = self.last_n(window);
        if turns.is_empty() {
            return 0.0;
        }
        let total: usize = turns.iter().map(|t| t.total_tokens).sum();
        let tool: usize = turns.iter().map(|t| t.tool_result_tokens).sum();
        if total == 0 {
            return 0.0;
        }
        tool as f64 / total as f64
    }

    /// Stale read ratio from the latest turn: stale_read_count / active_message_count.
    /// Returns 0.0 if no messages.
    pub fn stale_ratio(&self) -> f64 {
        let latest = match self.history.last() {
            Some(t) => t,
            None => return 0.0,
        };
        if latest.active_message_count == 0 {
            return 0.0;
        }
        latest.stale_read_count as f64 / latest.active_message_count as f64
    }

    /// Max consecutive identical tool calls (same name + args_hash) across recent turns.
    /// Returns the maximum streak length found.
    pub fn retry_accumulation(&self) -> usize {
        let all_calls: Vec<&ToolCallRecord> = self.history.iter()
            .flat_map(|t| &t.tool_calls)
            .collect();
        if all_calls.is_empty() {
            return 0;
        }
        let mut max_streak = 1;
        let mut current_streak = 1;
        for i in 1..all_calls.len() {
            if all_calls[i].tool_name == all_calls[i - 1].tool_name
                && all_calls[i].args_hash == all_calls[i - 1].args_hash
            {
                current_streak += 1;
                if current_streak > max_streak {
                    max_streak = current_streak;
                }
            } else {
                current_streak = 1;
            }
        }
        max_streak
    }

    /// Token growth rate between last two turns: delta / prev_total.
    /// Returns 0.0 if insufficient data.
    pub fn growth_rate(&self) -> f64 {
        if self.history.len() < 2 {
            return 0.0;
        }
        let prev = &self.history[self.history.len() - 2];
        let curr = &self.history[self.history.len() - 1];
        if prev.total_tokens == 0 {
            return 0.0;
        }
        let delta = curr.total_tokens as i64 - prev.total_tokens as i64;
        delta.max(0) as f64 / prev.total_tokens as f64
    }

    fn last_n(&self, n: usize) -> &[TurnMetrics] {
        let start = self.history.len().saturating_sub(n);
        &self.history[start..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_turn(total_tokens: usize, tool_result_tokens: usize, tool_calls: Vec<ToolCallRecord>) -> TurnMetrics {
        TurnMetrics {
            total_tokens,
            tool_result_tokens,
            active_message_count: 10,
            stale_read_count: 0,
            tool_calls,
        }
    }

    #[test]
    fn tool_bloat_rate_computation() {
        let mut collector = LifecycleMetricsCollector::new();
        collector.record_turn(make_turn(1000, 600, vec![]));
        collector.record_turn(make_turn(2000, 1400, vec![]));
        assert!((collector.tool_bloat_rate(2) - 2000.0 / 3000.0).abs() < 0.001);
    }

    #[test]
    fn retry_accumulation_detects_streaks() {
        let call = ToolCallRecord { tool_name: "bash".into(), args_hash: 42 };
        let mut collector = LifecycleMetricsCollector::new();
        collector.record_turn(make_turn(1000, 300, vec![call.clone(), call.clone()]));
        collector.record_turn(make_turn(1200, 400, vec![call.clone(), call.clone(), call.clone()]));
        assert_eq!(collector.retry_accumulation(), 5);
    }

    #[test]
    fn growth_rate_between_turns() {
        let mut collector = LifecycleMetricsCollector::new();
        collector.record_turn(make_turn(1000, 200, vec![]));
        collector.record_turn(make_turn(1200, 300, vec![]));
        assert!((collector.growth_rate() - 0.2).abs() < 0.001);
    }

    #[test]
    fn history_bounded_to_20() {
        let mut collector = LifecycleMetricsCollector::new();
        for i in 0..25 {
            collector.record_turn(make_turn(100 + i, 50, vec![]));
        }
        assert_eq!(collector.history.len(), 20);
    }
}
