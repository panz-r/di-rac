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

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Basic operations
    // -----------------------------------------------------------------------
    #[test]
    fn new_is_empty() {
        let buf = InputBuffer::new();
        assert!(buf.content.is_empty());
        assert_eq!(buf.cursor, 0);
        assert!(!buf.multi_line);
    }

    #[test]
    fn insert_ascii() {
        let mut buf = InputBuffer::new();
        buf.insert('a');
        buf.insert('b');
        buf.insert('c');
        assert_eq!(buf.content, "abc");
        assert_eq!(buf.cursor, 3);
    }

    #[test]
    fn insert_multibyte() {
        let mut buf = InputBuffer::new();
        buf.insert('é'); // 2 bytes
        buf.insert('€'); // 3 bytes
        assert_eq!(buf.content, "é€");
        assert_eq!(buf.cursor, 5);
    }

    #[test]
    fn insert_at_cursor_position() {
        let mut buf = InputBuffer::new();
        buf.insert('a');
        buf.insert('c');
        buf.move_left();
        buf.insert('b');
        assert_eq!(buf.content, "abc");
    }

    #[test]
    fn insert_past_max_is_dropped() {
        let mut buf = InputBuffer::new();
        // Fill to just under limit
        let base = "a".repeat(MAX_INPUT_BYTES);
        buf.insert_str(&base);
        assert_eq!(buf.content.len(), MAX_INPUT_BYTES);
        // Any further insert beyond limit is dropped
        buf.insert('x');
        assert_eq!(buf.content.len(), MAX_INPUT_BYTES);
    }

    // -----------------------------------------------------------------------
    // Backspace
    // -----------------------------------------------------------------------
    #[test]
    fn backspace_ascii() {
        let mut buf = InputBuffer::new();
        buf.insert_str("abc");
        buf.backspace();
        assert_eq!(buf.content, "ab");
        assert_eq!(buf.cursor, 2);
    }

    #[test]
    fn backspace_multibyte() {
        let mut buf = InputBuffer::new();
        buf.insert('a');
        buf.insert('é'); // 2 bytes
        buf.backspace();
        assert_eq!(buf.content, "a");
        assert_eq!(buf.cursor, 1);
    }

    #[test]
    fn backspace_at_start_is_noop() {
        let mut buf = InputBuffer::new();
        buf.backspace(); // cursor = 0, no-op
        assert!(buf.content.is_empty());
        assert_eq!(buf.cursor, 0);
    }

    #[test]
    fn backspace_after_multi_byte_move() {
        let mut buf = InputBuffer::new();
        buf.insert_str("aé€b");
        // Content: 'a'=1, 'é'=2, '€'=3, 'b'=1 = 7 bytes
        // Cursor at end (7). Move left past 'b', then past '€' to before '€'.
        buf.move_left(); // cursor=6 (before 'b')
        buf.move_left(); // cursor=3 (before '€')
        buf.backspace(); // removes 'é' (bytes 1-2), cursor→1
        assert_eq!(buf.content, "a€b");
        assert_eq!(buf.cursor, 1);
    }

    // -----------------------------------------------------------------------
    // Delete
    // -----------------------------------------------------------------------
    #[test]
    fn delete_ascii() {
        let mut buf = InputBuffer::new();
        buf.insert_str("abc");
        buf.move_home();
        buf.delete();
        assert_eq!(buf.content, "bc");
    }

    #[test]
    fn delete_multibyte() {
        let mut buf = InputBuffer::new();
        buf.insert_str("aé€b");
        buf.move_home();
        buf.delete(); // removes 'a'
        assert_eq!(buf.content, "é€b");
    }

    #[test]
    fn delete_at_end_is_noop() {
        let mut buf = InputBuffer::new();
        buf.insert_str("abc");
        buf.delete(); // cursor at end
        assert_eq!(buf.content, "abc");
    }

    // -----------------------------------------------------------------------
    // Cursor movement
    // -----------------------------------------------------------------------
    #[test]
    fn move_home_and_end() {
        let mut buf = InputBuffer::new();
        buf.insert_str("hello");
        buf.move_home();
        assert_eq!(buf.cursor, 0);
        buf.move_end();
        assert_eq!(buf.cursor, 5);
    }

    #[test]
    fn move_left_right_ascii() {
        let mut buf = InputBuffer::new();
        buf.insert_str("ab");
        buf.move_home();
        assert_eq!(buf.cursor, 0);
        buf.move_right();
        assert_eq!(buf.cursor, 1);
        buf.move_right();
        assert_eq!(buf.cursor, 2);
        buf.move_left();
        assert_eq!(buf.cursor, 1);
    }

    #[test]
    fn move_left_right_multibyte() {
        let mut buf = InputBuffer::new();
        buf.insert_str("aé€b");
        // bytes: a=0, é=1-2, €=3-5, b=6
        buf.move_home(); // cursor=0
        buf.move_right(); // past 'a' → cursor=1
        assert_eq!(buf.cursor, 1);
        buf.move_right(); // past 'é' → cursor=3
        assert_eq!(buf.cursor, 3);
        buf.move_right(); // past '€' → cursor=6
        assert_eq!(buf.cursor, 6);
        buf.move_left(); // back before 'b' → cursor=3
        assert_eq!(buf.cursor, 3);
    }

    #[test]
    fn move_left_at_start_is_noop() {
        let mut buf = InputBuffer::new();
        buf.insert_str("a");
        buf.move_home();
        buf.move_left(); // no-op
        assert_eq!(buf.cursor, 0);
    }

    #[test]
    fn move_right_at_end_is_noop() {
        let mut buf = InputBuffer::new();
        buf.insert_str("a");
        buf.move_end();
        buf.move_right(); // no-op
        assert_eq!(buf.cursor, 1);
    }

    // -----------------------------------------------------------------------
    // insert_str
    // -----------------------------------------------------------------------
    #[test]
    fn insert_str_empty_is_noop() {
        let mut buf = InputBuffer::new();
        buf.insert_str("");
        assert!(buf.content.is_empty());
    }

    #[test]
    fn insert_str_past_max_is_truncated() {
        let mut buf = InputBuffer::new();
        let base = "a".repeat(MAX_INPUT_BYTES - 5);
        buf.insert_str(&base);
        let extra = "bbbbbbbbbb"; // 10 bytes, only 5 remaining
        buf.insert_str(extra);
        assert_eq!(buf.content.len(), MAX_INPUT_BYTES - 5 + 5);
    }

    // -----------------------------------------------------------------------
    // Submit and history
    // -----------------------------------------------------------------------
    #[test]
    fn submit_clears_and_returns() {
        let mut buf = InputBuffer::new();
        buf.insert_str("hello");
        let text = buf.submit();
        assert_eq!(text, "hello");
        assert!(buf.content.is_empty());
        assert_eq!(buf.cursor, 0);
    }

    #[test]
    fn submit_adds_to_history() {
        let mut buf = InputBuffer::new();
        buf.insert_str("cmd1");
        buf.submit();
        assert_eq!(buf.history.len(), 1);
        assert_eq!(buf.history[0], "cmd1");
    }

    #[test]
    fn history_up_down() {
        let mut buf = InputBuffer::new();
        buf.insert_str("first");
        buf.submit();
        buf.insert_str("second");
        buf.submit();
        assert_eq!(buf.history.len(), 2);

        buf.history_up();
        assert_eq!(buf.content, "second");
        buf.history_up();
        assert_eq!(buf.content, "first");
        buf.history_down();
        assert_eq!(buf.content, "second");
    }

    #[test]
    fn history_empty_is_noop() {
        let mut buf = InputBuffer::new();
        buf.history_up(); // no-op
        buf.history_down(); // no-op
        assert!(buf.content.is_empty());
    }

    #[test]
    fn history_capped_at_100() {
        let mut buf = InputBuffer::new();
        for i in 0..101 {
            buf.content = format!("cmd_{}", i);
            buf.submit();
        }
        assert_eq!(buf.history.len(), 100);
        assert_eq!(buf.history[0], "cmd_1"); // oldest evicted
        assert_eq!(buf.history[99], "cmd_100");
    }

    #[test]
    fn submit_empty_does_not_add_to_history() {
        let mut buf = InputBuffer::new();
        buf.submit();
        assert!(buf.history.is_empty());
    }

    // -----------------------------------------------------------------------
    // Multi-line
    // -----------------------------------------------------------------------
    #[test]
    fn toggle_multi_line() {
        let mut buf = InputBuffer::new();
        assert!(!buf.multi_line);
        buf.toggle_multi_line();
        assert!(buf.multi_line);
        buf.toggle_multi_line();
        assert!(!buf.multi_line);
    }

    #[test]
    fn history_up_enables_multi_line_for_newline_content() {
        let mut buf = InputBuffer::new();
        buf.content = "line1\nline2".to_string();
        buf.submit();
        assert!(!buf.multi_line);
        buf.history_up();
        assert!(buf.multi_line);
    }

    // -----------------------------------------------------------------------
    // Cursor row calculation
    // -----------------------------------------------------------------------
    #[test]
    fn cursor_row_single_line() {
        let mut buf = InputBuffer::new();
        buf.insert_str("hello");
        assert_eq!(buf.cursor_row(), 0);
    }

    #[test]
    fn cursor_row_multi_line() {
        let mut buf = InputBuffer::new();
        buf.content = "line1\nline2\nline3".to_string();
        buf.cursor = buf.content.len(); // at end
        assert_eq!(buf.cursor_row(), 2);
        buf.cursor = 0; // at start
        assert_eq!(buf.cursor_row(), 0);
    }

    #[test]
    fn cursor_row_empty() {
        let buf = InputBuffer::new();
        assert_eq!(buf.cursor_row(), 0);
    }
}
