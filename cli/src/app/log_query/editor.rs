//
// Multi-line editable buffer for the log-query editor pane. Stored as
// `Vec<String>` — one entry per visible line — with a `(row, col)` cursor
// where `col` is measured in characters, not bytes. Each helper keeps the
// cursor clamped to a valid position; callers don't need to re-clamp.
//

use std::cmp::min;

#[derive(Clone, Debug)]
pub struct EditorBuffer {
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
}

impl Default for EditorBuffer {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            cursor_row: 0,
            cursor_col: 0,
        }
    }
}

impl EditorBuffer {
    pub fn from_text(text: &str) -> Self {
        let lines: Vec<String> = if text.is_empty() {
            vec![String::new()]
        } else {
            text.split('\n').map(|s| s.to_string()).collect()
        };
        let cursor_row = lines.len() - 1;
        let cursor_col = lines[cursor_row].chars().count();
        Self {
            lines,
            cursor_row,
            cursor_col,
        }
    }

    pub fn as_text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn current_line_len(&self) -> usize {
        self.lines[self.cursor_row].chars().count()
    }

    pub fn insert_char(&mut self, ch: char) {
        let line = &mut self.lines[self.cursor_row];
        let byte_idx = char_idx_to_byte(line, self.cursor_col);
        line.insert(byte_idx, ch);
        self.cursor_col += 1;
    }

    pub fn insert_newline(&mut self) {
        let line = &mut self.lines[self.cursor_row];
        let byte_idx = char_idx_to_byte(line, self.cursor_col);
        let tail = line.split_off(byte_idx);
        self.cursor_row += 1;
        self.lines.insert(self.cursor_row, tail);
        self.cursor_col = 0;
    }

    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let line = &mut self.lines[self.cursor_row];
            let prev_byte = char_idx_to_byte(line, self.cursor_col - 1);
            let curr_byte = char_idx_to_byte(line, self.cursor_col);
            line.replace_range(prev_byte..curr_byte, "");
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            let line = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            let prev_len = self.lines[self.cursor_row].chars().count();
            self.lines[self.cursor_row].push_str(&line);
            self.cursor_col = prev_len;
        }
    }

    pub fn delete(&mut self) {
        let line_len = self.current_line_len();
        if self.cursor_col < line_len {
            let line = &mut self.lines[self.cursor_row];
            let curr_byte = char_idx_to_byte(line, self.cursor_col);
            let next_byte = char_idx_to_byte(line, self.cursor_col + 1);
            line.replace_range(curr_byte..next_byte, "");
        } else if self.cursor_row + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(&next);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.current_line_len();
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor_col < self.current_line_len() {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = min(self.cursor_col, self.current_line_len());
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = min(self.cursor_col, self.current_line_len());
        }
    }

    pub fn move_home(&mut self) {
        self.cursor_col = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor_col = self.current_line_len();
    }

    //
    // Text before the cursor on the current line — used by the autocomplete
    // engine to detect the current token and its context.
    //
    pub fn current_prefix(&self) -> String {
        let line = &self.lines[self.cursor_row];
        let byte_idx = char_idx_to_byte(line, self.cursor_col);
        line[..byte_idx].to_string()
    }

    //
    // Full text before the cursor (all previous lines plus the current
    // prefix). This is what context-aware autocomplete needs to work out
    // which pipeline stage we're in.
    //
    pub fn full_prefix(&self) -> String {
        let mut out = String::new();
        for line in &self.lines[..self.cursor_row] {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(&self.current_prefix());
        out
    }

    //
    // Replace the token immediately before the cursor with `replacement`.
    // "Token" here is the run of word-characters / dashes behind the
    // cursor; used by autocomplete when the user accepts a suggestion.
    //
    pub fn replace_current_token(&mut self, replacement: &str) {
        let line = &self.lines[self.cursor_row];
        let byte_idx = char_idx_to_byte(line, self.cursor_col);
        let head = &line[..byte_idx];
        let token_start = head
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || *c == '-')
            .last()
            .map(|(i, _)| i)
            .unwrap_or(byte_idx);

        let tail = self.lines[self.cursor_row][byte_idx..].to_string();
        let head_str = self.lines[self.cursor_row][..token_start].to_string();
        self.lines[self.cursor_row] = format!("{}{}{}", head_str, replacement, tail);
        self.cursor_col = head_str.chars().count() + replacement.chars().count();
    }
}

fn char_idx_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}
