//
// `cursor` is a byte offset into `text` (not a char count). All ops
// here keep it on a UTF-8 char boundary so multibyte input — emoji,
// CJK, accented letters — round-trips without panicking when the
// caller later slices on the cursor position.
//

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(crate) fn insert_char(text: &mut String, cursor: &mut usize, ch: char) {
    text.insert(*cursor, ch);
    *cursor += ch.len_utf8();
}

pub(crate) fn insert_newline(text: &mut String, cursor: &mut usize) {
    insert_char(text, cursor, '\n');
}

//
// True when the key should insert a newline in free-text prompts
// (orchestrator / session chat) rather than submit. Shift+Enter is the
// primary binding; Alt+Enter is a fallback for terminals that do not
// report modifiers on Enter without Kitty keyboard protocol.
//
pub(crate) fn wants_newline(key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Enter => {
            key.modifiers.contains(KeyModifiers::SHIFT)
                || key.modifiers.contains(KeyModifiers::ALT)
        }
        //
        // Some paste paths and a few terminals deliver a literal newline
        // as a character event rather than KeyCode::Enter.
        //
        KeyCode::Char('\n') => true,
        _ => false,
    }
}

//
// True when Enter should submit/send (no shift/alt).
//
pub(crate) fn wants_submit(key: KeyEvent) -> bool {
    key.code == KeyCode::Enter
        && !key.modifiers.contains(KeyModifiers::SHIFT)
        && !key.modifiers.contains(KeyModifiers::ALT)
}

//
// Move the cursor to the previous line, preserving column when possible.
// Returns false when already on the first line.
//
pub(crate) fn move_line_up(text: &str, cursor: &mut usize) -> bool {
    let pos = (*cursor).min(text.len());
    let line_start = text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    if line_start == 0 {
        return false;
    }
    let col = pos - line_start;
    let prev_end = line_start - 1; // the '\n'
    let prev_start = text[..prev_end].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let prev_len = prev_end - prev_start;
    *cursor = prev_start + col.min(prev_len);
    true
}

//
// Move the cursor to the next line, preserving column when possible.
// Returns false when already on the last line.
//
pub(crate) fn move_line_down(text: &str, cursor: &mut usize) -> bool {
    let pos = (*cursor).min(text.len());
    let line_start = text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = text[pos..]
        .find('\n')
        .map(|i| pos + i)
        .unwrap_or(text.len());
    if line_end >= text.len() {
        return false;
    }
    let col = pos - line_start;
    let next_start = line_end + 1;
    let next_end = text[next_start..]
        .find('\n')
        .map(|i| next_start + i)
        .unwrap_or(text.len());
    let next_len = next_end - next_start;
    *cursor = next_start + col.min(next_len);
    true
}

pub(crate) fn backspace(text: &mut String, cursor: &mut usize) -> bool {
    if *cursor == 0 {
        return false;
    }

    let removed = text.remove(prev_char_boundary(text, *cursor));
    *cursor -= removed.len_utf8();
    true
}

pub(crate) fn delete(text: &mut String, cursor: &usize) -> bool {
    if *cursor >= text.len() {
        return false;
    }

    text.remove(*cursor);
    true
}

pub(crate) fn move_left(text: &str, cursor: &mut usize) {
    if *cursor > 0 {
        *cursor = prev_char_boundary(text, *cursor);
    }
}

pub(crate) fn move_right(text: &str, cursor: &mut usize) {
    if *cursor < text.len() {
        *cursor += text[*cursor..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
    }
}

//
// Walk back from `pos` to the nearest preceding char boundary.
//

fn prev_char_boundary(text: &str, pos: usize) -> usize {
    let mut p = pos.saturating_sub(1);
    while p > 0 && !text.is_char_boundary(p) {
        p -= 1;
    }
    p
}

pub(crate) fn move_home(cursor: &mut usize) {
    *cursor = 0;
}

pub(crate) fn move_end(text: &str, cursor: &mut usize) {
    *cursor = text.len();
}

pub(crate) fn history_up(
    text: &mut String,
    cursor: &mut usize,
    history: &[String],
    history_index: &mut Option<usize>,
    saved_input: &mut String,
) {
    let hist_len = history.len();
    if hist_len == 0 {
        return;
    }

    match history_index {
        None => {
            *saved_input = text.clone();
            *history_index = Some(hist_len - 1);
        }
        Some(idx) if *idx > 0 => {
            *history_index = Some(*idx - 1);
        }
        _ => {}
    }

    if let Some(idx) = *history_index {
        *text = history[idx].clone();
        *cursor = text.len();
    }
}

pub(crate) fn history_down(
    text: &mut String,
    cursor: &mut usize,
    history: &[String],
    history_index: &mut Option<usize>,
    saved_input: &str,
) {
    let Some(idx) = *history_index else {
        return;
    };

    if idx + 1 < history.len() {
        *history_index = Some(idx + 1);
        *text = history[idx + 1].clone();
    } else {
        *history_index = None;
        *text = saved_input.to_string();
    }
    *cursor = text.len();
}
