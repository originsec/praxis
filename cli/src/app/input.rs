pub(crate) fn insert_char(text: &mut String, cursor: &mut usize, ch: char) {
    text.insert(*cursor, ch);
    *cursor += 1;
}

pub(crate) fn backspace(text: &mut String, cursor: &mut usize) -> bool {
    if *cursor == 0 {
        return false;
    }

    *cursor -= 1;
    text.remove(*cursor);
    true
}

pub(crate) fn delete(text: &mut String, cursor: &usize) -> bool {
    if *cursor >= text.len() {
        return false;
    }

    text.remove(*cursor);
    true
}

pub(crate) fn move_left(cursor: &mut usize) {
    if *cursor > 0 {
        *cursor -= 1;
    }
}

pub(crate) fn move_right(text: &str, cursor: &mut usize) {
    if *cursor < text.len() {
        *cursor += 1;
    }
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
