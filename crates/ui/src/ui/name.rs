//! The request for a new name.

use crossterm::event::KeyEvent;

use super::kit::prompt::{TextAction, TextPrompt, draw_text_prompt};
use super::text;
use super::theme::Theme;
use crate::exec::{ConnectionId, ConsoleId};
use crate::grid::printable;
use crate::store::config::FolderRef;
use crate::store::scripts::ScriptEntry;

/// What the new name applies to.
pub enum NameTarget {
    Console(ConsoleId),
    /// A saved console that no tab shows.
    Script(ScriptEntry),
    /// The name of a new folder for a connection.
    NewFolder(ConnectionId),
    /// A folder of the explorer.
    Folder(FolderRef),
}

impl NameTarget {
    fn title(&self) -> &'static str {
        match self {
            Self::Console(_) | Self::Script(_) => text::TITLE_RENAME,
            Self::NewFolder(_) => text::TITLE_NEW_FOLDER,
            Self::Folder(_) => text::TITLE_RENAME_FOLDER,
        }
    }
}

pub struct NamePrompt {
    pub target: NameTarget,
    pub note: Option<String>,
    input: TextPrompt,
}

impl NamePrompt {
    pub fn new(target: NameTarget, name: &str) -> Self {
        let mut input = TextPrompt::default();
        input.set(name);
        Self {
            target,
            note: None,
            input,
        }
    }

    pub fn paste(&mut self, value: &str) {
        self.input.paste(value);
    }

    pub fn on_key(&mut self, key: KeyEvent) -> TextAction {
        self.input.on_key(key)
    }
}

pub fn draw_name(frame: &mut ratatui::Frame, prompt: &NamePrompt, theme: &Theme) {
    let note = prompt.note.as_deref().map_or_else(String::new, printable);
    draw_text_prompt(
        frame,
        &prompt.input,
        prompt.target.title(),
        text::FIELD_NAME,
        &note,
        text::HINT_NAME,
        theme,
    );
}
