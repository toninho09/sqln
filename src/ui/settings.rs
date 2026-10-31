//! The settings dialog.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use super::console::centered;
use super::float_block;
use super::theme::Theme;
use super::{input_spans, text};
use crate::grid::printable;
use crate::settings::{Field, SettingsForm};
use crate::store::Scope;

const WIDTH: u16 = 66;
const HEIGHT: u16 = 14;
const LABEL_WIDTH: u16 = 14;
/// The value columns of a number field.
const NUMBER_WIDTH: usize = 12;
/// The rows of the note above the buttons.
const NOTE_ROWS: u16 = 2;
/// The columns before a label: a space, the focus mark, and a space.
const LEFT: u16 = 3;
/// The empty columns after a value.
const RIGHT: u16 = 2;

/// The screen areas of the fields. The mouse uses them.
#[derive(Clone, Debug, Default)]
pub struct SettingsLayout {
    pub fields: Vec<(Field, Rect)>,
}

/// Draws one row: the focus mark, the label, and the value.
fn draw_field_row(
    frame: &mut Frame,
    area: Rect,
    form: &SettingsForm,
    field: Field,
    label: &str,
    value: Line<'_>,
    theme: &Theme,
) {
    let focused = form.focus == field;
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
        width: LABEL_WIDTH,
        ..area
    };
    frame.render_widget(Paragraph::new(label).style(label_style), label_area);

    let start = area.x + LEFT + LABEL_WIDTH;
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

pub fn draw_settings(frame: &mut Frame, form: &SettingsForm, theme: &Theme) -> SettingsLayout {
    let area = centered(frame.area(), WIDTH, HEIGHT);
    frame.render_widget(Clear, area);
    let block = float_block(text::TITLE_SETTINGS, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut layout = SettingsLayout::default();
    let choice = |field: Field, value: String| {
        let style = if form.focus == field {
            theme.dialog_focus
        } else {
            Style::new()
        };
        Line::from(Span::styled(format!("< {value} >"), style))
    };

    let scope = match form.scope {
        Scope::Global => text::VALUE_SCOPE_GLOBAL,
        Scope::Project => text::VALUE_SCOPE_PROJECT,
    };
    let scope = if form.has_project() {
        choice(Field::Scope, scope.to_owned())
    } else {
        Line::raw(scope)
    };
    let rows = [
        (Field::Scope, text::FIELD_SCOPE, scope),
        (
            Field::Keyboard,
            text::FIELD_KEYBOARD,
            choice(
                Field::Keyboard,
                form.keyboard()
                    .map_or_else(|| form.inherited(Field::Keyboard), text::keyboard_name),
            ),
        ),
    ];
    for (offset, (field, label, value)) in rows.into_iter().enumerate() {
        let row = Rect {
            y: inner.y + offset as u16,
            height: 1,
            ..inner
        };
        draw_field_row(frame, row, form, field, label, value, theme);
        layout.fields.push((field, row));
    }

    let numbers = [
        (Field::PageSize, text::FIELD_PAGE_SIZE),
        (Field::RowCap, text::FIELD_ROW_CAP),
        (Field::Budget, text::FIELD_BUDGET),
    ];
    for (offset, (field, label)) in (2u16..).zip(numbers) {
        let Some(input) = form.input(field) else {
            continue;
        };
        let chars: Vec<char> = input.text().chars().collect();
        let mut spans = input_spans(
            &chars,
            input.cursor(),
            NUMBER_WIDTH,
            form.focus == field,
            theme,
        )
        .0;
        if input.text().is_empty() {
            spans.push(Span::styled(form.inherited(field), theme.hint));
        }
        let row = Rect {
            y: inner.y + offset,
            height: 1,
            ..inner
        };
        draw_field_row(frame, row, form, field, label, Line::from(spans), theme);
        layout.fields.push((field, row));
    }

    let reopen = form.reopen_tabs().map_or_else(
        || form.inherited(Field::ReopenTabs),
        |value| text::on_off(value).to_owned(),
    );
    let keys = form.suggestion_keys().map_or_else(
        || form.inherited(Field::SuggestionKeys),
        |keys| text::suggestion_keys_name(keys).to_owned(),
    );
    let rows = [
        (
            Field::ReopenTabs,
            text::FIELD_REOPEN_TABS,
            choice(Field::ReopenTabs, reopen),
        ),
        (
            Field::SuggestionKeys,
            text::FIELD_SUGGESTION_KEYS,
            choice(Field::SuggestionKeys, keys),
        ),
    ];
    for (offset, (field, label, value)) in (5u16..).zip(rows) {
        let row = Rect {
            y: inner.y + offset,
            height: 1,
            ..inner
        };
        draw_field_row(frame, row, form, field, label, value, theme);
        layout.fields.push((field, row));
    }

    let button_row = inner.bottom().saturating_sub(2);
    let content = Rect {
        x: inner.x + LEFT,
        y: button_row.saturating_sub(NOTE_ROWS),
        width: inner.width.saturating_sub(LEFT + RIGHT),
        height: NOTE_ROWS,
    };
    if let Some(note) = &form.note {
        // The note ends on the row above the buttons, and a long note goes up to the next row.
        let message = printable(note);
        let width = usize::from(content.width).max(1);
        let rows = u16::try_from(message.width().div_ceil(width))
            .unwrap_or(NOTE_ROWS)
            .clamp(1, NOTE_ROWS);
        let place = Rect {
            y: button_row.saturating_sub(rows),
            height: rows,
            ..content
        };
        let paragraph =
            Paragraph::new(Line::styled(message, theme.error)).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, place);
    }

    let style = |field| {
        if form.focus == field {
            theme.dialog_focus
        } else {
            theme.dialog_option
        }
    };
    let buttons = [
        (Field::Save, text::BUTTON_SAVE),
        (Field::Cancel, text::OPTION_CANCEL),
    ];
    let mut spans = Vec::new();
    let mut x = content.x;
    for (field, label) in buttons {
        let name = theme.button(label);
        let width = u16::try_from(name.width()).unwrap_or(0);
        layout
            .fields
            .push((field, Rect::new(x, button_row, width, 1)));
        spans.push(Span::styled(name, style(field)));
        spans.push(Span::raw("  "));
        x += width + 2;
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect {
            y: button_row,
            height: 1,
            ..content
        },
    );
    layout
}
