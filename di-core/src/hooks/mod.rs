pub mod directive;
pub mod parser;
pub mod compiler;
pub mod ir;
pub mod evaluator;
pub mod loader;

use std::path::PathBuf;
use std::sync::Arc;
use arc_swap::ArcSwap;
use crate::hooks::directive::*;
use crate::hooks::evaluator::{HookEvaluator, EvalContext, AgentHookLimits, EvalResult};
use crate::hooks::ir::CompiledHookModule;
use crate::hooks::loader::HookLoader;

/// Top-level hook manager. One per agent.
pub struct AgentHookManager {
    loader: HookLoader,
    active: ArcSwap<CompiledHookModule>,
    limits: AgentHookLimits,
    merged: MergedDirectives,
    agent_id: Option<String>,
}

impl AgentHookManager {
    pub fn new() -> Self {
        Self::with_agent_id(None)
    }

    /// Create a hook manager that auto-discovers the session overlay for the given agent.
    pub fn with_agent_id(agent_id: Option<String>) -> Self {
        let mut loader = HookLoader::new();
        // Auto-discover session overlay from ~/.di/hooks/<agent_id>.dhook
        if let Some(ref id) = agent_id {
            let overlay_path = crate::hooks::loader::HookLoader::user_hooks_dir()
                .join(format!("{}.dhook", id));
            if overlay_path.exists() {
                loader.set_session_overlay(overlay_path);
            }
        }
        let active = match loader.load() {
            Ok(module) => module,
            Err(errors) => {
                eprintln!("[di-core] hooks: load failed (using empty module): {}", errors.join("; "));
                CompiledHookModule {
                    id: "empty".to_string(),
                    source_hash: "empty".to_string(),
                    groups: Vec::new(),
                    roles: Vec::new(),
                    handlers: Vec::new(),
                }
            }
        };

        Self {
            loader,
            active: ArcSwap::new(Arc::new(active)),
            limits: AgentHookLimits::default(),
            merged: MergedDirectives::default(),
            agent_id,
        }
    }

    /// Reload from disk (repo + session overlay).
    pub fn reload(&mut self) -> Result<(), Vec<String>> {
        let module = self.loader.load()?;
        self.active.store(Arc::new(module));
        Ok(())
    }

    /// Reload hooks, discovering the session overlay from agent id.
    pub fn reload_session(&mut self) -> Result<(), Vec<String>> {
        if let Some(ref id) = self.agent_id {
            let overlay_path = crate::hooks::loader::HookLoader::user_hooks_dir()
                .join(format!("{}.dhook", id));
            // Only set overlay if the file exists
            if overlay_path.exists() {
                self.loader.set_session_overlay(overlay_path);
            } else {
                // No session overlay — clear any previous session overlay path
                // so only the repo hook is loaded
                self.loader.clear_session_overlay();
            }
        }
        self.reload()
    }

    /// Apply a new session overlay source (from TUI editing).
    pub fn apply_session_hook(&self, source: &str) -> Result<Arc<CompiledHookModule>, Vec<String>> {
        let module = self.loader.load_from_text(source)?;
        let arc = Arc::new(module);
        Ok(arc)
    }

    /// Hot-swap the active module (for live TUI editing).
    /// Returns the previous module that was replaced.
    pub fn swap_active(&self, module: Arc<CompiledHookModule>) -> Arc<CompiledHookModule> {
        self.active.swap(module)
    }

    /// Get the current active module.
    pub fn active_module(&self) -> Arc<CompiledHookModule> {
        self.active.load_full()
    }

    /// Save session overlay hook source to disk.
    /// Returns the path where it was saved.
    pub fn save_session_hook(source: &str, agent_id: &str) -> Result<PathBuf, String> {
        crate::hooks::loader::HookLoader::save_session_overlay_path(source, agent_id)
    }

    /// Save repo hook to .dirac/agent.dhook.
    /// Returns the path where it was saved.
    pub fn save_repo_hook(source: &str) -> Result<PathBuf, String> {
        crate::hooks::loader::HookLoader::save_repo_hook(source)
    }

    /// Load session overlay from disk for a given agent id.
    pub fn load_session_overlay(agent_id: &str) -> Option<String> {
        crate::hooks::loader::HookLoader::load_session_overlay(agent_id)
    }

    /// Fire an event through the hook system.
    pub fn on_event(&mut self, event: AgentLoopEvent) -> EvalResult {
        let module = self.active.load_full();
        let ctx = Self::build_context(&event);

        let result = HookEvaluator::evaluate(&module, &ctx, &self.limits);

        // Merge directives into accumulated state
        DirectiveMerger::merge(&mut self.merged, result.directives.clone());

        result
    }

    /// Get accumulated merged directives.
    pub fn merged_directives(&self) -> &MergedDirectives {
        &self.merged
    }

    /// Reset merged state (e.g., at session_start).
    pub fn reset(&mut self) {
        self.merged = MergedDirectives::default();
    }

    /// Clear only finish gates and evidence requirements. Leaves hints, criteria,
    /// validations, audit events, etc. intact for subsequent turns.
    pub fn clear_finish_gates(&mut self) {
        self.merged.finish_gates.clear();
        self.merged.evidence_required.clear();
        self.merged.final_notes.clear();
    }

    /// Get role definitions from the active module.
    pub fn roles(&self) -> Vec<ir::RoleDef> {
        self.active.load_full().roles.clone()
    }

    /// Take pending observer triggers (clears them from accumulated state).
    pub fn take_observer_triggers(&mut self) -> Vec<ObserverTrigger> {
        std::mem::take(&mut self.merged.observer_triggers)
    }

    /// Full source of repo + session overlay for TUI display.
    pub fn full_source(&self) -> Option<String> {
        self.loader.full_source()
    }

    fn build_context(event: &AgentLoopEvent) -> EvalContext {
        let (event_name, changed_files, tool_name, tool_success, observer_id, observer_output) = match &event {
            AgentLoopEvent::SessionStart => ("session_start", vec![], None, None, None, None),
            AgentLoopEvent::PostToolUse { tool_name: tn, changed_files: cf, success } => {
                ("post_tool_use", cf.clone(), Some(tn.clone()), Some(*success), None, None)
            }
            AgentLoopEvent::ObserverResult { observer_id: oid, output } => {
                ("observer_result", vec![], None, None, Some(oid.clone()), Some(output.clone()))
            }
            AgentLoopEvent::PreFinish => ("pre_finish", vec![], None, None, None, None),
            AgentLoopEvent::UserPrompt { .. } => ("user_prompt", vec![], None, None, None, None),
            AgentLoopEvent::PlanCreated { files, .. } => ("plan_created", files.clone(), None, None, None, None),
            AgentLoopEvent::ValidationResult { .. } => ("validation_result", vec![], None, None, None, None),
            AgentLoopEvent::PreCompact { .. } => ("pre_compact", vec![], None, None, None, None),
            AgentLoopEvent::TaskComplete { .. } => ("task_complete", vec![], None, None, None, None),
            AgentLoopEvent::ErrorOccurred { tool_name: tn, .. } => ("error_occurred", vec![], tn.clone(), None, None, None),
        };

        EvalContext {
            hook_id: "repo".to_string(),
            event: event_name.to_string(),
            changed_files,
            tool_name,
            tool_success,
            observer_id,
            observer_output,
        }
    }
}
