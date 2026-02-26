use rustyline::completion::Completer;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::Helper;
use rustyline::history::DefaultHistory;
use rustyline::{Config, Editor};
use std::borrow::Cow;

//
// Minimal rustyline helper that separates the plain-text prompt (used for
// width calculation) from the ANSI-colored prompt (used for display).
// Without this, rustyline on Windows counts ANSI escape bytes as visible
// characters, causing cursor misalignment.
//

pub struct ColoredPromptHelper {
    colored: String,
}

impl ColoredPromptHelper {
    pub fn new(colored: String) -> Self {
        Self { colored }
    }
}

impl Completer for ColoredPromptHelper {
    type Candidate = String;
}

impl Hinter for ColoredPromptHelper {
    type Hint = String;
}

impl Highlighter for ColoredPromptHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        _prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        Cow::Borrowed(&self.colored)
    }
}

impl Validator for ColoredPromptHelper {}
impl Helper for ColoredPromptHelper {}

pub fn editor_with_colored_prompt(
    plain: &str,
    colored: String,
) -> rustyline::Result<(Editor<ColoredPromptHelper, DefaultHistory>, String)> {
    let config = Config::builder().build();
    let mut rl = Editor::with_config(config)?;
    rl.set_helper(Some(ColoredPromptHelper::new(colored)));
    Ok((rl, plain.to_string()))
}
