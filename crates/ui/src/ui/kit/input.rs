//! One line of a form value: the text, the cursor, a mask, or the end of a path.

use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::super::input_spans;
use super::super::theme::Theme;
use super::measure::last_chars;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldMode {
    Text,
    /// Hides the text with a mark. `limit` caps the count of marks.
    Masked {
        mask: char,
        limit: Option<usize>,
    },
    /// Shows the end of a long path when the field has no focus.
    Path,
}

#[derive(Clone, Copy, Debug)]
pub struct FieldText<'a> {
    /// The text to show. It must already be printable when the screen needs that.
    pub text: &'a str,
    /// The cursor position in characters.
    pub cursor: usize,
    pub width: usize,
    pub focused: bool,
    pub mode: FieldMode,
}

pub struct View {
    pub spans: Vec<Span<'static>>,
    /// The index of the first visible character.
    pub first: usize,
    /// The columns of the text, for a value between brackets.
    pub used: usize,
}

/// Writes the visible part of a field and keeps the cursor inside `width`.
pub fn view(field: FieldText<'_>, theme: &Theme) -> View {
    if field.mode == FieldMode::Path && !field.focused {
        let chars: Vec<char> = field.text.chars().collect();
        let shown = last_chars(&chars, field.width);
        return View {
            used: shown.width(),
            spans: vec![Span::raw(shown)],
            first: 0,
        };
    }
    let chars: Vec<char> = match field.mode {
        FieldMode::Masked { mask, limit } => {
            let count = field.text.chars().count();
            let count = limit.map_or(count, |limit| count.min(limit));
            std::iter::repeat_n(mask, count).collect()
        }
        FieldMode::Text | FieldMode::Path => field.text.chars().collect(),
    };
    let (spans, first) = input_spans(&chars, field.cursor, field.width, field.focused, theme);
    let used = spans.iter().map(Span::width).sum();
    View { spans, first, used }
}

pub fn line(field: FieldText<'_>, theme: &Theme) -> Line<'static> {
    Line::from(view(field, theme).spans)
}
