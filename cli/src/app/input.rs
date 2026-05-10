//
// `cursor` is a byte offset into `text` (not a char count). All ops
// here keep it on a UTF-8 char boundary so multibyte input — emoji,
// CJK, accented letters — round-trips without panicking when the
// caller later slices on the cursor position.
//

pub(crate) fn insert_char(text: &mut String, cursor: &mut usize, ch: char) {
    text.insert(*cursor, ch);
    *cursor += ch.len_utf8();
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
