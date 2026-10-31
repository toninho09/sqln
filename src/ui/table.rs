//! The table view: the title, the filter bar, the grid, and the status lines.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use super::console::{draw_buttons, draw_grid, search_line, usage_bar};
use super::theme::Theme;
use super::{draw_suggestions, input_spans, text};
use crate::console::{Message, MessageLevel, Phase};
use crate::exec::ReadLimit;
use crate::grid::printable;
use crate::line_input::LineInput;
use crate::table::{TableFocus, TableTab};

const LABEL_WIDTH: u16 = 9;

/// A button of the filter bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableControl {
    Apply,
    Clear,
    LoadMore,
    Stop,
}

/// The screen areas of the last frame. The mouse uses them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableLayout {
    /// The rows of the two fields, with their labels.
    pub filter: Rect,
    pub order: Rect,
    pub controls: Rect,
    /// Each button, with its first column and the column after its end.
    pub buttons: Vec<(u16, u16, TableControl)>,
    pub grid: Rect,
}

pub struct TableBar<'a> {
    pub connection: &'a str,
    pub pool: Option<(usize, usize)>,
    pub data: (usize, usize),
    /// True when the data budget refuses a new page.
    pub blocked: bool,
}

/// Writes the label of a table tab.
pub fn tab_label(tab: &TableTab) -> String {
    super::main_tab_label(&tab.label(), tab.is_busy())
}

fn control_items(tab: &TableTab, blocked: bool) -> Vec<(TableControl, String, bool)> {
    let idle = !tab.is_busy();
    let button = |label: &str| format!("[{label}]");
    vec![
        (TableControl::Apply, button(text::BUTTON_APPLY), idle),
        (TableControl::Clear, button(text::BUTTON_CLEAR), idle),
        (TableControl::LoadMore, button(text::BUTTON_LOAD_MORE), idle && tab.has_more && !blocked),
        (TableControl::Stop, button(text::CONTROL_STOP), !idle),
    ]
}

/// Draws a field between brackets. It returns the area of the text and the index
/// of its first visible character.
fn draw_field(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    input: &LineInput,
    focused: bool,
    theme: &Theme,
) -> (Rect, usize) {
    let [name, field] =
        Layout::horizontal([Constraint::Length(LABEL_WIDTH), Constraint::Min(0)]).areas(area);
    frame.render_widget(Paragraph::new(label), name);
    let width = usize::from(field.width).saturating_sub(2).max(1);
    let chars: Vec<char> = printable(input.text()).chars().collect();
    let (spans, first) = input_spans(&chars, input.cursor(), width, focused, theme);
    let used: usize = spans.iter().map(Span::width).sum();
    let mut line = vec![Span::raw("[")];
    line.extend(spans);
    line.push(Span::raw(" ".repeat(width.saturating_sub(used))));
    line.push(Span::raw("]"));
    frame.render_widget(Paragraph::new(Line::from(line)), field);
    (Rect { x: field.x + 1, width: field.width.saturating_sub(2), ..field }, first)
}

fn message_style(message: &Message, theme: &Theme) -> Style {
    match message.level {
        MessageLevel::Info => Style::new(),
        MessageLevel::Error => theme.error,
    }
}

/// Writes the state of the loaded rows.
fn summary_text(tab: &TableTab) -> String {
    let mut parts = vec![text::rows_loaded(tab.loaded())];
    match tab.phase {
        Phase::Waiting => parts.push(text::TABLE_WAITING.to_owned()),
        Phase::Running => parts.push(text::TABLE_LOADING.to_owned()),
        Phase::Stopping => parts.push(text::TABLE_STOPPING.to_owned()),
        Phase::Idle => match tab.limit {
            ReadLimit::Budget => parts.push(text::TABLE_BUDGET.to_owned()),
            ReadLimit::ValueTooLarge => parts.push(text::TABLE_VALUE_TOO_LARGE.to_owned()),
            _ if tab.has_more => parts.push(text::TABLE_MORE.to_owned()),
            _ if tab.message.is_none() => parts.push(text::TABLE_NO_MORE_ROWS.to_owned()),
            _ => {}
        },
    }
    parts.join(" · ")
}

/// Draws the table view. It returns the areas for the mouse.
pub fn draw_table(
    frame: &mut Frame,
    area: Rect,
    tab: &mut TableTab,
    bar: &TableBar,
    focused: bool,
    theme: &Theme,
) -> TableLayout {
    let [title, filter, order, controls, grid, summary, notice] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let usage = usage_bar(bar.pool, bar.data);
    let usage_width = u16::try_from(usage.width()).unwrap_or(0).saturating_add(1);
    let [name, usage_area] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(usage_width)]).areas(title);
    let heading = text::table_title(bar.connection, &tab.relation.schema, &tab.relation.relation, tab.is_view);
    frame.render_widget(Paragraph::new(printable(&heading)).style(theme.bar), name);
    frame.render_widget(Paragraph::new(usage).style(theme.bar).alignment(Alignment::Right), usage_area);

    let (filter_text, filter_first) = draw_field(
        frame,
        filter,
        text::LABEL_WHERE,
        &tab.filter,
        focused && tab.focus == TableFocus::Filter,
        theme,
    );
    let (order_text, order_first) = draw_field(
        frame,
        order,
        text::LABEL_ORDER_BY,
        &tab.order,
        focused && tab.focus == TableFocus::Order,
        theme,
    );
    let buttons = draw_buttons(frame, controls, control_items(tab, bar.blocked), theme);

    let message_in_grid = tab.result.table.columns.is_empty() && tab.message.is_some();
    if tab.result.table.columns.is_empty() {
        draw_empty(frame, grid, tab, theme);
    } else {
        let grid_focused = focused && tab.focus == TableFocus::Grid;
        draw_grid(frame.buffer_mut(), grid, &mut tab.result, grid_focused, theme);
    }

    let line = match &tab.result.search {
        Some(search) => search_line(search, &tab.result.table, theme),
        None => Line::styled(summary_text(tab), theme.hint),
    };
    frame.render_widget(Paragraph::new(line), summary);
    let note = match &tab.message {
        Some(message) if !message_in_grid => {
            Some(Line::styled(printable(&message.text), message_style(message, theme)))
        }
        _ if tab.applied_order().is_empty() => Some(Line::styled(text::TABLE_NO_ORDER, theme.hint)),
        _ => None,
    };
    if let Some(note) = note {
        frame.render_widget(Paragraph::new(note), notice);
    }

    if let Some(list) = &tab.list {
        let (field, first, input) = match tab.focus {
            TableFocus::Filter => (filter_text, filter_first, &tab.filter),
            _ => (order_text, order_first, &tab.order),
        };
        let start = input.text()[..list.completion.replace.start.min(input.text().len())].chars().count();
        let offset = u16::try_from(start.saturating_sub(first)).unwrap_or(0);
        let anchor = Position::new(field.x.saturating_add(offset).min(field.right()), field.y);
        draw_suggestions(frame, area, anchor, list, theme);
    }
    TableLayout { filter, order, controls, buttons, grid }
}

/// Draws the area of a grid without columns: the reason, or the wait.
fn draw_empty(frame: &mut Frame, area: Rect, tab: &TableTab, theme: &Theme) {
    let line = match &tab.message {
        Some(message) => Line::styled(printable(&message.text), message_style(message, theme)),
        None if tab.is_busy() => Line::styled(text::TABLE_LOADING, theme.hint),
        None => return,
    };
    frame.render_widget(Paragraph::new(line).wrap(Wrap { trim: false }), area);
}
