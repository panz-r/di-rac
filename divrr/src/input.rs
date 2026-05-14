/// Maximum input buffer size (1 MiB). Pastes and keystrokes beyond this are silently dropped.
const MAX_INPUT_BYTES: usize = 1_048_576;

pub struct InputBuffer {
    pub content: String,
    pub cursor: usize,
    pub multi_line: bool,
    pub scroll_offset: usize,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
}

impl InputBuffer {
    pub fn new() -> Self {
        Self {
            content: String::new(),
            cursor: 0,
            multi_line: false,
            scroll_offset: 0,
            history: Vec::new(),
            history_index: None,
        }
    }

    pub fn insert(&mut self, c: char) {
        if self.content.len() + c.len_utf8() > MAX_INPUT_BYTES {
            return;
        }
        self.content.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Insert an entire string at the cursor in O(n) instead of O(n*m) via repeated `insert`.
    pub fn insert_str(&mut self, s: &str) {
        let available = MAX_INPUT_BYTES.saturating_sub(self.content.len());
        if available == 0 || s.is_empty() {
            return;
        }
        let insert = if s.len() <= available {
            s
        } else {
            // Find the longest char-boundary-aligned prefix that fits
            let mut end = available;
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            &s[..end]
        };
        self.content.insert_str(self.cursor, insert);
        self.cursor += insert.len();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.content[..self.cursor]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor -= prev;
            self.content.remove(self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.content.len() {
            self.content.remove(self.cursor);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            let prev = self.content[..self.cursor]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor -= prev;
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.content.len() {
            let next = self.content[self.cursor..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor += next;
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.content.len();
    }

    pub fn clear(&mut self) {
        self.content.clear();
        self.cursor = 0;
        self.scroll_offset = 0;
        self.history_index = None;
    }

    pub fn submit(&mut self) -> String {
        let text = self.content.clone();
        if !text.is_empty() {
            self.history.push(text.clone());
            // Cap history at 100 entries
            if self.history.len() > 100 {
                self.history.remove(0);
            }
        }
        self.clear();
        text
    }

    pub fn toggle_multi_line(&mut self) {
        self.multi_line = !self.multi_line;
        self.scroll_offset = 0;
    }

    pub fn cursor_row(&self) -> usize {
        let up_to = self.cursor.min(self.content.len());
        self.content[..up_to].lines().count().max(1) - 1
    }

    pub fn clamp_scroll(&mut self, visible_h: usize) {
        if visible_h == 0 {
            return;
        }
        let row = self.cursor_row();
        if row < self.scroll_offset {
            self.scroll_offset = row;
        } else if row >= self.scroll_offset + visible_h {
            self.scroll_offset = row - visible_h + 1;
        }
    }

    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let idx = self.history_index.unwrap_or(self.history.len());
        if idx > 0 {
            self.history_index = Some(idx - 1);
            self.content = self.history[idx - 1].clone();
            self.cursor = self.content.len();
            if self.content.contains('\n') {
                self.multi_line = true;
            }
        }
    }

    pub fn history_down(&mut self) {
        if let Some(idx) = self.history_index {
            if idx + 1 < self.history.len() {
                self.history_index = Some(idx + 1);
                self.content = self.history[idx + 1].clone();
                self.cursor = self.content.len();
                if self.content.contains('\n') {
                    self.multi_line = true;
                }
            } else {
                self.history_index = None;
                self.content.clear();
                self.cursor = 0;
            }
        }
    }
}
