use std::collections::HashSet;

/// Manages tool approval decisions.
pub struct ApprovalManager {
    /// Per-tool auto-approve overrides from settings.
    pub auto_approve: HashSet<String>,
}

impl ApprovalManager {
    pub fn new() -> Self {
        Self {
            auto_approve: HashSet::new(),
        }
    }

    /// Read-only tools that auto-approve by default (v9.5.1 wire names).
    const READ_TOOLS: &[&str] = &[
        "read",
        "search",
        "repo",
        "symbols",
        "ask",
        "compact",
        "tools",
    ];

    /// Safe bash commands that are read-only and auto-approvable.
    const SAFE_BASH_COMMANDS: &[&str] = &[
        "ls", "cat", "grep", "find", "head", "tail", "wc", "echo",
        "which", "pwd", "dirname", "basename", "realpath", "readlink",
        "stat", "file", "sort", "uniq", "cut", "tr", "diff",
        "type", "printenv", "env", "date", "cal", "nproc",
    ];

    /// Returns true if the tool should auto-approve.
    pub fn should_auto_approve(&self, tool: &str) -> bool {
        if self.auto_approve.contains(tool) {
            return true;
        }

        if Self::READ_TOOLS.contains(&tool) {
            return true;
        }

        // Auto-approve read-only bash commands
        if tool == "bash" {
            return false; // bash itself requires approval; individual safe commands handled below
        }

        false
    }

    /// Check if a bash command is safe (read-only) and can skip approval.
    pub fn is_safe_bash_command(command: &str) -> bool {
        let trimmed = command.trim();
        Self::SAFE_BASH_COMMANDS.iter().any(|safe| {
            trimmed == *safe || trimmed.starts_with(&format!("{} ", safe))
        })
    }
}
