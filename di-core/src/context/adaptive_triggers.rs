use crate::context::lifecycle_metrics::LifecycleMetricsCollector;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TriggerSeverity {
    Soft,
    Hard,
}

#[derive(Debug, Clone)]
pub struct TriggerResult {
    pub severity: TriggerSeverity,
}

/// Threshold constants for adaptive triggers.
const TOKEN_SOFT_PCT: f64 = 0.75;
const TOKEN_HARD_PCT: f64 = 0.85;
const BLOAT_SOFT: f64 = 0.6;
const BLOAT_HARD: f64 = 0.8;
const STALE_SOFT: f64 = 0.7;
const STALE_HARD: f64 = 0.85;
const RETRY_SOFT: usize = 3;
const RETRY_HARD: usize = 5;
const GROWTH_SOFT: f64 = 0.15;
const GROWTH_HARD: f64 = 0.25;

pub struct AdaptiveTriggerEvaluator;

impl AdaptiveTriggerEvaluator {
    /// Evaluate all triggers against current metrics and token usage.
    /// Returns a list of triggers that fired.
    pub fn evaluate(
        metrics: &LifecycleMetricsCollector,
        current_tokens: usize,
        token_limit: usize,
    ) -> Vec<TriggerResult> {
        let mut results = Vec::new();

        // Token threshold
        if let Some(r) = Self::check_token_threshold(current_tokens, token_limit) {
            results.push(r);
        }

        // Tool bloat rate (over last 5 turns)
        if let Some(r) = Self::check_tool_bloat(metrics) {
            results.push(r);
        }

        // Stale history ratio
        if let Some(r) = Self::check_stale_history(metrics) {
            results.push(r);
        }

        // Retry accumulation
        if let Some(r) = Self::check_retry_accumulation(metrics) {
            results.push(r);
        }

        // Growth rate
        if let Some(r) = Self::check_growth_rate(metrics) {
            results.push(r);
        }

        results
    }

    fn check_token_threshold(current_tokens: usize, token_limit: usize) -> Option<TriggerResult> {
        if token_limit == 0 {
            return None;
        }
        let ratio = current_tokens as f64 / token_limit as f64;
        if ratio >= TOKEN_HARD_PCT {
            Some(TriggerResult {
                severity: TriggerSeverity::Hard,
            })
        } else if ratio >= TOKEN_SOFT_PCT {
            Some(TriggerResult {
                severity: TriggerSeverity::Soft,
            })
        } else {
            None
        }
    }

    fn check_tool_bloat(metrics: &LifecycleMetricsCollector) -> Option<TriggerResult> {
        let rate = metrics.tool_bloat_rate(5);
        if rate >= BLOAT_HARD {
            Some(TriggerResult {
                severity: TriggerSeverity::Hard,
            })
        } else if rate >= BLOAT_SOFT {
            Some(TriggerResult {
                severity: TriggerSeverity::Soft,
            })
        } else {
            None
        }
    }

    fn check_stale_history(metrics: &LifecycleMetricsCollector) -> Option<TriggerResult> {
        let ratio = metrics.stale_ratio();
        if ratio >= STALE_HARD {
            Some(TriggerResult {
                severity: TriggerSeverity::Hard,
            })
        } else if ratio >= STALE_SOFT {
            Some(TriggerResult {
                severity: TriggerSeverity::Soft,
            })
        } else {
            None
        }
    }

    fn check_retry_accumulation(metrics: &LifecycleMetricsCollector) -> Option<TriggerResult> {
        let retries = metrics.retry_accumulation();
        if retries >= RETRY_HARD {
            Some(TriggerResult {
                severity: TriggerSeverity::Hard,
            })
        } else if retries >= RETRY_SOFT {
            Some(TriggerResult {
                severity: TriggerSeverity::Soft,
            })
        } else {
            None
        }
    }

    fn check_growth_rate(metrics: &LifecycleMetricsCollector) -> Option<TriggerResult> {
        let rate = metrics.growth_rate();
        if rate >= GROWTH_HARD {
            Some(TriggerResult {
                severity: TriggerSeverity::Hard,
            })
        } else if rate >= GROWTH_SOFT {
            Some(TriggerResult {
                severity: TriggerSeverity::Soft,
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::lifecycle_metrics::{TurnMetrics, ToolCallRecord};

    fn make_turn(total_tokens: usize, tool_result_tokens: usize,
                 stale_read_count: usize, tool_calls: Vec<ToolCallRecord>) -> TurnMetrics {
        TurnMetrics {
            total_tokens,
            tool_result_tokens,
            active_message_count: 10,
            stale_read_count,
            tool_calls,
        }
    }

    #[test]
    fn token_threshold_soft_at_75pct() {
        let metrics = LifecycleMetricsCollector::new();
        let results = AdaptiveTriggerEvaluator::evaluate(&metrics, 7500, 10000);
        assert!(results.iter().any(|r| r.severity == TriggerSeverity::Soft));
    }

    #[test]
    fn token_threshold_hard_at_85pct() {
        let metrics = LifecycleMetricsCollector::new();
        let results = AdaptiveTriggerEvaluator::evaluate(&metrics, 8600, 10000);
        assert!(results.iter().any(|r| r.severity == TriggerSeverity::Hard));
    }

    #[test]
    fn no_triggers_below_thresholds() {
        let mut metrics = LifecycleMetricsCollector::new();
        metrics.record_turn(make_turn(1000, 200, 0, vec![]));
        let results = AdaptiveTriggerEvaluator::evaluate(&metrics, 1000, 10000);
        assert!(results.is_empty());
    }

    #[test]
    fn tool_bloat_soft_trigger() {
        let mut metrics = LifecycleMetricsCollector::new();
        // 65% tool result tokens = soft bloat
        metrics.record_turn(make_turn(1000, 650, 0, vec![]));
        metrics.record_turn(make_turn(1000, 650, 0, vec![]));
        let results = AdaptiveTriggerEvaluator::evaluate(&metrics, 5000, 10000);
        assert!(results.iter().any(|r| r.severity == TriggerSeverity::Soft));
    }
}
