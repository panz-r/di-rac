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
}

impl AgentHookManager {
    pub fn new() -> Self {
        let mut loader = HookLoader::new();
        let active = loader.load().unwrap_or_else(|_| CompiledHookModule {
            id: "empty".to_string(),
            source_hash: "empty".to_string(),
            groups: Vec::new(),
            roles: Vec::new(),
            handlers: Vec::new(),
        });

        Self {
            loader,
            active: ArcSwap::new(Arc::new(active)),
            limits: AgentHookLimits::default(),
            merged: MergedDirectives::default(),
        }
    }

    /// Reload from disk (repo + session overlay).
    pub fn reload(&mut self) -> Result<(), Vec<String>> {
        let module = self.loader.load()?;
        self.active.store(Arc::new(module));
        Ok(())
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
