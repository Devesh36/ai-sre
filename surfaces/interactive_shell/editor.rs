use crate::surfaces::shared::terminal::{clean, interactive};
use anyhow::Result;
use rustyline::{
    Context, Editor, Helper,
    completion::{Completer, Pair},
    error::ReadlineError,
    highlight::Highlighter,
    hint::{Hinter, HistoryHinter},
    history::DefaultHistory,
    validate::Validator,
};
use std::{
    borrow::Cow,
    io::{self, BufRead, Write},
};

pub const COMMANDS: &[(&str, &str)] = &[
    ("/help", "Show commands and keyboard shortcuts"),
    ("/service ", "Switch service and start a new conversation"),
    ("/new", "Start a fresh conversation for this service"),
    ("/sessions", "List saved investigations"),
    ("/resume ", "Continue a saved investigation by ID"),
    ("/status", "Show the current investigation"),
    ("/cost", "Show token usage and estimated cost"),
    ("/evidence", "Show retained evidence and source references"),
    ("/trace", "Show execution steps and timing"),
    ("/compact", "Compact context while keeping evidence"),
    ("/integrations", "List configured investigation tools"),
    ("/history", "Show recent questions for this service"),
    (
        "/clear-history",
        "Clear this service's saved question history",
    ),
    ("/exit", "Leave the terminal workspace"),
];
pub fn completions(line: &str) -> Vec<Pair> {
    if !line.starts_with('/') || line.contains(char::is_whitespace) {
        return vec![];
    }
    COMMANDS
        .iter()
        .filter(|(name, _)| name.starts_with(line))
        .map(|(name, description)| Pair {
            display: format!("{name:<18} {description}"),
            replacement: (*name).into(),
        })
        .collect()
}
struct ShellHelper {
    color: bool,
    hinter: HistoryHinter,
}
impl Completer for ShellHelper {
    type Candidate = Pair;
    fn complete(
        &self,
        line: &str,
        pos: usize,
        _context: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        Ok((
            0,
            if pos == line.len() {
                completions(line)
            } else {
                vec![]
            },
        ))
    }
}
impl Hinter for ShellHelper {
    type Hint = String;
    fn hint(&self, line: &str, pos: usize, context: &Context<'_>) -> Option<String> {
        self.hinter.hint(line, pos, context).map(|s| clean(&s))
    }
}
impl Highlighter for ShellHelper {
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        if self.color {
            Cow::Owned(format!("\x1b[2m{}\x1b[0m", clean(hint)))
        } else {
            Cow::Borrowed(hint)
        }
    }
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        if self.color {
            Cow::Owned(format!("\x1b[1;36m{prompt}\x1b[0m"))
        } else {
            Cow::Borrowed(prompt)
        }
    }
}
impl Validator for ShellHelper {}
impl Helper for ShellHelper {}
type LineEditor = Editor<ShellHelper, DefaultHistory>;
pub enum Input {
    Line(String),
    Interrupted,
    Eof,
}
pub struct Prompt {
    editor: Option<LineEditor>,
}
impl Prompt {
    pub fn new(color: bool) -> Result<Self> {
        let editor = if interactive() {
            let config = rustyline::Config::builder()
                .max_history_size(200)?
                .auto_add_history(false)
                .completion_type(rustyline::CompletionType::List)
                .build();
            let mut editor = Editor::with_config(config)?;
            editor.set_helper(Some(ShellHelper {
                color,
                hinter: HistoryHinter::new(),
            }));
            Some(editor)
        } else {
            None
        };
        Ok(Self { editor })
    }
    pub fn replace_history(&mut self, lines: impl Iterator<Item = String>) -> Result<()> {
        if let Some(editor) = &mut self.editor {
            editor.clear_history()?;
            for line in lines {
                editor.add_history_entry(clean(&line))?;
            }
        }
        Ok(())
    }
    pub fn remember(&mut self, redacted: &str) -> Result<()> {
        if let Some(editor) = &mut self.editor {
            editor.add_history_entry(clean(redacted))?;
        }
        Ok(())
    }
    pub async fn read(&mut self, prompt: String) -> Result<Input> {
        if let Some(mut editor) = self.editor.take() {
            let (editor, result) = tokio::task::spawn_blocking(move || {
                let result = editor.readline(&prompt);
                (editor, result)
            })
            .await?;
            self.editor = Some(editor);
            Ok(match result {
                Ok(line) => Input::Line(line),
                Err(ReadlineError::Interrupted) => Input::Interrupted,
                Err(ReadlineError::Eof) => Input::Eof,
                Err(error) => return Err(error.into()),
            })
        } else {
            print!("{prompt}");
            io::stdout().flush()?;
            let mut line = String::new();
            if io::stdin().lock().read_line(&mut line)? == 0 {
                Ok(Input::Eof)
            } else {
                Ok(Input::Line(line))
            }
        }
    }
}
