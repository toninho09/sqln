//! The request input of an AI completion.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::statusline::{HintItem, hint_line};
use super::theme::Theme;
use super::widgets::{centered, open_modal, short_modal_width};
use super::{input_spans, text};
use crate::line_input::LineInput;

/// The most characters a request holds.
const LIMIT: usize = 1024;

pub enum RequestAction {
    None,
    Cancel,
    Accept(String),
}

#[derive(Default)]
pub struct RequestPrompt {
    input: LineInput,
}

impl RequestPrompt {
    pub fn new() -> Self {
        Self {
            input: LineInput::default(),
        }
    }

    pub fn paste(&mut self, value: &str) {
        self.input.insert(value.lines().next().unwrap_or(""));
        self.truncate();
    }

    pub fn on_key(&mut self, key: KeyEvent) -> RequestAction {
        match key.code {
            KeyCode::Esc => RequestAction::Cancel,
            KeyCode::Enter => RequestAction::Accept(self.input.text().trim().to_owned()),
            _ => {
                self.input.on_key(key);
                self.truncate();
                RequestAction::None
            }
        }
    }

    /// Keeps the text within the limit, with the cursor at its end.
    fn truncate(&mut self) {
        let text = self.input.text();
        if text.chars().count() <= LIMIT {
            return;
        }
        let kept: String = text.chars().take(LIMIT).collect();
        self.input.set(&kept);
    }
}

pub fn draw_request(frame: &mut Frame, prompt: &RequestPrompt, theme: &Theme) {
    let area = centered(frame.area(), short_modal_width(frame.area().width, 60), 6);
    let inner = open_modal(frame, area, text::TITLE_AI_REQUEST, theme);
    let chars: Vec<char> = prompt.input.text().chars().collect();
    let width = usize::from(inner.width).saturating_sub(11);
    let mut spans = vec![Span::raw(format!("{}: ", text::FIELD_AI_REQUEST))];
    spans.extend(input_spans(&chars, prompt.input.cursor(), width, true, theme).0);
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    frame.render_widget(
        Paragraph::new(text::NOTE_AI_REQUEST).style(theme.status),
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );
    frame.render_widget(
        Paragraph::new(hint_line(&HintItem::of(text::HINT_AI_REQUEST), theme)),
        Rect::new(inner.x, inner.y + 3, inner.width, 1),
    );
}
