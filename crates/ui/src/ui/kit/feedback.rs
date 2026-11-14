//! The notes and the other feedback of the interface.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use super::super::theme::Theme;
use super::geometry::NOTE_ROWS;
use crate::grid::printable;

/// Draws a note that ends on the row above `row`. A long note goes up to the next row.
pub fn draw_note(frame: &mut Frame, content: Rect, row: u16, note: &str, style: Style) {
    let message = printable(note);
    let width = usize::from(content.width).max(1);
    let rows = u16::try_from(message.width().div_ceil(width))
        .unwrap_or(NOTE_ROWS)
        .clamp(1, NOTE_ROWS);
    let place = Rect {
        y: row.saturating_sub(rows),
        height: rows,
        ..content
    };
    let paragraph = Paragraph::new(Line::styled(message, style)).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, place);
}

/// One command of the hint bar: a key and its effect. A note has an empty key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HintItem {
    pub key: String,
    pub label: &'static str,
}

impl HintItem {
    pub fn of(parts: &[super::super::text::HintPart]) -> Vec<Self> {
        parts
            .iter()
            .map(|(key, label)| Self {
                key: (*key).to_owned(),
                label: *label,
            })
            .collect()
    }

    fn text(&self) -> String {
        if self.key.is_empty() {
            self.label.to_owned()
        } else {
            format!("{} {}", self.key, self.label)
        }
    }
}

const GAP: &str = "  ";

/// Returns the text of a row of commands.
pub fn hint_text(items: &[HintItem]) -> String {
    let parts: Vec<String> = items.iter().map(HintItem::text).collect();
    parts.join(GAP)
}

/// Keeps the commands that fit a row, in their order. A command that does not fit stays
/// out whole, so the row shows no cut key, and a later short command can still fit.
pub fn fit_hints(items: &[HintItem], width: usize) -> Vec<HintItem> {
    use unicode_width::UnicodeWidthStr;
    let mut used = 0;
    let mut kept = Vec::new();
    for item in items {
        let gap = if kept.is_empty() { 0 } else { GAP.len() };
        let cost = gap + item.text().width();
        if used + cost <= width {
            used += cost;
            kept.push(item.clone());
        }
    }
    kept
}

/// Returns a row of commands. The style of the keys and the gap separate each command.
pub fn hint_line(items: &[HintItem], theme: &Theme) -> ratatui::text::Line<'static> {
    use ratatui::text::Span;
    let mut spans = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw(GAP));
        }
        if !item.key.is_empty() {
            spans.push(Span::styled(item.key.clone(), theme.key));
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(item.label, theme.hint));
    }
    ratatui::text::Line::from(spans)
}

fn level_note(level: crate::console::MessageLevel) -> super::style::NoteLevel {
    use crate::console::MessageLevel;
    match level {
        MessageLevel::Info => super::style::NoteLevel::Info,
        MessageLevel::Warning => super::style::NoteLevel::Warning,
        MessageLevel::Error => super::style::NoteLevel::Error,
    }
}

/// The style of a message of the console.
pub fn message_style(message: &crate::console::Message, theme: &Theme) -> Style {
    super::style::note_style(level_note(message.level), theme)
}

/// The style of a note in the command log.
pub fn log_note_style(level: crate::console::MessageLevel, theme: &Theme) -> Style {
    super::style::note_style(level_note(level), theme)
}
