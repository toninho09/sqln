//! The command palette: the actions of the application, with a filter.

use crossterm::event::KeyEvent;

use crate::filter_list::{FilterList, ListRequest};
use crate::input::Action;
use crate::line_input::LineInput;

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
    list: FilterList<Command>,
}

/// A word of the filter can be in the label or in the key.
fn filter_text(command: &Command) -> String {
    format!("{} {}", command.label, command.key.as_deref().unwrap_or(""))
}

impl Palette {
    pub fn new(commands: Vec<Command>) -> Self {
        Self {
            list: FilterList::new(commands, filter_text),
        }
    }

    pub fn list(&self) -> &FilterList<Command> {
        &self.list
    }

    pub fn input(&self) -> &LineInput {
        self.list.input()
    }

    pub fn cursor(&self) -> usize {
        self.list.cursor()
    }

    /// Lists the commands that the filter keeps.
    pub fn shown(&self) -> impl ExactSizeIterator<Item = &Command> {
        self.list.shown()
    }

    pub fn paste(&mut self, text: &str) {
        self.list.paste(text);
    }

    pub fn move_by(&mut self, rows: isize) {
        self.list.move_by(rows);
    }

    /// Returns the request for a row of the list.
    pub fn choose(&mut self, index: usize) -> PaletteRequest {
        match self.list.row(index) {
            Some(command) => PaletteRequest::Run(command.action),
            None => PaletteRequest::None,
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> PaletteRequest {
        match self.list.on_key(key) {
            ListRequest::None => PaletteRequest::None,
            ListRequest::Cancel => PaletteRequest::Cancel,
            ListRequest::Choose(index) => self.choose(index),
        }
    }
}
