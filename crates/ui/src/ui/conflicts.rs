//! The question about the scripts that another program changed.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::kit::buttons::{self, ButtonState, RowOptions};
use super::kit::form::choice_value;
use super::kit::geometry::{centered, list_rows, short_modal_width};
use super::kit::list::{Viewport, draw_rows};
use super::kit::modal::open_modal;
use super::kit::style::{self, ListRow};
use super::text;
use super::theme::Theme;
use crate::grid::printable;

/// What the question shows for one script.
pub struct ConflictRowView<'a> {
    pub name: &'a str,
    pub removed: bool,
    /// The label of the chosen action. `None` waits for the user.
    pub choice: Option<&'static str>,
}

/// The screen areas of the last frame. The mouse uses them.
#[derive(Clone, Debug, Default)]
pub struct ConflictLayout {
    pub rows: Vec<Rect>,
    /// The areas of **Apply** and **Decide later**.
    pub buttons: [Rect; 2],
}

const WIDTH: u16 = 66;
/// The columns of a name in a window of `WIDTH`. A wider window gives its columns to it.
const NAME_WIDTH: usize = 24;
/// The rows of the list on a small screen.
const ROWS: u16 = 12;
/// The rows of the window without the list: the borders, the note, and the buttons.
const CHROME_ROWS: u16 = 6;

pub fn draw_conflicts(
    frame: &mut Frame,
    rows: &[ConflictRowView<'_>],
    cursor: usize,
    note: Option<&str>,
    theme: &Theme,
) -> ConflictLayout {
    let screen = frame.area();
    let limit = list_rows(screen.height, CHROME_ROWS, ROWS);
    let height = u16::try_from(rows.len()).unwrap_or(limit).min(limit) + CHROME_ROWS;
    let area = centered(screen, short_modal_width(screen.width, WIDTH), height);
    let name_width = NAME_WIDTH + usize::from(area.width.saturating_sub(WIDTH));
    let inner = open_modal(frame, area, text::TITLE_CONFLICTS, theme);
    let mut layout = ConflictLayout::default();
    let view = Viewport {
        first: 0,
        rows: usize::from(inner.height.saturating_sub(4)),
        cursor,
    };
    draw_rows(frame, inner, rows, view, |frame, line, row, selected| {
        let state = if row.removed {
            text::STATE_REMOVED
        } else {
            text::STATE_CHANGED
        };
        let name: String = printable(row.name).chars().take(name_width).collect();
        let choice = choice_value(row.choice.unwrap_or(text::CHOICE_NONE));
        let style = style::list_row_style(
            if selected {
                ListRow::Selected
            } else {
                ListRow::Normal
            },
            theme,
        );
        let spans = vec![
            Span::raw(format!("{name:<name_width$} {state:<9} ")),
            Span::styled(choice, style),
        ];
        frame.render_widget(Paragraph::new(Line::from(spans)), line);
        layout.rows.push(line);
    });
    let note_line = Rect {
        y: inner.bottom().saturating_sub(3),
        height: 1,
        ..inner
    };
    if let Some(note) = note {
        frame.render_widget(Paragraph::new(Line::styled(note, theme.error)), note_line);
    }
    let buttons = Rect {
        y: inner.bottom().saturating_sub(1),
        height: 1,
        ..inner
    };
    let items: Vec<(usize, String, ButtonState)> = [text::BUTTON_APPLY, text::BUTTON_LATER]
        .into_iter()
        .enumerate()
        .map(|(index, label)| {
            let state = if cursor == rows.len() + index {
                ButtonState::Focused
            } else {
                ButtonState::Idle
            };
            (index, theme.button(label), state)
        })
        .collect();
    for (index, rect) in buttons::draw(frame, buttons, RowOptions::DIALOG, &items, theme) {
        layout.buttons[index] = rect;
    }
    layout
}
