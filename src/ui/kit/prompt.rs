//! The one-line text prompt.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::super::text;
use super::super::theme::Theme;
use super::feedback::{HintItem, hint_line};
use super::geometry::{centered, short_modal_width};
use super::input::{self, FieldMode, FieldText};
use super::modal::open_modal;
use crate::grid::printable;
use crate::line_input::LineInput;

/// A one-line text prompt. Esc cancels, Enter accepts the trimmed text.
pub struct TextPrompt {
    input: LineInput,
    /// The most characters the text holds.
    limit: usize,
}

/// The answer of a text prompt.
pub enum TextAction {
    None,
    Cancel,
    Accept(String),
}

impl Default for TextPrompt {
    fn default() -> Self {
        Self::new(usize::MAX)
    }
}

impl TextPrompt {
    pub fn new(limit: usize) -> Self {
        Self {
            input: LineInput::default(),
            limit,
        }
    }

    /// Replaces the text, with the cursor at its end.
    pub fn set(&mut self, text: &str) {
        self.input.set(text);
    }

    pub fn text(&self) -> &str {
        self.input.text()
    }

    fn cursor(&self) -> usize {
        self.input.cursor()
    }

    pub fn paste(&mut self, value: &str) {
        self.input.insert(value.lines().next().unwrap_or(""));
        self.truncate();
    }

    pub fn on_key(&mut self, key: KeyEvent) -> TextAction {
        match key.code {
            KeyCode::Esc => TextAction::Cancel,
            KeyCode::Enter => TextAction::Accept(self.input.text().trim().to_owned()),
            _ => {
                self.input.on_key(key);
                self.truncate();
                TextAction::None
            }
        }
    }

    /// Keeps the text within the limit, with the cursor at its end.
    fn truncate(&mut self) {
        let text = self.input.text();
        if text.chars().count() <= self.limit {
            return;
        }
        let kept: String = text.chars().take(self.limit).collect();
        self.input.set(&kept);
    }
}

/// Draws a one-line prompt window: the label with the input, a note below it, and a
/// hint on the last row.
pub fn draw_text_prompt(
    frame: &mut Frame,
    prompt: &TextPrompt,
    title: &str,
    label: &str,
    note: &str,
    hint: &[text::HintPart],
    theme: &Theme,
) {
    let area = centered(frame.area(), short_modal_width(frame.area().width, 60), 6);
    let inner = open_modal(frame, area, title, theme);
    let columns = usize::from(inner.width)
        .saturating_sub(UnicodeWidthStr::width(label) + 4)
        .max(1);
    let mut spans = vec![Span::raw(format!("{label}: "))];
    spans.extend(
        input::view(
            FieldText {
                text: prompt.text(),
                cursor: prompt.cursor(),
                width: columns,
                focused: true,
                mode: FieldMode::Text,
            },
            theme,
        )
        .spans,
    );
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    frame.render_widget(
        Paragraph::new(printable(note)).style(theme.status),
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );
    frame.render_widget(
        Paragraph::new(hint_line(&HintItem::of(hint), theme)),
        Rect::new(inner.x, inner.y + 3, inner.width, 1),
    );
}
