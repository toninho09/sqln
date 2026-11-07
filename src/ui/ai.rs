//! The request input of an AI completion.

use crossterm::event::KeyEvent;

use super::text;
use super::theme::Theme;
use super::widgets::{TextAction, TextPrompt, draw_text_prompt};
use ratatui::Frame;

/// The most characters a request holds.
const LIMIT: usize = 1024;

#[derive(Default)]
pub struct RequestPrompt {
    input: TextPrompt,
}

impl RequestPrompt {
    pub fn new() -> Self {
        Self {
            input: TextPrompt::new(LIMIT),
        }
    }

    pub fn paste(&mut self, value: &str) {
        self.input.paste(value);
    }

    pub fn on_key(&mut self, key: KeyEvent) -> TextAction {
        self.input.on_key(key)
    }
}

pub fn draw_request(frame: &mut Frame, prompt: &RequestPrompt, theme: &Theme) {
    draw_text_prompt(
        frame,
        &prompt.input,
        text::TITLE_AI_REQUEST,
        text::FIELD_AI_REQUEST,
        text::NOTE_AI_REQUEST,
        text::HINT_AI_REQUEST,
        theme,
    );
}
