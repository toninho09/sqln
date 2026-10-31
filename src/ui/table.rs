//! The table view: the title, the filter bar, the grid, and the status lines.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::console::{draw_buttons, draw_grid, draw_panel_title, search_line};
use super::theme::Theme;
use super::{draw_suggestions, input_spans, text};
use crate::console::{Message, MessageLevel, Phase};
use crate::exec::ReadLimit;
use crate::grid::printable;
use crate::line_input::LineInput;
use crate::table::{TableFocus, TableTab};

const LABEL_WIDTH: u16 = 10;

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
    /// True when the data budget refuses a new page.
    pub blocked: bool,
    /// True on a tall screen, which has a title row.
    pub title: bool,
}

/// Writes the label of a table tab.
pub fn tab_label(tab: &TableTab, theme: &Theme) -> String {
    let icon = if tab.is_view { theme.icons.view } else { theme.icons.table };
    super::main_tab_label(icon, &tab.label(), false)
}

fn control_items(tab: &TableTab, blocked: bool, theme: &Theme) -> Vec<(TableControl, String, bool)> {
    let idle = !tab.is_busy();
    let icons = &theme.icons;
    vec![
        (TableControl::Apply, theme.tool(icons.apply, text::BUTTON_APPLY), idle),
        (TableControl::Clear, theme.tool(icons.clear, text::BUTTON_CLEAR), idle),
        (TableControl::LoadMore, theme.tool(icons.load_more, text::BUTTON_LOAD_MORE), idle && tab.has_more && !blocked),
        (TableControl::Stop, theme.tool(icons.stop, text::CONTROL_STOP), !idle),
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
    frame.render_widget(Paragraph::new(format!(" {label}")), name);
    let width = usize::from(field.width).saturating_sub(2).max(1);
    let chars: Vec<char> = printable(input.text()).chars().collect();
    let (spans, first) = input_spans(&chars, input.cursor(), width, focused, theme);
    let used: usize = spans.iter().map(Span::width).sum();
    let mut line = vec![Span::raw(theme.button_marks.0)];
    line.extend(spans);
    line.push(Span::raw(" ".repeat(width.saturating_sub(used))));
    line.push(Span::raw(theme.button_marks.1));
    frame.render_widget(Paragraph::new(Line::from(line)).style(theme.field), field);
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
        Constraint::Length(u16::from(bar.title)),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    if bar.title {
        let heading = text::table_title(bar.connection, &tab.relation.schema, &tab.relation.relation, tab.is_view);
        draw_panel_title(frame, title, &printable(&heading), focused, theme);
    }

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
    let buttons = draw_buttons(frame, controls, control_items(tab, bar.blocked, theme), theme);

    let message_in_grid = tab.result.table.columns.is_empty() && tab.message.is_some();
    if tab.result.table.columns.is_empty() {
        draw_empty(frame, grid, tab, theme);
    } else {
        let grid_focused = focused && tab.focus == TableFocus::Grid;
        draw_grid(frame.buffer_mut(), grid, &mut tab.result, grid_focused, theme);
    }

    let line = match &tab.result.search {
        Some(search) => search_line(search, &tab.result.table, theme),
        None => Line::styled(format!(" {}", summary_text(tab)), theme.hint),
    };
    frame.render_widget(Paragraph::new(line), summary);
    let note = match &tab.message {
        Some(message) if !message_in_grid => {
            Some(Line::styled(format!(" {}", printable(&message.text)), message_style(message, theme)))
        }
        _ if tab.applied_order().is_empty() => {
            Some(Line::styled(format!(" {}", text::TABLE_NO_ORDER), theme.hint))
        }
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
