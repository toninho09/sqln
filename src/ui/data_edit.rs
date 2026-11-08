//! Cell editing and the read-only review of pending updates.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::kit::buttons::{self, RowOptions};
use super::kit::modal::open_modal;
use super::theme::Theme;
use super::{draw_editor, text};
use crate::data_edit::form::{Body, EditForm};
use crate::grid::printable;

#[derive(Clone, Debug, Default)]
pub struct EditLayout {
    pub contents: Rect,
    pub controls: Rect,
    pub buttons: Vec<(usize, Rect)>,
}

pub fn draw_edit(frame: &mut Frame, form: &mut EditForm, theme: &Theme) -> EditLayout {
    let screen = frame.area();
    let width = screen.width.saturating_sub(4).min(100);
    let height = screen.height.saturating_sub(4).min(26);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    );
    let inner = open_modal(frame, area, &printable(&form.title), theme);
    let [contents, note, controls] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);
    let review = matches!(form.body, Body::Review { .. });
    match &mut form.body {
        Body::Cell { editor, .. } => draw_editor(frame, contents, editor, form.focus == 0, theme),
        Body::Review {
            contents: value,
            scroll,
        } => {
            value.set_width(usize::from(contents.width));
            *scroll = value.last_row_at(*scroll);
            let lines: Vec<Line> = (0..usize::from(contents.height))
                .filter_map(|offset| {
                    value
                        .row(*scroll + offset)
                        .map(|row| Line::from(printable(row)))
                })
                .collect();
            frame.render_widget(Paragraph::new(lines), contents);
        }
    }
    let hint = form.note.as_deref().unwrap_or(if review {
        text::EDIT_REVIEW_HINT
    } else {
        text::EDIT_CELL_HINT
    });
    frame.render_widget(Paragraph::new(printable(hint)).style(theme.hint), note);
    let labels: Vec<&str> = if review {
        vec![text::EDIT_APPLY, text::OPTION_CANCEL]
    } else {
        vec![text::EDIT_KEEP, text::EDIT_SET_NULL, text::OPTION_CANCEL]
    };
    let items: Vec<(usize, String, bool)> = labels
        .into_iter()
        .enumerate()
        .map(|(index, label)| {
            let mark = if form.focus == index + 1 {
                format!("> {label} <")
            } else {
                label.to_owned()
            };
            (index + 1, theme.button(&mark), true)
        })
        .collect();
    let items = buttons::toolbar_items(items);
    let buttons = buttons::draw(frame, controls, RowOptions::TOOLBAR, &items, theme);
    EditLayout {
        contents,
        controls,
        buttons,
    }
}
