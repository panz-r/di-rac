use crate::context::adaptive_triggers::{AdaptiveTriggerEvaluator, TriggerSeverity};
use crate::context::lifecycle_metrics::LifecycleMetricsCollector;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LifecycleState {
    Normal,
    Growing,
    NearLimit,
    CompactionDue,
    PostCompaction,
    Hibernating,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PressureLevel {
    Low,
    Moderate,
    High,
    Critical,
}

pub struct CompactAdvisory {
    pub allowed: bool,
    pub pressure_level: PressureLevel,
    pub guidance: Option<String>,
}

pub struct ContextLifecycleManager {
    state: LifecycleState,
    metrics: LifecycleMetricsCollector,
}

impl ContextLifecycleManager {
    pub fn new() -> Self {
        Self {
            state: LifecycleState::Normal,
            metrics: LifecycleMetricsCollector::new(),
        }
    }

    pub fn metrics_mut(&mut self) -> &mut LifecycleMetricsCollector {
        &mut self.metrics
    }

    /// Evaluate current state based on token usage and trigger analysis.
    /// Returns the new (or unchanged) state after evaluation.
    pub fn evaluate(&mut self, current_tokens: usize, token_limit: usize) -> LifecycleState {
        // PostCompaction always returns to Normal on next evaluation
        if self.state == LifecycleState::PostCompaction {
            self.transition_to(LifecycleState::Normal);
            return self.state;
        }

        // Hibernating stays until explicitly woken
        if self.state == LifecycleState::Hibernating {
            return self.state;
        }

        // CompactionDue stays until explicitly notified complete
        if self.state == LifecycleState::CompactionDue {
            return self.state;
        }

        // Evaluate triggers
        let triggers = AdaptiveTriggerEvaluator::evaluate(&self.metrics, current_tokens, token_limit);

        let usage_ratio = if token_limit > 0 {
            current_tokens as f64 / token_limit as f64
        } else {
            0.0
        };

        let has_soft = triggers.iter().any(|t| t.severity == TriggerSeverity::Soft);
        let has_hard = triggers.iter().any(|t| t.severity == TriggerSeverity::Hard);

        // State transitions
        match self.state {
            LifecycleState::Normal => {
                if usage_ratio > 0.50 {
                    self.transition_to(LifecycleState::Growing);
                }
            }
            LifecycleState::Growing => {
                if usage_ratio > 0.75 || has_soft {
                    self.transition_to(LifecycleState::NearLimit);
                } else if usage_ratio <= 0.40 {
                    self.transition_to(LifecycleState::Normal);
                }
            }
            LifecycleState::NearLimit => {
                if usage_ratio > 0.85 || has_hard {
                    self.transition_to(LifecycleState::CompactionDue);
                } else if usage_ratio <= 0.50 {
                    self.transition_to(LifecycleState::Growing);
                }
            }
            _ => {}
        }

        self.state
    }

    /// Whether compaction should be performed now.
    pub fn should_compact(&self) -> bool {
        self.state == LifecycleState::CompactionDue
    }

    /// Notify that compaction has completed.
    pub fn notify_compaction_complete(&mut self) {
        self.transition_to(LifecycleState::PostCompaction);
    }

    /// Evaluate whether a model-initiated compact advisory should be accepted.
    pub fn evaluate_compact_advisory(
        &self,
        summary: &str,
        current_tokens: usize,
        token_limit: usize,
    ) -> CompactAdvisory {
        let usage_ratio = if token_limit > 0 {
            current_tokens as f64 / token_limit as f64
        } else {
            0.0
        };

        match self.state {
            LifecycleState::CompactionDue => CompactAdvisory {
                allowed: true,
                pressure_level: PressureLevel::Critical,
                guidance: None,
            },
            LifecycleState::NearLimit => {
                // Allow but check summary quality
                let guidance = check_summary_quality(summary);
                CompactAdvisory {
                    allowed: true,
                    pressure_level: PressureLevel::High,
                    guidance,
                }
            }
            LifecycleState::Growing => {
                if usage_ratio > 0.50 {
                    let guidance = check_summary_quality(summary);
                    CompactAdvisory {
                        allowed: true,
                        pressure_level: PressureLevel::Moderate,
                        guidance,
                    }
                } else {
                    CompactAdvisory {
                        allowed: false,
                        pressure_level: PressureLevel::Low,
                        guidance: Some("Context pressure is low. Continue working — compaction will trigger automatically when needed.".into()),
                    }
                }
            }
            _ => CompactAdvisory {
                allowed: false,
                pressure_level: PressureLevel::Low,
                guidance: Some("Context pressure is low. Continue working.".into()),
            },
        }
    }

    fn transition_to(&mut self, new_state: LifecycleState) {
        if self.state != new_state {
            self.state = new_state;
        }
    }
}

/// Check summary quality and return guidance if issues found.
fn check_summary_quality(summary: &str) -> Option<String> {
    let mut issues: Vec<String> = Vec::new();
    if summary.len() < 200 {
        issues.push("Summary is too short (need >200 chars)".into());
    }
    if !summary.contains('/') && !summary.contains('.') {
        issues.push("Summary should mention specific file paths".into());
    }
    if issues.is_empty() {
        None
    } else {
        Some(format!("Guidance: {}", issues.join("; ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::lifecycle_metrics::TurnMetrics;

    #[allow(dead_code)]
    fn make_turn(total_tokens: usize, tool_result_tokens: usize, stale_read_count: usize) -> TurnMetrics {
        TurnMetrics {
            total_tokens,
            tool_result_tokens,
            active_message_count: 10,
            stale_read_count,
            tool_calls: vec![],
        }
    }

    #[test]
    fn normal_to_growing_at_50pct() {
        let mut lm = ContextLifecycleManager::new();
        let state = lm.evaluate(5500, 10000);
        assert_eq!(state, LifecycleState::Growing);
    }

    #[test]
    fn growing_to_near_limit_at_75pct() {
        let mut lm = ContextLifecycleManager::new();
        lm.evaluate(5500, 10000); // Normal -> Growing
        let state = lm.evaluate(7600, 10000); // Growing -> NearLimit
        assert_eq!(state, LifecycleState::NearLimit);
    }

    #[test]
    fn near_limit_to_compaction_due_at_85pct() {
        let mut lm = ContextLifecycleManager::new();
        lm.evaluate(5500, 10000);
        lm.evaluate(7600, 10000);
        let state = lm.evaluate(8600, 10000); // NearLimit -> CompactionDue
        assert_eq!(state, LifecycleState::CompactionDue);
        assert!(lm.should_compact());
    }

    #[test]
    fn post_compaction_returns_to_normal() {
        let mut lm = ContextLifecycleManager::new();
        lm.evaluate(5500, 10000);
        lm.evaluate(7600, 10000);
        lm.evaluate(8600, 10000);
        lm.notify_compaction_complete();
        // Next evaluate with low usage -> Normal
        let state = lm.evaluate(1000, 10000);
        assert_eq!(state, LifecycleState::Normal);
    }

    #[test]
    fn compact_advisory_rejected_when_low_pressure() {
        let lm = ContextLifecycleManager::new();
        let advisory = lm.evaluate_compact_advisory("summary text", 2000, 10000);
        assert!(!advisory.allowed);
        assert_eq!(advisory.pressure_level, PressureLevel::Low);
    }

    #[test]
    fn compact_advisory_allowed_when_critical() {
        let mut lm = ContextLifecycleManager::new();
        lm.evaluate(5500, 10000);
        lm.evaluate(7600, 10000);
        lm.evaluate(8600, 10000);
        let advisory = lm.evaluate_compact_advisory("summary text", 8600, 10000);
        assert!(advisory.allowed);
        assert_eq!(advisory.pressure_level, PressureLevel::Critical);
    }

}
