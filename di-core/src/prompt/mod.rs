pub mod stable;
pub mod session;

use crate::agent::file_context::FileContextTracker;
use crate::context::{MemoryVault, ConservativeEstimator, TokenEstimator};
use crate::daemons::GatewayMessage;
use crate::tools::tool_defs::TOOL_DEFINITIONS;
use session::SessionContext;
use std::collections::HashSet;

/// Compiled output ready for the gateway.
pub struct ContextFrame {
    pub system: String,
    pub tools: Vec<serde_json::Value>,
    pub messages: Vec<GatewayMessage>,
}

/// Dynamic context that changes every turn.
pub struct DynamicContext<'a> {
    pub file_context: &'a FileContextTracker,
    pub observations: &'a MemoryVault,
    pub current_apis: &'a HashSet<String>,
    pub background_summary: &'a Option<String>,
    pub distilled_context: &'a Option<String>,
    pub task_state_summary: &'a Option<String>,
    pub tail_reminder: &'a Option<String>,
}

impl<'a> DynamicContext<'a> {
    /// Build the dynamic suffix string.
    pub fn to_string_content(&self) -> Option<String> {
        let mut parts = Vec::new();

        if let Some(summary) = self.task_state_summary {
            if !summary.is_empty() {
                parts.push(format!("# Task State\n\n{}", summary));
            }
        }

        let file_ctx = self.file_context.get_summary();
        if !file_ctx.is_empty() {
            parts.push(file_ctx);
        }

        let relevant_obs = self.observations.get_relevant_observations(self.current_apis, 0.5);
        if !relevant_obs.is_empty() {
            let obs_block = relevant_obs.iter()
                .map(|o| format!("[{}] {}", o.obs_type.to_uppercase(), o.content))
                .collect::<Vec<_>>()
                .join("\n\n");
            parts.push(format!("# Past Observations\n\n{}", obs_block));
        }

        if let Some(bg) = self.background_summary {
            if !bg.is_empty() {
                parts.push(bg.clone());
            }
        }

        if let Some(distilled) = self.distilled_context {
            if !distilled.is_empty() {
                parts.push(format!("# Distilled Context\n\n{}", distilled));
            }
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n"))
        }
    }
}

/// Builds context frames with four-layer caching:
///
/// ```text
/// full_system = STABLE_PREFIX + SESSION_STATIC + SESSION_POLICY + DYNAMIC_SUFFIX
///               compile-time    once/session    mutable/session  every turn
/// ```
pub struct ContextCompiler {
    cached_static_prefix: String,      // stable + session_static (never changes)
    cached_session: SessionContext,     // owned session for policy recomputation
    estimator: ConservativeEstimator,   // calibrated token estimation
    token_limit: usize,                 // max context tokens for the model
}

impl ContextCompiler {
    pub fn new(session: &SessionContext) -> Self {
        let stable = stable::stable_prefix();
        let session_static = session.build_static_info();
        let cached_static_prefix = format!("{}\n\n{}", stable, session_static);

        Self {
            cached_static_prefix,
            estimator: ConservativeEstimator::default_conservative(),
            token_limit: 128_000,
            cached_session: SessionContext {
                os: session.os.clone(),
                shell: session.shell.clone(),
                cwd: session.cwd.clone(),
                available_cores: session.available_cores,
                mode: session.mode,
                yolo_mode: session.yolo_mode,
                skills: session.skills.clone(),
                custom_instructions: session.custom_instructions.clone(),
            },
        }
    }

    pub fn build_frame(
        &mut self,
        dynamic: &DynamicContext,
        messages: Vec<GatewayMessage>,
    ) -> ContextFrame {
        let dynamic_suffix = dynamic.to_string_content();

        // Recompute policy each frame from live session state
        let current_policy = self.cached_session.build_policy_info();

        // Assemble system: stable + session_static + session_policy + dynamic + tail_reminder
        let mut system = self.cached_static_prefix.clone();
        if let Some(ref policy) = current_policy {
            system.push_str(&format!("\n\n{}", policy));
        }
        if let Some(ref suffix) = dynamic_suffix {
            system.push_str(&format!("\n\n{}", suffix));
        }
        // Tail reminder: placed at end of system prompt to counter positional fragility
        if let Some(ref reminder) = dynamic.tail_reminder {
            if !reminder.is_empty() {
                system.push_str(&format!("\n\n{}", reminder));
            }
        }

        eprintln!("[di-core] frame: {} msgs, dynamic={}", messages.len(), dynamic_suffix.is_some());

        ContextFrame {
            system,
            tools: TOOL_DEFINITIONS.clone(),
            messages,
        }
    }

    pub fn session_prefix_len(&self) -> usize {
        let policy_len = self.cached_session.build_policy_info()
            .map(|p| p.len() + 2)
            .unwrap_or(0);
        self.cached_static_prefix.len() + policy_len
    }

    /// Get the model's token limit.
    pub fn token_limit(&self) -> usize {
        self.token_limit
    }

    /// Build the system string and measure its token cost from the current frame.
    /// Returns (system_string, system_tokens).
    pub fn build_system_string(&self, dynamic: &DynamicContext) -> (String, usize) {
        let dynamic_suffix = dynamic.to_string_content();
        let current_policy = self.cached_session.build_policy_info();

        let mut system = self.cached_static_prefix.clone();
        if let Some(ref policy) = current_policy {
            system.push_str(&format!("\n\n{}", policy));
        }
        if let Some(ref suffix) = dynamic_suffix {
            system.push_str(&format!("\n\n{}", suffix));
        }
        if let Some(ref reminder) = dynamic.tail_reminder {
            if !reminder.is_empty() {
                system.push_str(&format!("\n\n{}", reminder));
            }
        }

        let tokens = self.estimator.count_text(&system);
        (system, tokens)
    }

    /// Compute the history budget from the current frame's measured system + tools tokens.
    pub fn compute_history_budget(&self, system_tokens: usize, tools_tokens: usize) -> usize {
        const OUTPUT_RESERVE: usize = 4096;
        const SAFETY_MARGIN: usize = 256;
        const PROTOCOL_OVERHEAD: usize = 128;

        self.token_limit
            .saturating_sub(system_tokens)
            .saturating_sub(tools_tokens)
            .saturating_sub(OUTPUT_RESERVE)
            .saturating_sub(SAFETY_MARGIN)
            .saturating_sub(PROTOCOL_OVERHEAD)
    }

    /// Token count for the current tool definitions.
    pub fn tools_token_count(&self) -> usize {
        self.estimator.count_tools(&TOOL_DEFINITIONS)
    }

}

