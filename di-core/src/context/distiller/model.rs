use super::*;
use super::schemas::*;
use super::validate;
use super::prompts;
use crate::daemons::{GatewayStreamClient, GatewayRequest, ProviderConfig};
use crate::agent::metrics::ContextMetrics;
use std::sync::Arc;

/// Typed error from a distiller model call.
#[derive(Debug)]
pub enum DistillerCallError {
    ProviderError(String),
    InvalidJson(String),
}

/// Model-backed context distiller. Makes a single model call per operation.
/// Falls back to NoopContextDistiller on any failure (network, parse, validation).
/// Records specific failure reasons in telemetry.
pub struct ModelContextDistiller {
    config: ProviderConfig,
    gateway: Arc<GatewayStreamClient>,
    noop: noop::NoopContextDistiller,
    request_counter: std::sync::atomic::AtomicI64,
    metrics: Option<Arc<ContextMetrics>>,
    timeouts: super::DistillerTimeouts,
    admission: Option<Arc<super::admission::DistillerAdmission>>,
}

impl ModelContextDistiller {
    pub fn new(config: ProviderConfig, gateway: Arc<GatewayStreamClient>, metrics: Option<Arc<ContextMetrics>>, timeouts: Option<super::DistillerTimeouts>) -> Self {
        Self {
            config,
            gateway,
            noop: noop::NoopContextDistiller,
            request_counter: std::sync::atomic::AtomicI64::new(0),
            metrics,
            timeouts: timeouts.unwrap_or_default(),
            admission: None,
        }
    }

    fn record_provider_error(&self, msg: &str) {
        eprintln!("[distiller] provider error: {}", msg);
        if let Some(ref m) = self.metrics { m.inc_distiller_provider_error(); }
    }

    fn record_invalid_json(&self, msg: &str) {
        eprintln!("[distiller] JSON parse failed: {}", msg);
        if let Some(ref m) = self.metrics { m.inc_distiller_invalid_json(); }
    }

    fn record_schema_mismatch(&self, msg: &str) {
        eprintln!("[distiller] schema mismatch: {}", msg);
        if let Some(ref m) = self.metrics { m.inc_distiller_schema_mismatch(); }
    }

    fn record_validation_failed(&self, msg: &str) {
        eprintln!("[distiller] validation failed: {}", msg);
        if let Some(ref m) = self.metrics { m.inc_distiller_validation_failed(); }
    }

    fn record_model_call(&self) {
        if let Some(ref m) = self.metrics { m.inc_distiller_model_call(); }
    }

    async fn check_admission(&self, is_hard_compaction: bool) -> bool {
        if let Some(ref admission) = self.admission {
            let decision = admission.try_acquire(uuid::Uuid::nil(), is_hard_compaction).await;
            match decision {
                super::admission::AdmissionDecision::Allowed => {
                    if let Some(ref m) = self.metrics { m.inc_distiller_admission_accepted(); }
                    true
                }
                _ => {
                    if let Some(ref m) = self.metrics { m.inc_distiller_admission_rejected(); }
                    eprintln!("[distiller] admission rejected: {:?}", decision);
                    false
                }
            }
        } else {
            true
        }
    }

    async fn call_model(&self, user_messages: Vec<crate::daemons::GatewayMessage>, timeout_ms: u64) -> Result<serde_json::Value, DistillerCallError> {
        let request_id = self.request_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let request = GatewayRequest {
            id: request_id,
            stream: false, // Non-streaming: distiller returns a single JSON response
            provider: Some(self.config.clone()),
            messages: user_messages,
            system: Some(prompts::system_prompt()),
            tools: None,
            max_tokens: Some(2048),
            temperature: Some(0.1),
            thinking: None,
            timeout: Some(timeout_ms as i64),
        };

        let result = self.gateway.stream_chat(request).await;
        let mut chunk_rx = match result {
            Ok(rx) => rx,
            Err(e) => {
                return Err(DistillerCallError::ProviderError(format!("gateway connection failed: {}", e)));
            }
        };

        let mut full_text = String::new();
        while let Some(chunk_result) = chunk_rx.recv().await {
            match chunk_result {
                Ok(chunk) => {
                    match chunk.chunk_type.as_str() {
                        "delta" => {
                            if let Some(text) = &chunk.text_delta {
                                full_text.push_str(text);
                            }
                        }
                        "complete" => break,
                        _ => {}
                    }
                }
                Err(e) => {
                    return Err(DistillerCallError::ProviderError(format!("chunk error: {}", e)));
                }
            }
        }

        let trimmed = full_text.trim();
        let cleaned = trimmed
            .strip_prefix("```json")
            .unwrap_or(trimmed)
            .strip_prefix("```")
            .unwrap_or(trimmed);
        let cleaned = cleaned.trim();
        let cleaned = cleaned.strip_suffix("```").unwrap_or(cleaned).trim();

        match serde_json::from_str::<serde_json::Value>(cleaned) {
            Ok(v) => Ok(v),
            Err(e) => {
                Err(DistillerCallError::InvalidJson(format!("{} — input was {} chars", e, cleaned.len())))
            }
        }
    }
}

#[async_trait::async_trait]
impl ContextDistiller for ModelContextDistiller {
    async fn distill_tool_result(
        &self,
        input: ToolDistillInput,
    ) -> DistillationResult<DistilledToolResult> {
        if !self.check_admission(false).await {
            return self.noop.distill_tool_result(input).await;
        }
        self.record_model_call();
        let messages = prompts::tool_result_messages(&input);
        match self.call_model(messages, self.timeouts.inline_ms).await {
            Ok(json) => {
                match serde_json::from_value::<DistilledToolResult>(json) {
                    Ok(parsed) => {
                        match validate::validate_tool_result(&parsed) {
                            Ok(()) => {
                                // Faithfulness check: log warnings for ungrounded refs (Phase 1)
                                let warnings = validate::validate_tool_result_faithfulness(
                                    &parsed, &input.tool_result.to_string(),
                                );
                                for w in &warnings {
                                    eprintln!("[distiller] faithfulness warning: {}", w);
                                }
                                let evidence_warnings = validate::validate_exact_evidence_faithfulness(
                                    &parsed, &input.tool_result.to_string(),
                                );
                                for w in &evidence_warnings {
                                    eprintln!("[distiller] evidence faithfulness warning: {}", w);
                                }
                                let mut output = parsed;
                                output.source_event_ids = input.source_event_ids.iter().map(|u| u.to_string()).collect();
                                DistillationResult {
                                    output,
                                    provenance: Provenance {
                                        source_event_ids: input.source_event_ids.clone(),
                                        confidence: 0.7,
                                        source: DistillerSource::Model,
                                        config_version: 0,
                                    },
                                }
                            }
                            Err(e) => {
                                self.record_validation_failed(&format!("{:?}", e));
                                self.noop.distill_tool_result(input).await
                            }
                        }
                    }
                    Err(e) => {
                        self.record_schema_mismatch(&e.to_string());
                        self.noop.distill_tool_result(input).await
                    }
                }
            }
            Err(DistillerCallError::ProviderError(msg)) => {
                self.record_provider_error(&msg);
                self.noop.distill_tool_result(input).await
            }
            Err(DistillerCallError::InvalidJson(msg)) => {
                self.record_invalid_json(&msg);
                self.noop.distill_tool_result(input).await
            }
        }
    }

    async fn consolidate_task_state(
        &self,
        input: TaskStateInput,
    ) -> DistillationResult<TaskStatePatch> {
        if !self.check_admission(true).await {
            return self.noop.consolidate_task_state(input).await;
        }
        self.record_model_call();
        let messages = prompts::task_state_messages(&input);
        match self.call_model(messages, self.timeouts.compaction_ms).await {
            Ok(json) => {
                match serde_json::from_value::<TaskStatePatch>(json) {
                    Ok(parsed) => {
                        match validate::validate_task_state_patch(&parsed) {
                            Ok(()) => DistillationResult {
                                output: parsed,
                                provenance: Provenance {
                                    source_event_ids: input.source_event_ids.clone(),
                                    confidence: 0.7,
                                    source: DistillerSource::Model,
                                    config_version: 0,
                                },
                            },
                            Err(e) => {
                                self.record_validation_failed(&format!("{:?}", e));
                                self.noop.consolidate_task_state(input).await
                            }
                        }
                    }
                    Err(e) => {
                        self.record_schema_mismatch(&e.to_string());
                        self.noop.consolidate_task_state(input).await
                    }
                }
            }
            Err(DistillerCallError::ProviderError(msg)) => {
                self.record_provider_error(&msg);
                self.noop.consolidate_task_state(input).await
            }
            Err(DistillerCallError::InvalidJson(msg)) => {
                self.record_invalid_json(&msg);
                self.noop.consolidate_task_state(input).await
            }
        }
    }
}
