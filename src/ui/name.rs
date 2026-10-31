//! The request for a new name.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::console::centered;
use super::float_block;
use super::theme::Theme;
use super::{input_spans, text};
use crate::exec::ConsoleId;
use crate::grid::printable;
use crate::line_input::LineInput;
use crate::store::scripts::ScriptEntry;

/// What the new name applies to.
pub enum NameTarget {
    Console(ConsoleId),
    /// A saved console that no tab shows.
    Script(ScriptEntry),
}

pub enum NameAction {
    None,
    Cancel,
    Accept(String),
}

pub struct NamePrompt {
    pub target: NameTarget,
    pub note: Option<String>,
    input: LineInput,
}

impl NamePrompt {
    pub fn new(target: NameTarget, name: &str) -> Self {
        let mut input = LineInput::default();
        input.set(name);
        Self { target, note: None, input }
    }

    pub fn paste(&mut self, value: &str) {
        self.input.insert(value.lines().next().unwrap_or(""));
    }

    pub fn on_key(&mut self, key: KeyEvent) -> NameAction {
        match key.code {
            KeyCode::Esc => NameAction::Cancel,
            KeyCode::Enter => NameAction::Accept(self.input.text().trim().to_owned()),
            _ => {
                self.input.on_key(key);
                NameAction::None
            }
        }
    }
}

pub fn draw_name(frame: &mut Frame, prompt: &NamePrompt, theme: &Theme) {
    let area = centered(frame.area(), 60, 6);
    frame.render_widget(Clear, area);
    let block = float_block(text::TITLE_RENAME, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let chars: Vec<char> = prompt.input.text().chars().collect();
    let width = usize::from(inner.width).saturating_sub(8);
    let mut spans = vec![Span::raw(format!("{}: ", text::FIELD_NAME))];
    spans.extend(input_spans(&chars, prompt.input.cursor(), width, true, theme).0);
    frame.render_widget(Paragraph::new(Line::from(spans)), Rect::new(inner.x, inner.y, inner.width, 1));
    let note = prompt.note.as_deref().map_or_else(String::new, printable);
    frame.render_widget(
        Paragraph::new(note).style(theme.status),
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );
    frame.render_widget(
        Paragraph::new(text::HINT_NAME).style(theme.hint),
        Rect::new(inner.x, inner.y + 3, inner.width, 1),
    );
}
