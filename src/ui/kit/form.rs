//! The rows of a form: the focus mark, the label, and the value.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::super::text;
use super::super::theme::Theme;
use super::geometry::{LEFT, RIGHT};
use super::style;

/// The two widths of the label column of a form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelWidth {
    Narrow,
    Wide,
}

impl LabelWidth {
    pub const fn columns(self) -> u16 {
        match self {
            Self::Narrow => 16,
            Self::Wide => 24,
        }
    }
}

/// Returns the style of the value of a focused field.
pub fn focus_style(focused: bool, theme: &Theme) -> Style {
    if focused {
        theme.dialog_focus
    } else {
        Style::new()
    }
}

/// Writes a choice between angles, as the value of a form row.
pub fn choice_value(name: &str) -> String {
    format!("< {name} >")
}

/// The mark of a flag of a form.
pub fn flag_mark(on: bool) -> &'static str {
    if on { "[x]" } else { "[ ]" }
}

/// Returns the line of a choice value, in the focus style.
pub fn choice_line(name: &str, focused: bool, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        choice_value(name),
        focus_style(focused, theme),
    ))
}

/// Returns the line of a flag, in the focus style.
pub fn flag_line(on: bool, focused: bool, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(flag_mark(on), focus_style(focused, theme)))
}

/// Returns the columns that the button of a path field takes from its value, with a gap.
pub fn browse_width(theme: &Theme) -> u16 {
    u16::try_from(theme.button(text::BUTTON_BROWSE).width() + 1).unwrap_or(0)
}

/// Draws the button of a path field at the end of its row, and returns its area.
pub fn draw_browse_button(frame: &mut Frame, row: Rect, focused: bool, theme: &Theme) -> Rect {
    let width = browse_width(theme).saturating_sub(1);
    let area = Rect {
        x: row.right().saturating_sub(RIGHT + 1 + width),
        width,
        ..row
    };
    let style = if focused {
        theme.dialog_focus
    } else {
        theme.dialog_option
    };
    let name = Span::styled(theme.button(text::BUTTON_BROWSE), style);
    frame.render_widget(Paragraph::new(name), area);
    area
}

/// Returns the columns of the value of a row.
pub fn value_width(row: Rect, label_width: u16) -> usize {
    usize::from(row.width.saturating_sub(LEFT + label_width + RIGHT + 2))
}

/// Draws one row: the focus mark, the label, and the value.
pub fn draw_row(
    frame: &mut Frame,
    area: Rect,
    (name, label_width): (&str, u16),
    focused: bool,
    value: Line,
    theme: &Theme,
) {
    if focused {
        let mark = Span::styled(theme.icons.focus_mark, theme.panel_title_focused);
        frame.render_widget(
            Paragraph::new(mark),
            Rect {
                x: area.x + 1,
                width: 1,
                ..area
            },
        );
    }
    let label_style = style::label_style(focused, theme);
    let label_area = Rect {
        x: area.x + LEFT,
        width: label_width,
        ..area
    };
    frame.render_widget(Paragraph::new(name).style(label_style), label_area);

    let start = area.x + LEFT + label_width;
    let value_area = Rect {
        x: start,
        width: area.right().saturating_sub(start + RIGHT),
        ..area
    };
    // The background comes first, because the text and the cursor have their own styles.
    frame
        .buffer_mut()
        .set_style(value_area, style::field_style(focused, theme));
    let text_area = Rect {
        x: value_area.x + 1,
        width: value_area.width.saturating_sub(2),
        ..value_area
    };
    frame.render_widget(Paragraph::new(value), text_area);
}
