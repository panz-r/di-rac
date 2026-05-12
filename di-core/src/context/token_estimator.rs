use serde_json::Value;

/// Trait for token estimation. Implementations can range from a simple
/// character heuristic to model-specific tokenizers.
pub trait TokenEstimator: Send + Sync {
    fn count_text(&self, text: &str) -> usize;
    fn count_tools(&self, tools: &[Value]) -> usize;
}

/// Conservative estimator: returns `max(chars/3.2, provider_estimate)`.
/// Prevents under-counting compared to the old chars/4 heuristic while
/// allowing model-specific calibration to reduce over-counting.
pub struct ConservativeEstimator {
    chars_per_token: f64,
    floor_ratio: f64,
}

impl ConservativeEstimator {
    pub fn new(chars_per_token: f64) -> Self {
        Self { chars_per_token, floor_ratio: 3.2 }
    }

    /// Default estimator with the conservative 3.2 floor.
    pub fn default_conservative() -> Self {
        Self::new(3.2)
    }
}

impl TokenEstimator for ConservativeEstimator {
    fn count_text(&self, text: &str) -> usize {
        let calibrated = (text.len() as f64 / self.chars_per_token) as usize;
        let floor = (text.len() as f64 / self.floor_ratio) as usize;
        calibrated.max(floor)
    }

    fn count_tools(&self, tools: &[Value]) -> usize {
        let total_chars: usize = tools.iter()
            .filter_map(|t| serde_json::to_string(t).ok())
            .map(|s| s.len())
            .sum();
        let calibrated = (total_chars as f64 / self.chars_per_token) as usize;
        let floor = (total_chars as f64 / self.floor_ratio) as usize;
        calibrated.max(floor)
    }
}
