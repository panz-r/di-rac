use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct HooksEditorState {
    pub source: String,
    pub cursor: usize,
    pub diagnostics: Vec<String>,
    pub preview: String,
    pub saved: bool,
    pub saving: bool,
    pub error: Option<String>,
    pub agent_id: Option<uuid::Uuid>,
}

impl HooksEditorState {
    pub fn new() -> Self {
        // Start with empty source — the message handler fills it from CoreEvent
        Self {
            source: String::new(),
            cursor: 0,
            diagnostics: Vec::new(),
            preview: String::new(),
            saved: true,
            saving: false,
            error: None,
            agent_id: None,
        }
    }

    pub fn source_byte_len(&self) -> usize {
        self.source.len()
    }

    pub fn type_char(&mut self, c: char) {
        let pos = self.cursor.min(self.source.len());
        self.source.insert(pos, c);
        self.cursor = pos + c.len_utf8();
        self.saved = false;
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.source[..self.cursor]
            .chars()
            .next_back()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
        let pos = self.cursor - prev;
        self.source.drain(pos..self.cursor);
        self.cursor = pos;
        self.saved = false;
    }

    pub fn delete(&mut self) {
        if self.cursor >= self.source.len() {
            return;
        }
        let next = self.source[self.cursor..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
        self.source.drain(self.cursor..self.cursor + next);
        self.saved = false;
    }

    pub fn move_left(&mut self) {
        if self.cursor == 0 { return; }
        let prev = self.source[..self.cursor]
            .chars()
            .next_back()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
        self.cursor -= prev;
    }

    pub fn move_right(&mut self) {
        if self.cursor >= self.source.len() { return; }
        let next = self.source[self.cursor..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
        self.cursor += next;
    }

    pub fn move_up(&mut self) {
        // Simple: go back one line
        if self.cursor == 0 { return; }
        let before = &self.source[..self.cursor];
        if let Some(pos) = before[..before.len().saturating_sub(1)].rfind('\n') {
            self.cursor = pos + 1;
        } else {
            self.cursor = 0;
        }
    }

    pub fn move_down(&mut self) {
        // Simple: go forward one line
        if self.cursor >= self.source.len() { return; }
        let after = &self.source[self.cursor..];
        if let Some(pos) = after.find('\n') {
            self.cursor += pos + 1;
        } else {
            self.cursor = self.source.len();
        }
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.source.len();
    }

    pub fn insert_newline(&mut self) {
        let pos = self.cursor.min(self.source.len());
        self.source.insert(pos, '\n');
        self.cursor = pos + 1;
        self.saved = false;
    }

    /// Update diagnostics from parse errors.
    pub fn set_diagnostics(&mut self, errors: Vec<String>) {
        self.diagnostics = errors;
    }

    /// Update preview text.
    pub fn set_preview(&mut self, text: String) {
        self.preview = text;
    }

    /// Save source as session overlay for the given agent id.
    /// Writes to ~/.dirac/sessions/<agent_id>/hooks.dhook
    pub fn save_session(source: &str, agent_id: &str) -> Result<(), String> {
        let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
        let dir = std::path::PathBuf::from(home)
            .join(".dirac").join("sessions").join(agent_id);
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create session dir: {}", e))?;
        let path = dir.join("hooks.dhook");
        std::fs::write(&path, source).map_err(|e| format!("Cannot write session hook: {}", e))?;
        Ok(())
    }

    /// Save source as repo hook.
    /// Writes to .dirac/agent.dhook in the current directory.
    pub fn save_repo(source: &str) -> Result<String, String> {
        let cwd = std::env::current_dir().map_err(|e| format!("Cannot get cwd: {}", e))?;
        let dir = cwd.join(".dirac");
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create .dirac dir: {}", e))?;
        let path = dir.join("agent.dhook");
        let path_str = path.to_string_lossy().to_string();
        std::fs::write(&path, source).map_err(|e| format!("Cannot write repo hook: {}", e))?;
        Ok(path_str)
    }

    /// Validate .dhook source syntax.
    /// Returns a list of error messages (empty = valid).
    pub fn validate(source: &str) -> Vec<String> {
        // Simple syntax check: count balanced parens and brackets
        let mut parens = 0i32;
        let mut brackets = 0i32;
        let mut braces = 0i32;
        let mut errors = Vec::new();

        for (i, line) in source.lines().enumerate() {
            for c in line.chars() {
                match c {
                    '(' => parens += 1,
                    ')' => parens -= 1,
                    '[' => brackets += 1,
                    ']' => brackets -= 1,
                    '{' => braces += 1,
                    '}' => braces -= 1,
                    _ => {}
                }
            }
        }

        if parens > 0 {
            errors.push(format!("{} unmatched opening parenthesis", parens));
        } else if parens < 0 {
            errors.push(format!("{} unmatched closing parenthesis", -parens));
        }
        if brackets > 0 {
            errors.push(format!("{} unmatched opening bracket", brackets));
        } else if brackets < 0 {
            errors.push(format!("{} unmatched closing bracket", -brackets));
        }
        if braces > 0 {
            errors.push(format!("{} unmatched opening brace", braces));
        } else if braces < 0 {
            errors.push(format!("{} unmatched closing brace", -braces));
        }

        if source.is_empty() {
            errors.push("Source is empty".to_string());
        }

        errors
    }
}
