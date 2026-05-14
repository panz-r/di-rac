use crate::agent::engine::AgentMode;

/// Session-level context resolved once per agent lifetime.
/// These values don't change between turns.
pub struct SessionContext {
    // Static: OS, shell, CWD, CPU — truly immutable
    pub os: String,
    pub shell: String,
    pub cwd: String,
    pub available_cores: usize,

    // Policy: mode, instructions, skills — can change during a session
    pub mode: AgentMode,
    pub skills: Option<String>,
    pub custom_instructions: Option<String>,
}

impl SessionContext {
    /// Build the static portion (OS, shell, CWD, CPU, path rules).
    /// This is hashed separately and never invalidated.
    pub fn build_static_info(&self) -> String {
        let sys_info = format!(
            "SYSTEM INFO\n\n\
- Operating System: {}\n\
- Default Shell: {}\n\
- You are running in a full-featured shell environment. You have access to \
standard Unix tools (`grep`, `sed`, `awk`, `find`, `xargs`, etc.).\n\
- Current Working Directory: {} (this is where all the tools will be executed from)\n\
- Workspace Root: {}\n\
- PROJECT-RELATIVE PATHS: All file paths you provide MUST be project-relative \
(e.g., 'src/main.ts', not '/absolute/path/src/main.ts'). Absolute paths are \
strictly forbidden and will be blocked by the system.\n\
- Available CPU Cores: {} (Use this value for parallel jobs like 'make -j' instead of 'nproc')",
            self.os, self.shell, self.cwd, self.cwd, self.available_cores
        );
        sys_info
    }

    /// Build the mutable policy portion (mode, skills, custom instructions).
    /// This is hashed separately and can be recomputed if config changes.
    pub fn build_policy_info(&self) -> Option<String> {
        let mut parts = Vec::new();

        if self.mode == AgentMode::Plan {
            parts.push(
                "[Plan Mode Active] You may only use read-only tools, ask, done, plan, and compact. \
Do not modify any files.".to_string()
            );
        }

        if let Some(skills) = &self.skills {
            if !skills.is_empty() {
                parts.push(format!("# SKILLS\n\n{}", skills));
            }
        }

        if let Some(instructions) = &self.custom_instructions {
            if !instructions.is_empty() {
                parts.push(format!("# USER'S CUSTOM INSTRUCTIONS\n\n{}", instructions));
            }
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n"))
        }
    }

}
