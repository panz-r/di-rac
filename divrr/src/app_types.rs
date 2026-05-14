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
];

/// State for the save-block-to-file dialog.
#[derive(Debug, Clone)]
pub struct SaveDialogState {
    pub cursor: usize,
    pub path: String,
    pub exists_warned: bool,
    pub block_text: String,
}
