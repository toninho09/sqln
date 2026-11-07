//! The request for a new name.

use crossterm::event::KeyEvent;

use super::text;
use super::theme::Theme;
use super::widgets::{TextAction, TextPrompt, draw_text_prompt};
use crate::exec::ConsoleId;
use crate::grid::printable;
use crate::store::scripts::ScriptEntry;

/// What the new name applies to.
pub enum NameTarget {
    Console(ConsoleId),
    /// A saved console that no tab shows.
    Script(ScriptEntry),
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
        text::TITLE_RENAME,
        text::FIELD_NAME,
        &note,
        text::HINT_NAME,
        theme,
    );
}
