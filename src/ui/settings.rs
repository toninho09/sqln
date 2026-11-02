//! The settings dialog.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use super::console::{centered, modal_width};
use super::float_block;
use super::theme::Theme;
use super::{input_spans, text};
use crate::grid::printable;
use crate::settings::{Field, SECTIONS, SettingsForm};
use crate::store::Scope;

pub(super) const WIDTH: u16 = 72;
/// The columns of the sidebar, without the divider.
pub(super) const SIDEBAR_WIDTH: u16 = 16;
const LABEL_WIDTH: u16 = 14;
/// The value columns of a number field.
const NUMBER_WIDTH: usize = 12;
/// The rows of the note above the buttons.
pub(super) const NOTE_ROWS: u16 = 2;
/// The columns before a label: a space, the focus mark, and a space.
pub(super) const LEFT: u16 = 3;
/// The empty columns after a value.
pub(super) const RIGHT: u16 = 2;
/// The rows inside the window, from its top.
const SCOPE_ROW: u16 = 1;
const TITLE_ROW: u16 = 3;
const FIRST_FIELD_ROW: u16 = 4;
/// The rows after the last field: a gap, the note, the buttons, and a gap.
pub(super) const FOOTER_ROWS: u16 = NOTE_ROWS + 3;
/// The rows of the top and bottom borders.
pub(super) const BORDER_ROWS: u16 = 2;

/// The screen areas of the sidebar and the fields. The mouse uses them.
#[derive(Clone, Debug, Default)]
pub struct SettingsLayout {
    /// One area for each row of `SECTIONS`, in the same order.
    pub sections: Vec<Rect>,
    pub fields: Vec<(Field, Rect)>,
}

/// The window has the height of the largest section, and all sections keep the same size.
fn window_height() -> u16 {
    let fields = SECTIONS.iter().map(|section| section.fields.len()).max();
    let fields = u16::try_from(fields.unwrap_or(0)).unwrap_or(u16::MAX);
    BORDER_ROWS + FIRST_FIELD_ROW + fields + FOOTER_ROWS
}

fn label(field: Field) -> &'static str {
    match field {
        Field::Scope => text::FIELD_SCOPE,
        Field::Keyboard => text::FIELD_KEYBOARD,
        Field::PageSize => text::FIELD_PAGE_SIZE,
        Field::RowCap => text::FIELD_ROW_CAP,
        Field::Budget => text::FIELD_BUDGET,
        Field::ReopenTabs => text::FIELD_REOPEN_TABS,
        Field::QueryHistory => text::FIELD_QUERY_HISTORY,
        Field::BellAfter => text::FIELD_BELL_AFTER,
        Field::FormatJson => text::FIELD_FORMAT_JSON,
        Field::SuggestionKeys => text::FIELD_SUGGESTION_KEYS,
        Field::UndoLimit => text::FIELD_UNDO_LIMIT,
        Field::Theme => text::FIELD_THEME,
        Field::Sections | Field::Save | Field::Cancel => "",
    }
}

/// Returns the text of a choice.
fn choice(form: &SettingsForm, field: Field) -> String {
    match field {
        Field::Scope if form.scope == Scope::Project => text::VALUE_SCOPE_PROJECT.to_owned(),
        Field::Scope => text::VALUE_SCOPE_GLOBAL.to_owned(),
        Field::Keyboard => form
            .keyboard()
            .map_or_else(|| form.inherited(field), text::keyboard_name),
        Field::ReopenTabs => form.reopen_tabs().map_or_else(
            || form.inherited(field),
            |value| text::on_off(value).to_owned(),
        ),
        Field::QueryHistory => form.query_history().map_or_else(
            || form.inherited(field),
            |value| text::on_off(value).to_owned(),
        ),
        Field::FormatJson => form.format_json().map_or_else(
            || form.inherited(field),
            |value| text::on_off(value).to_owned(),
        ),
        Field::SuggestionKeys => form.suggestion_keys().map_or_else(
            || form.inherited(field),
            |keys| text::suggestion_keys_name(keys).to_owned(),
        ),
        Field::Theme => form.theme().map_or_else(
            || form.inherited(field),
            |name| text::theme_name(name).to_owned(),
        ),
        _ => String::new(),
    }
}

fn value<'a>(form: &SettingsForm, field: Field, theme: &Theme) -> Line<'a> {
    let focused = form.focus == field;
    if let Some(input) = form.input(field) {
        let chars: Vec<char> = input.text().chars().collect();
        let mut spans = input_spans(&chars, input.cursor(), NUMBER_WIDTH, focused, theme).0;
        if input.text().is_empty() {
            spans.push(Span::styled(form.inherited(field), theme.hint));
        }
        return Line::from(spans);
    }
    let name = choice(form, field);
    // Without a project, the scope has one value, so it is not a choice.
    if field == Field::Scope && !form.has_project() {
        return Line::raw(name);
    }
    let style = if focused {
        theme.dialog_focus
    } else {
        Style::new()
    };
    Line::from(Span::styled(format!("< {name} >"), style))
}

fn draw_field_row(frame: &mut Frame, area: Rect, form: &SettingsForm, field: Field, theme: &Theme) {
    draw_row(
        frame,
        area,
        (label(field), LABEL_WIDTH),
        form.focus == field,
        value(form, field, theme),
        theme,
    );
}

/// Draws one row: the focus mark, the label, and the value.
pub(super) fn draw_row(
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
    let label_style = if focused {
        theme.panel_title_focused
    } else {
        Style::new()
    };
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
    frame.buffer_mut().set_style(
        value_area,
        if focused {
            theme.field_focus
        } else {
            theme.field
        },
    );
    let text_area = Rect {
        x: value_area.x + 1,
        width: value_area.width.saturating_sub(2),
        ..value_area
    };
    frame.render_widget(Paragraph::new(value), text_area);
}

/// Returns the columns that the button of a path field takes from its value, with a gap.
pub(super) fn browse_width(theme: &Theme) -> u16 {
    u16::try_from(theme.button(text::BUTTON_BROWSE).width() + 1).unwrap_or(0)
}

/// Draws the button of a path field at the end of its row, and returns its area.
pub(super) fn draw_browse_button(
    frame: &mut Frame,
    row: Rect,
    focused: bool,
    theme: &Theme,
) -> Rect {
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
pub(super) fn value_width(row: Rect, label_width: u16) -> usize {
    usize::from(row.width.saturating_sub(LEFT + label_width + RIGHT + 2))
}

/// Draws the rows of a sidebar and returns them. The open row stays marked when the
/// focus goes to the fields, as the row of the explorer does.
pub(super) fn draw_sidebar_rows(
    frame: &mut Frame,
    area: Rect,
    titles: &[&str],
    (open, focused): (usize, bool),
    theme: &Theme,
) -> Vec<Rect> {
    let mut rows = Vec::new();
    for (y, (index, title)) in (area.y..).zip(titles.iter().enumerate()) {
        let row = Rect {
            y,
            height: 1,
            ..area
        };
        rows.push(row);
        if index == open {
            let style = if focused {
                theme.tree_selected
            } else {
                theme.tree_cursor
            };
            frame.buffer_mut().set_style(row, style);
        }
        if index == open && focused {
            let mark = Span::styled(theme.icons.focus_mark, theme.panel_title_focused);
            frame.render_widget(
                Paragraph::new(mark),
                Rect {
                    x: row.x + 1,
                    width: 1,
                    ..row
                },
            );
        }
        frame.render_widget(
            Paragraph::new(*title),
            Rect {
                x: row.x + LEFT,
                width: row.width.saturating_sub(LEFT),
                ..row
            },
        );
    }
    rows
}

/// Draws the line between the sidebar and the fields. The line joins the window border.
pub(super) fn draw_divider(frame: &mut Frame, window: Rect, x: u16, theme: &Theme) {
    for y in window.y..window.bottom() {
        let symbol = if y == window.y {
            "┬"
        } else if y + 1 == window.bottom() {
            "┴"
        } else {
            "│"
        };
        frame
            .buffer_mut()
            .set_string(x, y, symbol, theme.float_border);
    }
}

/// Draws a note that ends on the row above `row`. A long note goes up to the next row.
pub(super) fn draw_note(frame: &mut Frame, content: Rect, row: u16, note: &str, style: Style) {
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

/// Draws a row of buttons and returns their areas.
pub(super) fn draw_buttons<T: Copy + PartialEq>(
    frame: &mut Frame,
    content: Rect,
    row: u16,
    (buttons, focus): (&[(T, &str)], T),
    theme: &Theme,
) -> Vec<(T, Rect)> {
    let mut areas = Vec::new();
    let mut spans = Vec::new();
    let mut x = content.x;
    for (field, label) in buttons {
        let style = if focus == *field {
            theme.dialog_focus
        } else {
            theme.dialog_option
        };
        let name = theme.button(label);
        let width = u16::try_from(name.width()).unwrap_or(0);
        areas.push((*field, Rect::new(x, row, width, 1)));
        spans.push(Span::styled(name, style));
        spans.push(Span::raw("  "));
        x += width + 2;
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect {
            y: row,
            height: 1,
            ..content
        },
    );
    areas
}

pub fn draw_settings(frame: &mut Frame, form: &SettingsForm, theme: &Theme) -> SettingsLayout {
    let width = modal_width(frame.area().width, WIDTH);
    let area = centered(frame.area(), width, window_height());
    frame.render_widget(Clear, area);
    let block = float_block(text::TITLE_SETTINGS, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let sidebar = Rect {
        y: inner.y + 1,
        width: SIDEBAR_WIDTH,
        height: inner.height.saturating_sub(1),
        ..inner
    };
    draw_divider(frame, area, sidebar.right(), theme);
    let pane = Rect {
        x: sidebar.right() + 1,
        width: inner.width.saturating_sub(SIDEBAR_WIDTH + 1),
        ..inner
    };
    let titles: Vec<&str> = SECTIONS.iter().map(|section| section.title).collect();
    let open = (form.section, form.focus == Field::Sections);
    let mut layout = SettingsLayout {
        sections: draw_sidebar_rows(frame, sidebar, &titles, open, theme),
        fields: Vec::new(),
    };

    let scope = Rect {
        y: pane.y + SCOPE_ROW,
        height: 1,
        ..pane
    };
    draw_field_row(frame, scope, form, Field::Scope, theme);
    if form.has_project() {
        layout.fields.push((Field::Scope, scope));
    }

    let section = &SECTIONS[form.section];
    let content = Rect {
        x: pane.x + LEFT,
        y: pane.y + TITLE_ROW,
        width: pane.width.saturating_sub(LEFT + RIGHT),
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(section.title.to_uppercase()).style(theme.panel_title),
        content,
    );
    for (y, field) in (pane.y + FIRST_FIELD_ROW..).zip(section.fields) {
        let row = Rect {
            y,
            height: 1,
            ..pane
        };
        draw_field_row(frame, row, form, *field, theme);
        layout.fields.push((*field, row));
    }

    let button_row = pane.bottom().saturating_sub(2);
    if let Some(note) = &form.note {
        draw_note(frame, content, button_row, note, theme.error);
    }
    let buttons = [
        (Field::Save, text::BUTTON_SAVE),
        (Field::Cancel, text::OPTION_CANCEL),
    ];
    layout.fields.extend(draw_buttons(
        frame,
        content,
        button_row,
        (&buttons, form.focus),
        theme,
    ));
    layout
}
