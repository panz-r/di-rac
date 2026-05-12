use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Runtime metrics for the context compilation system.
/// Counters are lock-free atomics.
#[derive(Debug)]
pub struct ContextMetrics {
    pub distiller_model_call_count: AtomicU64,
    pub distiller_fallback_count: AtomicU64,
    pub distiller_invalid_json_count: AtomicU64,
    pub distiller_schema_mismatch_count: AtomicU64,
    pub distiller_validation_failed_count: AtomicU64,
    pub distiller_provider_error_count: AtomicU64,
    pub redaction_count: AtomicU64,
    pub distiller_admission_accepted_count: AtomicU64,
    pub distiller_admission_rejected_count: AtomicU64,
}

impl ContextMetrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            distiller_model_call_count: AtomicU64::new(0),
            distiller_fallback_count: AtomicU64::new(0),
            distiller_invalid_json_count: AtomicU64::new(0),
            distiller_schema_mismatch_count: AtomicU64::new(0),
            distiller_validation_failed_count: AtomicU64::new(0),
            distiller_provider_error_count: AtomicU64::new(0),
            redaction_count: AtomicU64::new(0),
            distiller_admission_accepted_count: AtomicU64::new(0),
            distiller_admission_rejected_count: AtomicU64::new(0),
        })
    }

    pub fn inc_distiller_model_call(&self) { self.distiller_model_call_count.fetch_add(1, Ordering::Relaxed); }
    pub fn inc_distiller_fallback(&self) { self.distiller_fallback_count.fetch_add(1, Ordering::Relaxed); }
    pub fn inc_distiller_invalid_json(&self) { self.distiller_invalid_json_count.fetch_add(1, Ordering::Relaxed); self.inc_distiller_fallback(); }
    pub fn inc_distiller_schema_mismatch(&self) { self.distiller_schema_mismatch_count.fetch_add(1, Ordering::Relaxed); self.inc_distiller_fallback(); }
    pub fn inc_distiller_validation_failed(&self) { self.distiller_validation_failed_count.fetch_add(1, Ordering::Relaxed); self.inc_distiller_fallback(); }
    pub fn inc_distiller_provider_error(&self) { self.distiller_provider_error_count.fetch_add(1, Ordering::Relaxed); self.inc_distiller_fallback(); }
    pub fn inc_redaction(&self) { self.redaction_count.fetch_add(1, Ordering::Relaxed); }
    pub fn inc_distiller_admission_accepted(&self) { self.distiller_admission_accepted_count.fetch_add(1, Ordering::Relaxed); }
    pub fn inc_distiller_admission_rejected(&self) { self.distiller_admission_rejected_count.fetch_add(1, Ordering::Relaxed); }
}
