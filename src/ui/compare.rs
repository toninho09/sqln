//! The compare view and its dialog. The view has a title, the grid of the differences,
//! and the rows of the state.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use super::console::{draw_grid, draw_panel_title, message_style, search_line};
use super::theme::Theme;
use super::widgets::{
    BORDER_ROWS, FOOTER_ROWS, LEFT, RIGHT, centered, choice_line, draw_buttons, draw_note,
    draw_row, flag_line, modal_width, open_modal, value_width,
};
use super::{draw_anchored_list, input_spans, text};
use crate::compare::Kind;
use crate::compare::form::{CompareForm, Field};
use crate::compare::tab::CompareTab;
use crate::console::Phase;
use crate::grid::printable;
use crate::line_input::LineInput;

const WIDTH: u16 = 72;
const LABEL_WIDTH: u16 = 24;
const FIRST_FIELD_ROW: u16 = 1;
const FIELD_ROWS: u16 = 8;

/// Writes the label of a compare tab.
pub fn tab_label(tab: &CompareTab, theme: &Theme) -> String {
    super::main_tab_label(theme.icons.schema, &tab.label(), false)
}

/// Draws the compare view. It returns the area of the grid, for the mouse.
pub fn draw_compare(
    frame: &mut Frame,
    area: Rect,
    tab: &mut CompareTab,
    title: bool,
    focused: bool,
    theme: &Theme,
) -> Rect {
    let [heading, grid, summary, notice] = Layout::vertical([
        Constraint::Length(u16::from(title)),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    if title {
        draw_panel_title(frame, heading, &printable(&tab.title()), focused, theme);
    }
    if tab.done {
        draw_grid(frame.buffer_mut(), grid, &mut tab.result, focused, theme);
    } else if let Some(message) = &tab.message {
        let paragraph = Paragraph::new(Line::styled(
            format!(" {}", printable(&message.text)),
            message_style(message, theme),
        ))
        .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, grid);
    }
    let line = match &tab.result.search {
        Some(search) if tab.done => search_line(search, &tab.result.table, theme),
        _ => Line::styled(format!(" {}", summary_text(tab)), theme.hint),
    };
    frame.render_widget(Paragraph::new(line), summary);
    if let Some(message) = tab.message.as_ref().filter(|_| tab.done) {
        let line = Line::styled(
            format!(" {}", printable(&message.text)),
            message_style(message, theme),
        );
        frame.render_widget(Paragraph::new(line), notice);
    }
    grid
}

fn summary_text(tab: &CompareTab) -> String {
    let mut parts = Vec::new();
    if tab.done {
        parts.push(tab.result.summary.clone());
        if tab.count > 0 && !tab.same_dialect() && tab.kind == Kind::Structure {
            parts.push(text::COMPARE_TWO_DIALECTS.to_owned());
        }
    }
    match tab.phase {
        Phase::Waiting => parts.push(text::TABLE_WAITING.to_owned()),
        Phase::Running => parts.push(text::TABLE_LOADING.to_owned()),
        Phase::Stopping => parts.push(text::TABLE_STOPPING.to_owned()),
        Phase::Idle => {}
    }
    parts.join(" · ")
}

/// The screen areas of the fields and the buttons. The mouse uses them.
#[derive(Clone, Debug, Default)]
pub struct CompareLayout {
    pub fields: Vec<(Field, Rect)>,
    /// The area of the shown list and the index of its first row.
    pub list: Option<(Rect, usize)>,
}

fn label(field: Field) -> &'static str {
    match field {
        Field::Kind => text::FIELD_COMPARE_KIND,
        Field::SourceConnection => text::FIELD_COMPARE_SOURCE,
        Field::SourceSchema => text::FIELD_COMPARE_SOURCE_SCHEMA,
        Field::SourceTable => text::FIELD_COMPARE_SOURCE_TABLE,
        Field::TargetConnection => text::FIELD_COMPARE_TARGET,
        Field::TargetSchema => text::FIELD_COMPARE_TARGET_SCHEMA,
        Field::TargetTable => text::FIELD_COMPARE_TARGET_TABLE,
        Field::Removals => text::FIELD_COMPARE_REMOVALS,
        Field::Compare => text::BUTTON_COMPARE,
        Field::Cancel => text::OPTION_CANCEL,
    }
}

fn value(form: &CompareForm, field: Field, width: usize, theme: &Theme) -> Line<'static> {
    let focused = form.focus == field;
    let side = field.side().map(|side| &form.sides[side]);
    match (field, side) {
        (Field::SourceTable | Field::TargetTable, Some(side)) => {
            input_line(&side.table, width, focused, theme)
        }
        (_, Some(_)) if focused && !form.search.text().is_empty() => {
            input_line(&form.search, width, true, theme)
        }
        (Field::SourceConnection | Field::TargetConnection, Some(_)) => {
            let name = form
                .choice(field.side().unwrap_or(0))
                .map_or(String::new(), |choice| printable(&choice.name));
            choice_line(&name, focused, theme)
        }
        (Field::Kind, _) => {
            let kind = match form.kind {
                Kind::Structure => text::KIND_STRUCTURE,
                Kind::Data => text::KIND_DATA,
            };
            choice_line(kind, focused, theme)
        }
        (_, Some(side)) => match side.schema_name() {
            Some(schema) => choice_line(&printable(schema), focused, theme),
            None => Line::styled(text::COMPARE_LOADING, theme.hint),
        },
        _ => flag_line(form.removals, focused, theme),
    }
}

fn input_line(input: &LineInput, width: usize, focused: bool, theme: &Theme) -> Line<'static> {
    let chars: Vec<char> = printable(input.text()).chars().collect();
    Line::from(input_spans(&chars, input.cursor(), width, focused, theme).0)
}

/// Draws the list of the focused field below its value. It returns the area of the list
/// and the index of its first row.
fn draw_field_list(
    frame: &mut Frame,
    form: &CompareForm,
    row: Rect,
    theme: &Theme,
) -> Option<(Rect, usize)> {
    let list = form.list.as_ref().filter(|list| list.is_shown())?;
    let names = &list.names;
    let width = names.iter().map(|(_, name)| name.width() + 2).max();
    let shape = (names.len(), list.selected, width.unwrap_or_default());
    let anchor = Position::new(row.x + LEFT + LABEL_WIDTH, row.y);
    let bounds = frame.area();
    Some(draw_anchored_list(
        frame,
        (bounds, anchor),
        shape,
        theme,
        |index, _| vec![Span::raw(format!(" {} ", printable(&names[index].1)))],
    ))
}

pub fn draw_compare_form(frame: &mut Frame, form: &CompareForm, theme: &Theme) -> CompareLayout {
    let height = BORDER_ROWS + FIRST_FIELD_ROW + FIELD_ROWS + FOOTER_ROWS;
    let width = modal_width(frame.area().width, WIDTH);
    let area = centered(frame.area(), width, height);
    let pane = open_modal(frame, area, text::TITLE_COMPARE, theme);
    let mut layout = CompareLayout::default();
    let fields = &Field::ALL[..usize::from(FIELD_ROWS)];
    for (y, field) in (pane.y + FIRST_FIELD_ROW..).zip(fields.iter().copied()) {
        let row = Rect {
            y,
            height: 1,
            ..pane
        };
        let line = value(form, field, value_width(row, LABEL_WIDTH), theme);
        draw_row(
            frame,
            row,
            (label(field), LABEL_WIDTH),
            form.focus == field,
            line,
            theme,
        );
        layout.fields.push((field, row));
    }
    let content = Rect {
        x: pane.x + LEFT,
        y: pane.y,
        width: pane.width.saturating_sub(LEFT + RIGHT),
        height: 1,
    };
    let button_row = pane.bottom().saturating_sub(2);
    match &form.note {
        Some(note) => draw_note(frame, content, button_row, note, theme.error),
        None => {
            let hint = match form.kind {
                Kind::Structure => text::COMPARE_HINT,
                Kind::Data => text::COMPARE_DATA_HINT,
            };
            draw_note(frame, content, button_row, hint, theme.hint);
        }
    }
    let buttons = [
        (Field::Compare, text::BUTTON_COMPARE),
        (Field::Cancel, text::OPTION_CANCEL),
    ];
    layout.fields.extend(draw_buttons(
        frame,
        content,
        button_row,
        (&buttons, form.focus),
        theme,
    ));
    let focused = layout.fields.iter().find(|(field, _)| *field == form.focus);
    if let Some(&(_, row)) = focused {
        layout.list = draw_field_list(frame, form, row, theme);
    }
    layout
}
