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
        let cursor = self.cursor.min(self.source.len());
        if cursor == 0 {
            return;
        }
        let prev = self.source[..cursor]
            .chars()
            .next_back()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
        let pos = cursor.saturating_sub(prev);
        let end = cursor;
        self.source.drain(pos..end);
        self.cursor = pos;
        self.saved = false;
    }

    pub fn delete(&mut self) {
        let cursor = self.cursor.min(self.source.len());
        if cursor >= self.source.len() {
            return;
        }
        let next = self.source[cursor..]
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
    /// Writes to ~/.di/hooks/<agent_id>.dhook
    pub fn save_session(source: &str, agent_id: &str) -> Result<(), String> {
        let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
        let dir = std::path::PathBuf::from(home).join(".di").join("hooks");
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create hooks dir: {}", e))?;
        let path = dir.join(format!("{}.dhook", agent_id));
        std::fs::write(&path, source).map_err(|e| format!("Cannot write session hook: {}", e))?;
        Ok(())
    }

    /// Save source as repo hook.
    /// Writes to .di/hooks/agent.dhook in the current directory.
    pub fn save_repo(source: &str) -> Result<String, String> {
        let cwd = std::env::current_dir().map_err(|e| format!("Cannot get cwd: {}", e))?;
        let dir = cwd.join(".di").join("hooks");
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create hooks dir: {}", e))?;
        let path = dir.join("agent.dhook");
        let path_str = path.to_string_lossy().to_string();
        std::fs::write(&path, source).map_err(|e| format!("Cannot write repo hook: {}", e))?;
        Ok(path_str)
    }

    /// Load session overlay from ~/.di/hooks/ for a given agent id.
    pub fn load_session(agent_id: &str) -> Option<String> {
        let home = std::env::var("HOME").ok()?;
        let path = std::path::PathBuf::from(home).join(".di").join("hooks").join(format!("{}.dhook", agent_id));
        if path.exists() {
            std::fs::read_to_string(&path).ok()
        } else {
            None
        }
    }

    /// Normalize source by inserting newlines where statements are run together
    /// (e.g. after paste without newlines). Handles:
    /// - `)def` → `)\ndef`
    /// - `)@on` → `)\n@on`
    /// - `):    hint` → `):\n    hint` (after colon in def/if/for)
    /// - `)    audit` → `)\n    audit` (new statement after a function call)
    /// Known limitation: nested calls like `fn(inner())` may get split incorrectly.
    pub fn normalize_source(source: &str) -> String {
        let mut result = String::with_capacity(source.len() + 16);
        let chars: Vec<char> = source.chars().collect();
        let mut i = 0;

        // Known keywords that start new statements (not identifiers in expressions)
        let stmt_keywords = ["hint", "criterion", "warn", "approval_note",
            "require_validation", "trigger_observer", "trigger_planner_review",
            "require_evidence", "require_final_note", "remember", "audit",
            "block_finish_until", "if", "for", "return",
        ];

        while i < chars.len() {
            let c = chars[i];
            if c == ')' {
                result.push(')');
                let mut j = i + 1;
                while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') { j += 1; }
                let after = if j < chars.len() { chars[j] } else { '\0' };
                if after == 'd' && j + 2 < chars.len() && chars[j+1] == 'e' && chars[j+2] == 'f' {
                    result.push('\n');
                } else if after == '@' {
                    result.push('\n');
                } else if after.is_alphabetic() {
                    // Check if this starts a known statement keyword
                    let word: String = chars[j..].iter().take_while(|c| c.is_alphabetic()).collect();
                    if stmt_keywords.contains(&word.as_str()) {
                        result.push('\n');
                        // Preserve indentation: copy the leading whitespace from context
                        let indent = if !result.is_empty() {
                            let last_line = result.rsplit('\n').next().unwrap_or("");
                            let leading_spaces: usize = last_line.chars().take_while(|c| *c == ' ').count();
                            leading_spaces
                        } else { 0 };
                        for _ in 0..indent { result.push(' '); }
                    } else {
                        for k in i+1..j { result.push(chars[k]); }
                    }
                } else {
                    for k in i+1..j { result.push(chars[k]); }
                }
                i = j;
            } else if c == ':' {
                result.push(':');
                let mut j = i + 1;
                while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') { j += 1; }
                if j < chars.len() && chars[j] != '\n' && j > i + 1 {
                    result.push('\n');
                    // Indent 4 more than current context
                    let indent = if !result.is_empty() {
                        let last_line = result.rsplit('\n').next().unwrap_or("");
                        let leading = last_line.chars().take_while(|c| *c == ' ').count();
                        leading + 4
                    } else { 4 };
                    for _ in 0..indent { result.push(' '); }
                } else {
                    for k in i+1..j { result.push(chars[k]); }
                }
                i = j;
            } else {
                result.push(c);
                i += 1;
            }
        }
        result
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
