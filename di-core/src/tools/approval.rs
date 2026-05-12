use std::collections::HashSet;

/// Manages tool approval decisions.
pub struct ApprovalManager {
    pub yolo_mode: bool,
    /// Per-tool auto-approve overrides from settings.
    pub auto_approve: HashSet<String>,
}

impl ApprovalManager {
    pub fn new() -> Self {
        Self {
            yolo_mode: false,
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

    /// Returns true if the tool should auto-approve.
    pub fn should_auto_approve(&self, tool: &str) -> bool {
        if self.yolo_mode {
            return true;
        }

        if self.auto_approve.contains(tool) {
            return true;
        }

        Self::READ_TOOLS.contains(&tool)
    }
}
