//! The console view: bars, editor, results, grid, dialogs, and overlays.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use super::theme::Theme;
use super::wrap::WrappedText;
use super::{draw_editor, text};
use crate::connector::TransactionState;
use crate::console::{Console, Focus, MessageLevel, Phase, ResultTab};
use crate::grid::{GridView, ResultTable, RowSearch, Value, printable};

const MIB: usize = 1024 * 1024;

/// The screen areas of the last frame. The mouse uses them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConsoleLayout {
    pub editor: Rect,
    pub tabs: Rect,
    pub results: Rect,
}

pub struct BarData<'a> {
    pub connection: &'a str,
    pub pool: Option<(usize, usize)>,
    pub data: (usize, usize),
}

fn transaction_text(state: TransactionState) -> &'static str {
    match state {
        TransactionState::None => text::TRANSACTION_NONE,
        TransactionState::Open => text::TRANSACTION_OPEN,
        TransactionState::Failed => text::TRANSACTION_FAILED,
        TransactionState::Unknown => text::TRANSACTION_UNKNOWN,
    }
}

fn phase_text(phase: Phase) -> &'static str {
    match phase {
        Phase::Idle => text::PHASE_IDLE,
        Phase::Waiting => text::PHASE_WAITING,
        Phase::Running => text::PHASE_RUNNING,
        Phase::Stopping => text::PHASE_STOPPING,
    }
}

/// Writes the state of the console. The connection name gives way to the state,
/// because the user must always see the transaction and the execution.
fn top_bar(console: &Console, bar: &BarData, width: usize) -> String {
    let mode = if console.manual { text::MODE_MANUAL } else { text::MODE_AUTO };
    let transaction = transaction_text(console.transaction);
    let phase = phase_text(console.phase);
    let state = format!(" · {mode} · {transaction} · {phase}");
    let name = printable(bar.connection);
    let room = width.saturating_sub(state.width());
    if name.width() <= room {
        return format!("{name}{state}");
    }
    let (start, _) = fit(&name, room.saturating_sub(1));
    format!("{start}…{state}")
}

fn usage_bar(bar: &BarData) -> String {
    let pool = bar.pool.map_or(String::new(), |(busy, limit)| format!("Pool {busy}/{limit} · "));
    format!("{pool}Data {}/{} MiB", bar.data.0.div_ceil(MIB), bar.data.1 / MIB)
}

pub fn draw_console(
    frame: &mut Frame,
    area: Rect,
    console: &mut Console,
    bar: &BarData,
    theme: &Theme,
) -> ConsoleLayout {
    let [top, editor, tabs, results, summary] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Percentage(50),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    // The usage text keeps its place, because a long file name can fill the row.
    let usage = usage_bar(bar);
    let usage_width = u16::try_from(usage.width()).unwrap_or(0).saturating_add(1);
    let [state, usage_area] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(usage_width)]).areas(top);
    let state_text = top_bar(console, bar, usize::from(state.width));
    frame.render_widget(Paragraph::new(state_text).style(theme.bar), state);
    frame.render_widget(Paragraph::new(usage).style(theme.bar).alignment(Alignment::Right), usage_area);
    draw_editor(frame, editor, &mut console.editor, theme);
    draw_tabs(frame, tabs, console, theme);

    let focused = console.focus == Focus::Results;
    if console.showing_messages() {
        draw_messages(frame, results, console, theme);
    } else if let Some(result) = console.results.get_mut(console.active) {
        draw_grid(frame.buffer_mut(), results, result, focused, theme);
        let line = match &result.search {
            Some(search) => search_line(search, &result.table, theme),
            None => Line::from(Span::styled(printable(&result.summary), theme.hint)),
        };
        frame.render_widget(Paragraph::new(line), summary);
    }
    ConsoleLayout { editor, tabs, results }
}

fn search_line(search: &RowSearch, table: &ResultTable, theme: &Theme) -> Line<'static> {
    let count = if search.query().is_empty() {
        String::new()
    } else {
        text::search_count(search.position(), search.total(), search.is_partial(table))
    };
    Line::from(vec![
        Span::raw(format!("{}{}", text::FIND_LABEL, printable(search.query()))),
        Span::styled(" ", theme.cursor),
        Span::styled(format!(" {count}"), theme.hint),
    ])
}

fn tab_labels(console: &Console) -> Vec<String> {
    let mut labels: Vec<String> =
        console.results.iter().map(|result| text::result_tab(result.number)).collect();
    labels.push(text::MESSAGES_TAB.to_owned());
    labels
}

fn draw_tabs(frame: &mut Frame, area: Rect, console: &Console, theme: &Theme) {
    let active = console.active.min(console.results.len());
    let mut spans = Vec::new();
    for (index, label) in tab_labels(console).into_iter().enumerate() {
        let style = if index == active { theme.tab_active } else { theme.tab };
        spans.push(Span::styled(label, style));
        spans.push(Span::raw("  "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Returns the tab at a screen column.
pub fn tab_at(console: &Console, area: Rect, column: u16) -> Option<usize> {
    let mut x = usize::from(area.x);
    for (index, label) in tab_labels(console).iter().enumerate() {
        let end = x + label.width();
        if usize::from(column) >= x && usize::from(column) < end {
            return Some(index);
        }
        x = end + 2;
    }
    None
}

fn draw_messages(frame: &mut Frame, area: Rect, console: &Console, theme: &Theme) {
    if console.messages.is_empty() {
        let line = Span::styled(text::NO_MESSAGES, theme.hint);
        frame.render_widget(Paragraph::new(Line::from(line)), area);
        return;
    }
    let lines: Vec<Line> = console
        .messages
        .iter()
        .map(|message| {
            let style = match message.level {
                MessageLevel::Info => Style::new(),
                MessageLevel::Error => theme.error,
            };
            Line::styled(printable(&message.text), style)
        })
        .collect();
    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((console.messages_scroll, 0));
    frame.render_widget(paragraph, area);
}

fn fit(text: &str, width: usize) -> (String, usize) {
    let mut fitted = String::new();
    let mut used = 0;
    for c in text.chars() {
        let char_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if used + char_width > width {
            break;
        }
        fitted.push(c);
        used += char_width;
    }
    (fitted, used)
}

/// Lists the visible columns as `(column, x, width)`.
fn visible_columns(table: &ResultTable, view: &GridView, area: Rect) -> Vec<(usize, u16, u16)> {
    let mut x = area.x;
    let mut columns = Vec::new();
    for index in view.left..table.columns.len() {
        if x >= area.right() {
            break;
        }
        let width = u16::try_from(table.width(index)).unwrap_or(u16::MAX).min(area.right() - x);
        columns.push((index, x, width));
        x = x.saturating_add(width).saturating_add(1);
    }
    columns
}

fn draw_cell(buffer: &mut Buffer, x: u16, y: u16, width: u16, value: &Value, style: Style) {
    let (text, used) = fit(&value.preview(usize::from(width)), usize::from(width));
    let offset = if matches!(value, Value::Number(_)) {
        width.saturating_sub(u16::try_from(used).unwrap_or(width))
    } else {
        0
    };
    buffer.set_style(Rect::new(x, y, width, 1), style);
    buffer.set_stringn(x + offset, y, text, usize::from(width), style);
}

fn draw_grid(buffer: &mut Buffer, area: Rect, result: &mut ResultTab, focused: bool, theme: &Theme) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let rows = usize::from(area.height - 1);
    let table = &result.table;
    result.view.keep_visible(rows, table, usize::from(area.width));
    let view = result.view;
    let columns = visible_columns(table, &view, area);

    for (column, x, width) in &columns {
        let (name, _) = fit(&printable(&table.columns[*column]), usize::from(*width));
        buffer.set_stringn(*x, area.y, name, usize::from(*width), theme.header);
    }

    let selection = view.selection();
    for (offset, row) in table.rows.iter().skip(view.top).take(rows).enumerate() {
        let y = area.y + 1 + offset as u16;
        let row_index = view.top + offset;
        for (column, x, width) in &columns {
            let Some(value) = row.get(*column) else { continue };
            let selected = selection.is_some_and(|range| {
                (range.top..=range.bottom).contains(&row_index)
                    && (range.left..=range.right).contains(column)
            });
            let found = |check: fn(&RowSearch, usize, usize) -> bool| {
                result.search.as_ref().is_some_and(|search| check(search, row_index, *column))
            };
            let style = if focused && row_index == view.row && *column == view.col {
                theme.cell_cursor
            } else if found(RowSearch::is_current) {
                theme.search_current
            } else if found(RowSearch::is_match) {
                theme.search_match
            } else if selected {
                theme.cell_selected
            } else if matches!(value, Value::Null) {
                theme.null
            } else {
                Style::new()
            };
            draw_cell(buffer, *x, y, *width, value, style);
        }
    }
}

/// Returns the column that ends at a screen cell of the header row.
pub fn column_edge_at(result: &ResultTab, area: Rect, column: u16, row: u16) -> Option<usize> {
    if row != area.y {
        return None;
    }
    visible_columns(&result.table, &result.view, area)
        .into_iter()
        .find(|(_, x, width)| column == x + width)
        .map(|(index, _, _)| index)
}

/// Returns the row and the column at a screen cell.
pub fn grid_cell_at(result: &ResultTab, area: Rect, column: u16, row: u16) -> Option<(usize, usize)> {
    if row <= area.y || row >= area.bottom() {
        return None;
    }
    let row_index = result.view.top + usize::from(row - area.y - 1);
    if row_index >= result.table.rows.len() {
        return None;
    }
    visible_columns(&result.table, &result.view, area)
        .into_iter()
        .find(|(_, x, width)| column >= *x && column <= x + width)
        .map(|(index, _, _)| (row_index, index))
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

pub fn draw_dialog(
    frame: &mut Frame,
    title: &str,
    body: &str,
    options: &[&str],
    focus: usize,
    theme: &Theme,
) {
    let area = frame.area();
    let width = area.width.saturating_sub(8).clamp(20, 72);
    let inner_width = usize::from(width.saturating_sub(2)).max(1);
    let body_rows: usize = body
        .lines()
        .map(|line| line.width().div_ceil(inner_width).max(1))
        .sum();
    let box_area = centered(area, width, u16::try_from(body_rows).unwrap_or(10).saturating_add(4));
    frame.render_widget(Clear, box_area);
    let block = Block::bordered().title(format!(" {title} ")).style(theme.dialog);
    let inner = block.inner(box_area);
    frame.render_widget(block, box_area);

    let [message, buttons] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
    frame.render_widget(Paragraph::new(printable_lines(body)).wrap(Wrap { trim: false }), message);

    let mut spans = Vec::new();
    for (index, label) in options.iter().enumerate() {
        let style = if index == focus { theme.dialog_focus } else { theme.dialog_option };
        spans.push(Span::styled(format!("[{label}]"), style));
        spans.push(Span::raw("  "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), buttons);
}

/// Keeps the line breaks of a text and replaces the other control characters.
fn printable_lines(text: &str) -> Text<'static> {
    Text::from(text.split('\n').map(|line| Line::raw(printable(line))).collect::<Vec<_>>())
}

/// Draws a long text. It returns the first row, because the end of the text limits it.
pub fn draw_overlay(
    frame: &mut Frame,
    title: &str,
    contents: &mut WrappedText,
    footer: &str,
    scroll: usize,
    theme: &Theme,
) -> usize {
    let area = frame.area();
    let box_area = centered(area, area.width.saturating_sub(8).min(90), area.height.saturating_sub(4));
    frame.render_widget(Clear, box_area);
    let block = Block::bordered()
        .title(printable(title))
        .title_bottom(Line::from(printable(footer)).alignment(Alignment::Right))
        .style(theme.dialog);
    let inner = block.inner(box_area);
    contents.set_width(usize::from(inner.width));
    let scroll = contents.last_row_at(scroll);
    let rows = (scroll..scroll + usize::from(inner.height))
        .map_while(|index| contents.row(index).map(|row| Line::raw(printable(row))))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(rows).block(block), box_area);
    scroll
}

pub fn draw_small_terminal(frame: &mut Frame, area: Rect, minimum: (u16, u16)) {
    let lines = vec![
        Line::raw(format!("{}: {} x {}", text::SMALL_TERMINAL, area.width, area.height)),
        Line::raw(format!("{}: {} x {}", text::MINIMUM_SIZE, minimum.0, minimum.1)),
    ];
    frame.render_widget(Paragraph::new(lines).block(Block::bordered().title("SQLN")), area);
}
