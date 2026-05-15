use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Semaphore, Mutex};
use uuid::Uuid;

/// Admission control for distiller model calls.
/// Prevents gateway overload via concurrency limits and per-agent rate limits.
pub struct DistillerAdmission {
    concurrency_sem: Arc<Semaphore>,
    per_agent: Mutex<HashMap<Uuid, RateLimitState>>,
    max_calls_per_min: u32,
}

struct RateLimitState {
    count: u32,
    window_start: std::time::Instant,
}

const WINDOW_SECS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AdmissionDecision {
    Allowed,
    RateLimited,
    ConcurrencyLimited,
}

impl DistillerAdmission {
    #[allow(dead_code)]
    pub fn new(max_concurrency: usize, max_calls_per_min: u32) -> Self {
        Self {
            concurrency_sem: Arc::new(Semaphore::new(max_concurrency)),
            per_agent: Mutex::new(HashMap::new()),
            max_calls_per_min,
        }
    }

    /// Try to acquire admission for a distiller call.
    /// `is_hard_compaction` bypasses all limits (never block critical compaction).
    pub async fn try_acquire(&self, agent_id: Uuid, is_hard_compaction: bool) -> AdmissionDecision {
        if is_hard_compaction {
            return AdmissionDecision::Allowed;
        }

        // Check per-agent rate limit
        {
            let mut per_agent = self.per_agent.lock().await;
            let now = std::time::Instant::now();
            let state = per_agent.entry(agent_id).or_insert(RateLimitState {
                count: 0,
                window_start: now,
            });
            if now.duration_since(state.window_start).as_secs() >= WINDOW_SECS {
                state.count = 0;
                state.window_start = now;
            }
            if state.count >= self.max_calls_per_min {
                return AdmissionDecision::RateLimited;
            }
            state.count += 1;
        }

        // Check concurrency
        match self.concurrency_sem.try_acquire() {
            Ok(_permit) => AdmissionDecision::Allowed,
            Err(_) => AdmissionDecision::ConcurrencyLimited,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hard_compaction_bypasses_limits() {
        let admission = DistillerAdmission::new(1, 0);
        let id = Uuid::new_v4();
        // Even with 0 calls/min limit, hard compaction is allowed
        assert_eq!(admission.try_acquire(id, true).await, AdmissionDecision::Allowed);
    }

    #[tokio::test]
    async fn rate_limit_blocks_after_max() {
        let admission = DistillerAdmission::new(10, 2);
        let id = Uuid::new_v4();
        assert_eq!(admission.try_acquire(id, false).await, AdmissionDecision::Allowed);
        assert_eq!(admission.try_acquire(id, false).await, AdmissionDecision::Allowed);
        assert_eq!(admission.try_acquire(id, false).await, AdmissionDecision::RateLimited);
    }
}
