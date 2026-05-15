/// UI input modes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Normal,
    Insert,
    Command,
    Action,
    Settings,
    SaveDialog,
}

/// Actions available in the spacebar action palette.
pub const BLOCK_ACTIONS: &[&str] = &["Expand", "Save", "Copy", "Wrap"];

/// Number of actions — derived from the list so adding a new one only requires
/// updating BLOCK_ACTIONS.
pub const BLOCK_ACTION_COUNT: usize = BLOCK_ACTIONS.len();

/// A command that can be run from the command palette.
#[derive(Debug, Clone)]
pub struct CommandEntry {
    pub name: &'static str,
    pub description: &'static str,
    pub prefix: &'static str,
}

pub(crate) const COMMANDS: &[CommandEntry] = &[
    CommandEntry { name: "settings", description: "Open provider settings panel", prefix: "" },
    CommandEntry { name: "quit", description: "Exit divrr", prefix: "q" },
    CommandEntry { name: "interrupt", description: "Interrupt active agent", prefix: "" },
    CommandEntry { name: "new", description: "Spawn a new agent with a task", prefix: "" },
    CommandEntry { name: "close", description: "Close active agent tab", prefix: "" },
    CommandEntry { name: "plan", description: "Switch active agent to Plan mode (read-only)", prefix: "" },
    CommandEntry { name: "act", description: "Switch active agent to Act mode (full tool access)", prefix: "" },
];

/// State for the save-block-to-file dialog.
#[derive(Debug, Clone)]
pub struct SaveDialogState {
    pub cursor: usize,
    pub path: String,
    pub exists_warned: bool,
    pub block_text: String,
}
