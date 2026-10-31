//! The command palette: the actions of the application, with a filter.

use crossterm::event::{KeyCode, KeyEvent};

use crate::input::Action;
use crate::line_input::{Edit, LineInput};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub action: Action,
    pub label: &'static str,
    /// The key of the action. An action without a key stays in the list.
    pub key: Option<String>,
}

/// What a key asks from the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteRequest {
    None,
    Cancel,
    Run(Action),
}

#[derive(Clone, Debug)]
pub struct Palette {
    commands: Vec<Command>,
    input: LineInput,
    /// The positions in `commands` that the filter keeps.
    shown: Vec<usize>,
    cursor: usize,
}

/// Returns true when each word of the filter is in the label or in the key.
fn matches(command: &Command, words: &[String]) -> bool {
    let label = command.label.to_lowercase();
    let key = command.key.as_deref().unwrap_or("").to_lowercase();
    words
        .iter()
        .all(|word| label.contains(word) || key.contains(word))
}

impl Palette {
    pub fn new(commands: Vec<Command>) -> Self {
        let shown = (0..commands.len()).collect();
        Self {
            commands,
            input: LineInput::default(),
            shown,
            cursor: 0,
        }
    }

    pub fn input(&self) -> &LineInput {
        &self.input
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Lists the commands that the filter keeps.
    pub fn shown(&self) -> impl ExactSizeIterator<Item = &Command> {
        self.shown.iter().map(|index| &self.commands[*index])
    }

    fn refilter(&mut self) {
        let words: Vec<String> = self
            .input
            .text()
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        self.shown = (0..self.commands.len())
            .filter(|index| matches(&self.commands[*index], &words))
            .collect();
        self.cursor = 0;
    }

    pub fn paste(&mut self, text: &str) {
        self.input.insert(text.lines().next().unwrap_or(""));
        self.refilter();
    }

    pub fn move_by(&mut self, rows: isize) {
        let last = self.shown.len().saturating_sub(1);
        self.cursor = self.cursor.saturating_add_signed(rows).min(last);
    }

    /// Returns the request for a row of the list.
    pub fn choose(&mut self, index: usize) -> PaletteRequest {
        match self.shown.get(index) {
            Some(command) => PaletteRequest::Run(self.commands[*command].action),
            None => PaletteRequest::None,
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> PaletteRequest {
        match key.code {
            KeyCode::Esc => PaletteRequest::Cancel,
            KeyCode::Enter => self.choose(self.cursor),
            KeyCode::Up => {
                self.move_by(-1);
                PaletteRequest::None
            }
            KeyCode::Down => {
                self.move_by(1);
                PaletteRequest::None
            }
            _ => {
                if !matches!(self.input.on_key(key), Edit::None | Edit::Moved) {
                    self.refilter();
                }
                PaletteRequest::None
            }
        }
    }
}
