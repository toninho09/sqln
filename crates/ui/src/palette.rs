//! The command palette: the actions of the application, the settings, and the help
//! pages, with a filter.

use crossterm::event::KeyEvent;

use crate::exec::ConnectionId;
use crate::filter_list::{FilterList, ListRequest};
use crate::form;
use crate::input::Action;
use crate::line_input::LineInput;
use crate::settings;
use crate::ui::text;

/// What a row of the palette opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Action(Action),
    /// A page of the help, as an index of its sidebar.
    Help(usize),
    /// A field of the connection form.
    ConnectionField(ConnectionId, form::Field),
    /// A field of the settings dialog.
    Setting(settings::Field),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub target: Target,
    pub label: String,
    /// The key of the action. An action without a key stays in the list.
    pub key: Option<String>,
    /// More words for the filter, which the list does not show.
    pub words: String,
}

/// What a key asks from the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteRequest {
    None,
    Cancel,
    Run(Action),
    /// Opens the help on a page.
    Help(usize),
    /// Opens the connection form with the focus on the field.
    ConnectionField(ConnectionId, form::Field),
    /// Opens the settings dialog with the focus on the field.
    Setting(settings::Field),
}

#[derive(Clone, Debug)]
pub struct Palette {
    list: FilterList<Command>,
}

/// A word of the filter can be in the label, in the key, or in the other words.
fn filter_text(command: &Command) -> String {
    format!(
        "{} {} {}",
        command.label,
        command.key.as_deref().unwrap_or(""),
        command.words
    )
}

impl Command {
    /// Returns the row of an action. The filter also finds its name and aliases.
    pub fn action(action: Action, label: String, key: Option<String>) -> Self {
        Self {
            target: Target::Action(action),
            label,
            key,
            words: format!("{} {}", action.name(), text::command_aliases(action)),
        }
    }
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
            Some(command) => match command.target {
                Target::Action(action) => PaletteRequest::Run(action),
                Target::Help(page) => PaletteRequest::Help(page),
                Target::ConnectionField(id, field) => PaletteRequest::ConnectionField(id, field),
                Target::Setting(field) => PaletteRequest::Setting(field),
            },
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
