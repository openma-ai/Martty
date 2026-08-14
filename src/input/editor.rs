//! The composer line editor: buffer + cursor (char-indexed), prompt
//! history, and the word/kill motions the keymap dispatches. No app or
//! terminal knowledge — pure string surgery, unit-tested.

/// Editable prompt state. `cursor` is a char index (not bytes).
pub struct Input {
    pub buf: String,
    pub cursor: usize, // char index
    pub history: Vec<String>,
    pub hist_pos: Option<usize>,
    pub stash: String,
}

impl Input {
    pub fn new() -> Self {
        Input {
            buf: String::new(),
            cursor: 0,
            history: Vec::new(),
            hist_pos: None,
            stash: String::new(),
        }
    }

    fn byte_at(&self, char_idx: usize) -> usize {
        self.buf
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.buf.len())
    }

    /// Char count of the buffer (cursor upper bound).
    pub fn len_chars(&self) -> usize {
        self.buf.chars().count()
    }

    pub fn insert(&mut self, ch: char) {
        let at = self.byte_at(self.cursor);
        self.buf.insert(at, ch);
        self.cursor += 1;
        self.hist_pos = None;
    }

    pub fn insert_str(&mut self, s: &str) {
        let at = self.byte_at(self.cursor);
        self.buf.insert_str(at, s);
        self.cursor += s.chars().count();
        self.hist_pos = None;
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = self.byte_at(self.cursor - 1);
        let end = self.byte_at(self.cursor);
        self.buf.replace_range(start..end, "");
        self.cursor -= 1;
    }

    pub fn delete_word_back(&mut self) {
        let i = self.prev_word();
        let start = self.byte_at(i);
        let end = self.byte_at(self.cursor);
        self.buf.replace_range(start..end, "");
        self.cursor = i;
    }

    pub fn kill_to_end(&mut self) {
        let at = self.byte_at(self.cursor);
        self.buf.truncate(at);
    }

    pub fn kill_to_start(&mut self) {
        let at = self.byte_at(self.cursor);
        self.buf.replace_range(..at, "");
        self.cursor = 0;
    }

    pub fn delete_forward(&mut self) {
        if self.cursor >= self.len_chars() {
            return;
        }
        let start = self.byte_at(self.cursor);
        let end = self.byte_at(self.cursor + 1);
        self.buf.replace_range(start..end, "");
    }

    /// Cursor position one word left (whitespace-delimited).
    pub fn prev_word(&self) -> usize {
        let chars: Vec<char> = self.buf.chars().collect();
        let mut i = self.cursor.min(chars.len());
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        i
    }

    /// Cursor position one word right.
    pub fn next_word(&self) -> usize {
        let chars: Vec<char> = self.buf.chars().collect();
        let n = chars.len();
        let mut i = self.cursor.min(n);
        while i < n && chars[i].is_whitespace() {
            i += 1;
        }
        while i < n && !chars[i].is_whitespace() {
            i += 1;
        }
        i
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.cursor = 0;
        self.hist_pos = None;
    }

    pub fn set(&mut self, s: String) {
        self.cursor = s.chars().count();
        self.buf = s;
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(text: &str) -> Input {
        let mut i = Input::new();
        i.set(text.into());
        i
    }

    #[test]
    fn word_motions_hop_whitespace_delimited_words() {
        let mut i = editor("hello brave world");
        assert_eq!(i.next_word(), 17, "already at end");
        i.cursor = 0;
        assert_eq!(i.next_word(), 5);
        i.cursor = 5;
        assert_eq!(i.next_word(), 11);
        i.cursor = 17;
        assert_eq!(i.prev_word(), 12);
        i.cursor = 12;
        assert_eq!(i.prev_word(), 6);
    }

    #[test]
    fn kill_commands_split_at_cursor() {
        let mut i = editor("hello world");
        i.cursor = 6;
        i.kill_to_start();
        assert_eq!(i.buf, "world");
        assert_eq!(i.cursor, 0);

        let mut i = editor("hello world");
        i.cursor = 5;
        i.kill_to_end();
        assert_eq!(i.buf, "hello");
    }

    #[test]
    fn delete_forward_and_multibyte_safety() {
        let mut i = editor("a中b");
        i.cursor = 1;
        i.delete_forward();
        assert_eq!(i.buf, "ab");
        assert_eq!(i.cursor, 1);
        i.delete_forward();
        assert_eq!(i.buf, "a");
        i.delete_forward();
        assert_eq!(i.buf, "a", "at end: no-op");
    }

    #[test]
    fn delete_word_back_eats_trailing_whitespace_then_word() {
        let mut i = editor("one two   ");
        i.delete_word_back();
        assert_eq!(i.buf, "one ");
        assert_eq!(i.cursor, 4);
    }
}
