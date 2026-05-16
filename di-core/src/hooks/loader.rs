use std::path::PathBuf;
use crate::hooks::parser::{Parser, Module};
use crate::hooks::compiler::HookCompiler;
use crate::hooks::ir::CompiledHookModule;

/// Discovers and loads .dhook files from standard paths.
pub struct HookLoader {
    repo_path: Option<PathBuf>,
    session_overlay_path: Option<PathBuf>,
    loaded_source: Option<String>,
}

impl HookLoader {
    pub fn new() -> Self {
        let repo_path = Self::find_repo_hook();
        Self {
            repo_path,
            session_overlay_path: None,
            loaded_source: None,
        }
    }

    pub fn set_session_overlay(&mut self, path: PathBuf) {
        self.session_overlay_path = Some(path);
    }

    pub fn clear_session_overlay(&mut self) {
        self.session_overlay_path = None;
    }

    /// Find .di/hooks/agent.dhook in current or parent directories.
    fn find_repo_hook() -> Option<PathBuf> {
        let cwd = std::env::current_dir().ok()?;
        let mut dir = Some(cwd.as_path());
        while let Some(d) = dir {
            let candidate = d.join(".di").join("hooks").join("agent.dhook");
            if candidate.exists() {
                return Some(candidate);
            }
            dir = d.parent();
        }
        None
    }

    /// Load and compile the hook module from all sources.
    pub fn load(&mut self) -> Result<CompiledHookModule, Vec<String>> {
        let mut source_parts = Vec::new();

        // 1. Repo hook
        if let Some(ref path) = self.repo_path {
            match std::fs::read_to_string(path) {
                Ok(text) => source_parts.push(text),
                Err(e) => return Err(vec![format!("Failed to read repo hook {}: {}", path.display(), e)]),
            }
        }

        // 2. Session overlay (if active)
        if let Some(ref path) = self.session_overlay_path {
            if path.exists() {
                match std::fs::read_to_string(path) {
                    Ok(text) => source_parts.push(text),
                    Err(e) => return Err(vec![format!("Failed to read session hook {}: {}", path.display(), e)]),
                }
            }
        }

        if source_parts.is_empty() {
            return Ok(CompiledHookModule {
                id: "empty".to_string(),
                source_hash: "empty".to_string(),
                groups: Vec::new(),
                roles: Vec::new(),
                handlers: Vec::new(),
            });
        }

        let combined = source_parts.join("\n\n");
        self.loaded_source = Some(combined.clone());
        let hash = crate::util::stable_hash(combined.as_bytes());
        let source_hash = format!("{:.16}", hash);

        let parsed = match self.parse(&combined) {
            Ok(m) => m,
            Err(errors) => return Err(errors),
        };

        let compiled = HookCompiler::compile(&parsed)?;

        Ok(CompiledHookModule {
            id: self.repo_path.as_ref()
                .and_then(|p| p.parent().and_then(|pp| pp.file_name()).map(|n| n.to_string_lossy().to_string()))
                .unwrap_or_else(|| "session".to_string()),
            source_hash,
            ..compiled
        })
    }

    /// Re-load with a new source text (for live editing).
    pub fn load_from_text(&self, source: &str) -> Result<CompiledHookModule, Vec<String>> {
        let hash = crate::util::stable_hash(source.as_bytes());
        let source_hash = format!("{:.16}", hash);

        let parsed = self.parse(source)?;
        let mut compiled = HookCompiler::compile(&parsed)?;
        compiled.id = "session".to_string();
        compiled.source_hash = source_hash;

        Ok(compiled)
    }

    /// Full source text for the combined hook module.
    /// Uses cached source from last load() call instead of re-reading from disk.
    pub fn full_source(&self) -> Option<String> {
        self.loaded_source.clone()
    }

    /// Get the repo-level hooks directory (.di/hooks/ in cwd).
    pub fn hooks_dir() -> PathBuf {
        std::env::current_dir().unwrap_or_default().join(".di").join("hooks")
    }

    /// Get the user-level hooks directory (~/.di/hooks/).
    pub fn user_hooks_dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        PathBuf::from(home).join(".di").join("hooks")
    }

    /// Save session overlay hook source to disk under ~/.di/hooks/.
    pub fn save_session_overlay(source: &str, agent_id: &str) -> Result<(), String> {
        let dir = Self::user_hooks_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create hooks dir: {}", e))?;
        let path = dir.join(format!("{}.dhook", agent_id));
        std::fs::write(&path, source).map_err(|e| format!("Failed to write session hook: {}", e))?;
        Ok(())
    }

    /// Save session overlay and return the path used.
    pub fn save_session_overlay_path(source: &str, agent_id: &str) -> Result<PathBuf, String> {
        let dir = Self::user_hooks_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create hooks dir: {}", e))?;
        let path = dir.join(format!("{}.dhook", agent_id));
        std::fs::write(&path, source).map_err(|e| format!("Failed to write session hook: {}", e))?;
        Ok(path)
    }

    /// Load session overlay from ~/.di/hooks/ for a given agent id.
    pub fn load_session_overlay(agent_id: &str) -> Option<String> {
        let path = Self::user_hooks_dir().join(format!("{}.dhook", agent_id));
        if path.exists() {
            std::fs::read_to_string(&path).ok()
        } else {
            None
        }
    }

    /// Save repo hook to .di/hooks/agent.dhook in the current directory.
    pub fn save_repo_hook(source: &str) -> Result<PathBuf, String> {
        let dir = Self::hooks_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create hooks dir: {}", e))?;
        let path = dir.join("agent.dhook");
        std::fs::write(&path, source).map_err(|e| format!("Failed to write repo hook: {}", e))?;
        Ok(path)
    }

    fn parse(&self, source: &str) -> Result<Module, Vec<String>> {
        let mut parser = Parser::new(source);
        match parser.parse_module() {
            Ok(module) => Ok(module),
            Err(errors) => {
                Err(errors.into_iter().map(|e| {
                    format!("Line {}:{}: {}", e.span.line, e.span.column, e.message)
                }).collect())
            }
        }
    }
}
