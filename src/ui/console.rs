//! The console view: bars, editor, results, grid, dialogs, and overlays.

use std::time::Instant;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use super::theme::Theme;
use super::welcome::{Welcome, draw_welcome};
use super::wrap::WrappedText;
use super::{draw_editor, draw_scrollbar, float_block, text};
use crate::connector::Namespace;
use crate::console::{Console, Focus, MessageLevel, Phase, ResultTab, RunClock, RunEnd};
use crate::grid::{COLUMN_GAP, GridView, ResultTable, RowSearch, Value, printable};

const PICKER_WIDTH: u16 = 48;
/// The rows of a list on a small screen.
const PICKER_ROWS: u16 = 12;
/// The rows of the list window without the list.
const PICKER_CHROME_ROWS: u16 = 3;
/// The text between two buttons of a toolbar.
pub(super) const BUTTON_GAP: u16 = 2;
/// The columns between a row number and the first column: a space, a rule, and a space.
const NUMBER_END: u16 = 3;
/// The columns between two columns of the grid.
const GAP: u16 = COLUMN_GAP as u16;
const RULE: &str = "─";

/// A button of the console bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Run,
    Script,
    Stop,
    Mode,
    Commit,
    Rollback,
    Schema,
}

/// The screen areas of the last frame. The mouse uses them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConsoleLayout {
    pub controls: Rect,
    /// Each button, with its first column and the column after its end.
    pub control_buttons: Vec<(u16, u16, Control)>,
    pub editor: Rect,
    pub tabs: Rect,
    pub results: Rect,
    /// The rows that the editor and the results share.
    pub split: Rect,
}

pub struct BarData<'a> {
    pub connection: &'a str,
    pub namespace: Namespace,
    /// The schema that a session reads without a selection, when the catalog gives it.
    pub default_schema: Option<&'a str>,
    pub transactions: bool,
    /// True on a tall screen, which has a title row.
    pub title: bool,
    /// The first screen, for a console without text.
    pub welcome: Option<Welcome>,
}

/// Draws the title of a panel. The title of the panel with the focus has its own style.
pub fn draw_panel_title(frame: &mut Frame, area: Rect, title: &str, focused: bool, theme: &Theme) {
    let style = if focused {
        theme.panel_title_focused
    } else {
        theme.panel_title
    };
    frame.render_widget(Paragraph::new(format!(" {title}")).style(style), area);
}

/// Lists the buttons of the console bar. A button that has no effect now looks disabled.
fn control_items(console: &Console, bar: &BarData, theme: &Theme) -> Vec<(Control, String, bool)> {
    let idle = !console.is_busy();
    let open = idle && console.in_transaction();
    let icons = &theme.icons;
    let mode = if console.manual {
        text::MODE_MANUAL
    } else {
        text::MODE_AUTO
    };
    let mut items = vec![
        (Control::Run, theme.tool(icons.run, text::CONTROL_RUN), idle),
        (
            Control::Script,
            theme.tool(icons.script, text::CONTROL_SCRIPT),
            idle,
        ),
        (
            Control::Stop,
            theme.tool(icons.stop, text::CONTROL_STOP),
            !idle,
        ),
    ];
    if bar.transactions {
        items.push((Control::Mode, theme.tool(icons.mode, mode), idle));
        items.push((
            Control::Commit,
            theme.tool(icons.commit, text::CONTROL_COMMIT),
            open,
        ));
        items.push((
            Control::Rollback,
            theme.tool(icons.rollback, text::CONTROL_ROLLBACK),
            open,
        ));
    }
    let label = match bar.namespace {
        Namespace::None => None,
        Namespace::Schema => Some(text::LABEL_SCHEMA),
        Namespace::Database => Some(text::LABEL_DATABASE),
    };
    if let Some(label) = label {
        let value = console
            .schema
            .as_deref()
            .or(bar.default_schema)
            .unwrap_or("-");
        let value = printable(value);
        let shown = if icons.schema.is_empty() {
            format!("{label}: [{value}]")
        } else {
            format!("{} {value}", icons.schema)
        };
        items.push((Control::Schema, shown, idle && !console.in_transaction()));
    }
    items
}

/// Draws buttons in a row. A button that has no effect now looks disabled.
/// It returns the first column and the column after the end of each button.
pub fn draw_buttons<T: Copy>(
    frame: &mut Frame,
    area: Rect,
    items: Vec<(T, String, bool)>,
    theme: &Theme,
) -> Vec<(u16, u16, T)> {
    let mut spans = vec![Span::raw(" ")];
    let mut buttons = Vec::new();
    let mut x = area.x + 1;
    for (control, label, enabled) in items {
        let width = u16::try_from(label.width()).unwrap_or(u16::MAX);
        if x.saturating_add(width) > area.right() {
            break;
        }
        buttons.push((x, x + width, control));
        let style = if enabled {
            theme.control
        } else {
            theme.control_off
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::raw("  "));
        x += width + BUTTON_GAP;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    buttons
}

/// Writes the label of a main tab. A tab with a text that no file holds has a mark.
pub fn main_tab_label(icon: &str, name: &str, unsaved: bool) -> String {
    let mut label = Theme::icon(icon);
    label.push_str(&printable(name));
    if unsaved {
        label.push_str(" !");
    }
    label
}

/// One main tab before the draw. The badge is a block of color at the start of the tab.
pub struct TabView {
    pub label: String,
    pub badge: Option<(String, Style)>,
}

/// Returns the badge of a tab: the mark and the time of a run in progress, or the end
/// of the last run. `frame` chooses the picture of the mark.
pub fn run_badge(
    phase: Phase,
    run: &RunClock,
    now: Instant,
    frame: usize,
    theme: &Theme,
) -> Option<(String, Style)> {
    let icons = &theme.icons;
    if phase != Phase::Idle {
        let mark = icons.running[frame % icons.running.len()];
        let shown = run.elapsed(now).map_or_else(
            || mark.to_owned(),
            |time| format!("{mark} {}", text::elapsed(time)),
        );
        return Some((format!(" {shown} "), theme.run_active));
    }
    let (mark, style) = match run.end? {
        RunEnd::Done => (icons.done, theme.run_done),
        RunEnd::Failed => (icons.failed, theme.run_failed),
        RunEnd::Stopped => (icons.stopped, theme.run_stopped),
    };
    Some((format!(" {mark} "), style))
}

/// The columns of one main tab on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MainTab {
    pub start: u16,
    pub end: u16,
    /// The first column that closes the tab. A tab that the row cuts has no close mark.
    pub close: Option<u16>,
    pub index: usize,
}

/// Draws the main tabs. It returns the columns of each tab, and it starts
/// the row at a later tab when the active tab does not fit.
pub fn draw_main_tabs(
    frame: &mut Frame,
    area: Rect,
    views: Vec<TabView>,
    active: usize,
    theme: &Theme,
) -> Vec<MainTab> {
    frame.buffer_mut().set_style(area, theme.tabline);
    // A tab is one column, the badge with a column after it, its label, one column,
    // the close mark, and one column.
    let width_of = |view: &TabView| {
        let badge = view.badge.as_ref().map_or(0, |(text, _)| text.width() + 1);
        u16::try_from(badge + view.label.width() + 4).unwrap_or(u16::MAX)
    };
    let widths: Vec<u16> = views.iter().map(width_of).collect();
    let mut first = 0;
    while first < active
        && widths[first..=active]
            .iter()
            .map(|width| u32::from(*width))
            .sum::<u32>()
            > u32::from(area.width)
    {
        first += 1;
    }
    let mut spans = Vec::new();
    let mut tabs = Vec::new();
    let mut x = area.x;
    for (index, view) in views.into_iter().enumerate().skip(first) {
        let end = x.saturating_add(widths[index]).min(area.right());
        // The close mark and the column after it close the tab, so a click has two columns.
        let close = x.saturating_add(widths[index].saturating_sub(2));
        tabs.push(MainTab {
            start: x,
            end,
            close: (close < end).then_some(close),
            index,
        });
        let style = if index == active {
            theme.tab_active
        } else {
            theme.tab
        };
        let mark = if index == active {
            Span::styled(theme.icons.tab_mark, theme.tab_mark)
        } else {
            Span::styled(" ", style)
        };
        spans.push(mark);
        if let Some((text, badge)) = view.badge {
            spans.push(Span::styled(text, badge));
            spans.push(Span::styled(" ", style));
        }
        spans.push(Span::styled(format!("{} ", view.label), style));
        spans.push(Span::styled(
            theme.icons.close,
            style.patch(theme.tab_close),
        ));
        spans.push(Span::styled(" ", style));
        x = end;
        if x >= area.right() {
            break;
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    tabs
}

/// Draws the row under the main tabs.
pub fn draw_rule(frame: &mut Frame, area: Rect, theme: &Theme) {
    frame.render_widget(
        Paragraph::new(RULE.repeat(usize::from(area.width))).style(theme.divider),
        area,
    );
}

pub fn draw_console(
    frame: &mut Frame,
    area: Rect,
    console: &mut Console,
    bar: &BarData,
    editor_ratio: u16,
    focused: bool,
    theme: &Theme,
) -> ConsoleLayout {
    const BAR_ROWS: u16 = 3;
    let title_rows = u16::from(bar.title);
    let shared = area.height.saturating_sub(BAR_ROWS + title_rows);
    let editor_rows = u16::try_from(u32::from(shared) * u32::from(editor_ratio) / 100).unwrap_or(0);
    let [title, controls, editor, tabs, results, summary] = Layout::vertical([
        Constraint::Length(title_rows),
        Constraint::Length(1),
        Constraint::Length(editor_rows.max(1)),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    let editor_focused = focused && console.focus == Focus::Editor;
    if bar.title {
        let chosen = console.schema.as_deref().or(bar.default_schema);
        let schema = chosen.filter(|_| bar.namespace != Namespace::None);
        let parts: Vec<&str> = [Some(bar.connection), schema, Some(console.name.as_str())]
            .into_iter()
            .flatten()
            .collect();
        let path = printable(&parts.join(" › "));
        draw_panel_title(frame, title, &path, editor_focused, theme);
    }
    let control_buttons = draw_buttons(frame, controls, control_items(console, bar, theme), theme);
    draw_editor(frame, editor, &mut console.editor, editor_focused, theme);
    if let Some(welcome) = bar
        .welcome
        .as_ref()
        .filter(|_| console.editor.search().is_none())
    {
        draw_welcome(frame, editor, welcome, theme);
    }
    let focused = focused && console.focus == Focus::Results;
    let active = console.active.min(console.results.len());
    draw_tabs(frame, tabs, &tab_labels(console), active, focused, theme);

    if console.showing_messages() {
        draw_messages(frame, results, console, theme);
    } else if let Some(result) = console.results.get_mut(console.active) {
        draw_grid(frame.buffer_mut(), results, result, focused, theme);
        let line = match &result.search {
            Some(search) => search_line(search, &result.table, theme),
            None => Line::from(Span::styled(
                format!(" {}", printable(&result.summary)),
                theme.hint,
            )),
        };
        frame.render_widget(Paragraph::new(line), summary);
    }
    let split = Rect {
        y: editor.y,
        height: shared,
        ..editor
    };
    ConsoleLayout {
        controls,
        control_buttons,
        editor,
        tabs,
        results,
        split,
    }
}

pub fn search_line(search: &RowSearch, table: &ResultTable, theme: &Theme) -> Line<'static> {
    let count = if search.query().is_empty() {
        String::new()
    } else {
        text::search_count(search.position(), search.total(), search.is_partial(table))
    };
    Line::from(vec![
        Span::raw(format!(
            " {}{}",
            text::FIND_LABEL,
            printable(search.query())
        )),
        Span::styled(" ", theme.cursor),
        Span::styled(format!(" {count}"), theme.hint),
    ])
}

fn tab_labels(console: &Console) -> Vec<String> {
    let mut labels: Vec<String> = console
        .results
        .iter()
        .map(|result| text::result_tab(result.number))
        .collect();
    labels.push(text::MESSAGES_TAB.to_owned());
    labels
}

/// Lists each tab with its text and its first column. The row of the tabs starts
/// with a rule, and each tab has one column on each side of its label.
fn tab_places(labels: &[String], area: Rect) -> Vec<(String, u16)> {
    let mut x = area.x.saturating_add(1);
    let mut places = Vec::new();
    for label in labels {
        let shown = format!(" {label} ");
        let width = u16::try_from(shown.width()).unwrap_or(u16::MAX);
        places.push((shown, x));
        x = x.saturating_add(width).saturating_add(1);
    }
    places
}

/// Draws a row of tabs: the results of a console, or the sections of a structure.
pub fn draw_tabs(
    frame: &mut Frame,
    area: Rect,
    labels: &[String],
    active: usize,
    focused: bool,
    theme: &Theme,
) {
    draw_rule(frame, area, theme);
    let active_style = if focused {
        theme.result_tab_active.patch(theme.panel_title_focused)
    } else {
        theme.result_tab_active
    };
    let buffer = frame.buffer_mut();
    for (index, (label, x)) in tab_places(labels, area).into_iter().enumerate() {
        if x >= area.right() {
            break;
        }
        let style = if index == active {
            active_style
        } else {
            theme.result_tab
        };
        buffer.set_stringn(x, area.y, label, usize::from(area.right() - x), style);
    }
}

/// Returns the tab at a screen column.
pub fn label_at(labels: &[String], area: Rect, column: u16) -> Option<usize> {
    tab_places(labels, area).iter().position(|(label, x)| {
        let end = usize::from(*x) + label.width();
        column >= *x && usize::from(column) < end
    })
}

/// Returns the result tab of a console at a screen column.
pub fn tab_at(console: &Console, area: Rect, column: u16) -> Option<usize> {
    label_at(&tab_labels(console), area, column)
}

fn draw_messages(frame: &mut Frame, area: Rect, console: &Console, theme: &Theme) {
    let area = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(1),
        ..area
    };
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

pub fn fit(text: &str, width: usize) -> (String, usize) {
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
        let width = u16::try_from(table.width(index))
            .unwrap_or(u16::MAX)
            .min(area.right() - x);
        columns.push((index, x, width));
        x = x.saturating_add(width).saturating_add(GAP);
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

/// Returns the area of the columns: the grid without the row numbers and the scrollbar.
fn grid_body(area: Rect, table: &ResultTable) -> Rect {
    let digits = u16::try_from(table.rows.len().max(1).to_string().len()).unwrap_or(u16::MAX);
    let numbers = digits.saturating_add(NUMBER_END).min(area.width);
    let width = area.width.saturating_sub(numbers).saturating_sub(1);
    Rect {
        x: area.x + numbers,
        width,
        ..area
    }
}

/// Writes a row number in the columns before `body`.
fn draw_row_number(
    buffer: &mut Buffer,
    area: Rect,
    body: Rect,
    y: u16,
    number: usize,
    style: Style,
) {
    let digits = usize::from((body.x - area.x).saturating_sub(NUMBER_END));
    buffer.set_stringn(area.x, y, format!("{number:>digits$}"), digits, style);
}

/// Writes the rule after the row numbers and the rule after each column.
fn draw_rules(
    buffer: &mut Buffer,
    body: Rect,
    columns: &[(usize, u16, u16)],
    y: u16,
    theme: &Theme,
) {
    let edges = columns
        .iter()
        .map(|(_, x, width)| x.saturating_add(*width).saturating_add(1));
    for x in std::iter::once(body.x.saturating_sub(2)).chain(edges) {
        if x < body.right() {
            buffer.set_stringn(x, y, "│", 1, theme.grid_rule);
        }
    }
}

pub fn draw_grid(
    buffer: &mut Buffer,
    area: Rect,
    result: &mut ResultTab,
    focused: bool,
    theme: &Theme,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let rows = usize::from(area.height - 1);
    let table = &result.table;
    let body = grid_body(area, table);
    result
        .view
        .keep_visible(rows, table, usize::from(body.width));
    let view = result.view;
    let columns = visible_columns(table, &view, body);

    buffer.set_style(Rect { height: 1, ..area }, theme.header);
    for (column, x, width) in &columns {
        let (name, _) = fit(&printable(&table.columns[*column]), usize::from(*width));
        buffer.set_stringn(*x, area.y, name, usize::from(*width), theme.header);
    }
    draw_rules(buffer, body, &columns, area.y, theme);

    let selection = view.selection();
    for (offset, row) in table.rows.iter().skip(view.top).take(rows).enumerate() {
        let y = area.y + 1 + offset as u16;
        let row_index = view.top + offset;
        let current = focused && row_index == view.row;
        if current {
            let line = Rect::new(area.x, y, area.width.saturating_sub(1), 1);
            buffer.set_style(line, theme.grid_line);
        }
        let number = if current {
            theme.gutter_current
        } else {
            theme.row_number
        };
        draw_row_number(buffer, area, body, y, row_index + 1, number);
        draw_rules(buffer, body, &columns, y, theme);
        for (column, x, width) in &columns {
            let Some(value) = row.get(*column) else {
                continue;
            };
            let selected = selection.is_some_and(|range| {
                (range.top..=range.bottom).contains(&row_index)
                    && (range.left..=range.right).contains(column)
            });
            let found = |check: fn(&RowSearch, usize, usize) -> bool| {
                result
                    .search
                    .as_ref()
                    .is_some_and(|search| check(search, row_index, *column))
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
    let track = Rect {
        y: area.y + 1,
        height: area.height - 1,
        ..area
    };
    draw_scrollbar(buffer, track, table.rows.len(), rows, view.top, theme);
}

/// Returns the column that ends at a screen cell of the header row.
pub fn column_edge_at(result: &ResultTab, area: Rect, column: u16, row: u16) -> Option<usize> {
    if row != area.y {
        return None;
    }
    visible_columns(&result.table, &result.view, grid_body(area, &result.table))
        .into_iter()
        .find(|(_, x, width)| column >= x + width && column < x + width + GAP)
        .map(|(index, _, _)| index)
}

/// Returns the column of a header cell at a screen cell.
pub fn header_column_at(result: &ResultTab, area: Rect, column: u16, row: u16) -> Option<usize> {
    if row != area.y {
        return None;
    }
    visible_columns(&result.table, &result.view, grid_body(area, &result.table))
        .into_iter()
        .find(|(_, x, width)| column >= *x && column < x + width)
        .map(|(index, _, _)| index)
}

/// Returns the row and the column at a screen cell.
pub fn grid_cell_at(
    result: &ResultTab,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<(usize, usize)> {
    if row <= area.y || row >= area.bottom() {
        return None;
    }
    let row_index = result.view.top + usize::from(row - area.y - 1);
    if row_index >= result.table.rows.len() {
        return None;
    }
    visible_columns(&result.table, &result.view, grid_body(area, &result.table))
        .into_iter()
        .find(|(_, x, width)| column >= *x && column <= x + width)
        .map(|(index, _, _)| (row_index, index))
}

/// The columns that stay free beside a modal that grows.
const MODAL_FREE_COLUMNS: u16 = 8;
/// The rows that stay free above and below a modal that grows.
pub(super) const MODAL_FREE_ROWS: u16 = 4;
const MODAL_WIDTH_LIMIT: u16 = 140;
const SHORT_MODAL_WIDTH_LIMIT: u16 = 100;
const LIST_ROWS_LIMIT: u16 = 30;

/// Returns a size that leaves `free` cells of the screen, from `base` to `limit`.
pub fn fitted(screen: u16, free: u16, base: u16, limit: u16) -> u16 {
    screen.saturating_sub(free).clamp(base, limit.max(base))
}

/// Returns the width of a modal. A long path needs the free columns of a wide screen.
pub fn modal_width(screen: u16, base: u16) -> u16 {
    fitted(screen, MODAL_FREE_COLUMNS, base, MODAL_WIDTH_LIMIT)
}

/// Returns the width of a modal with short rows, such as a list of names.
pub fn short_modal_width(screen: u16, base: u16) -> u16 {
    fitted(screen, MODAL_FREE_COLUMNS, base, SHORT_MODAL_WIDTH_LIMIT)
}

/// Returns the largest count of rows of a list. `chrome` is the other rows of its window.
pub fn list_rows(screen: u16, chrome: u16, base: u16) -> u16 {
    fitted(screen, MODAL_FREE_ROWS + chrome, base, LIST_ROWS_LIMIT)
}

pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

/// Draws a question. It returns the columns of each option and the row of the options.
pub fn draw_dialog(
    frame: &mut Frame,
    title: &str,
    body: &str,
    options: &[&str],
    focus: usize,
    theme: &Theme,
) -> (Vec<(u16, u16, usize)>, u16) {
    let area = frame.area();
    let width = fitted(area.width, MODAL_FREE_COLUMNS, 20, SHORT_MODAL_WIDTH_LIMIT);
    let inner_width = usize::from(width.saturating_sub(2)).max(1);
    let body_rows: usize = body
        .lines()
        .map(|line| line.width().div_ceil(inner_width).max(1))
        .sum();
    let box_area = centered(
        area,
        width,
        u16::try_from(body_rows).unwrap_or(10).saturating_add(4),
    );
    frame.render_widget(Clear, box_area);
    let block = float_block(title, theme);
    let inner = block.inner(box_area);
    frame.render_widget(block, box_area);

    let [message, buttons] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
    frame.render_widget(
        Paragraph::new(printable_lines(body)).wrap(Wrap { trim: false }),
        message,
    );

    let mut spans = Vec::new();
    let mut targets = Vec::new();
    let mut x = buttons.x;
    for (index, label) in options.iter().enumerate() {
        let style = if index == focus {
            theme.dialog_focus
        } else {
            theme.dialog_option
        };
        let text = theme.button(label);
        let label_width = u16::try_from(text.width()).unwrap_or(u16::MAX);
        targets.push((x, x.saturating_add(label_width), index));
        x = x.saturating_add(label_width).saturating_add(2);
        spans.push(Span::styled(text, style));
        spans.push(Span::raw("  "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), buttons);
    (targets, buttons.y)
}

/// Draws the list that selects a schema or a database. It returns its area and the
/// index of its first row.
pub fn draw_picker(
    frame: &mut Frame,
    title: &str,
    items: &[&str],
    cursor: usize,
    loading: bool,
    theme: &Theme,
) -> (Rect, usize) {
    let screen = frame.area();
    let limit = list_rows(screen.height, PICKER_CHROME_ROWS, PICKER_ROWS);
    let rows = u16::try_from(items.len()).unwrap_or(limit).min(limit);
    let box_area = centered(
        screen,
        short_modal_width(screen.width, PICKER_WIDTH),
        rows + PICKER_CHROME_ROWS,
    );
    frame.render_widget(Clear, box_area);
    let block = float_block(title, theme);
    let inner = block.inner(box_area);
    frame.render_widget(block, box_area);
    let height = usize::from(inner.height.saturating_sub(1)).max(1);
    let first = (cursor + 1).saturating_sub(height);
    let mut lines: Vec<Line> = items
        .iter()
        .enumerate()
        .skip(first)
        .take(height)
        .map(|(index, item)| {
            let style = if index == cursor {
                theme.dialog_focus
            } else {
                theme.dialog_option
            };
            Line::styled(printable(item), style)
        })
        .collect();
    if loading {
        lines.push(Line::styled(text::SCHEMA_LOADING, theme.hint));
    }
    frame.render_widget(Paragraph::new(lines), inner);
    (inner, first)
}

/// Keeps the line breaks of a text and replaces the other control characters.
fn printable_lines(text: &str) -> Text<'static> {
    Text::from(
        text.split('\n')
            .map(|line| Line::raw(printable(line)))
            .collect::<Vec<_>>(),
    )
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
    let box_area = centered(
        area,
        area.width
            .saturating_sub(MODAL_FREE_COLUMNS)
            .min(MODAL_WIDTH_LIMIT),
        area.height.saturating_sub(MODAL_FREE_ROWS),
    );
    frame.render_widget(Clear, box_area);
    let block = float_block(printable(title).trim(), theme)
        .title_bottom(Line::from(printable(footer)).alignment(Alignment::Right));
    let inner = block.inner(box_area);
    contents.set_width(usize::from(inner.width));
    let scroll = contents.last_row_at(scroll);
    let rows = (scroll..scroll + usize::from(inner.height))
        .map_while(|index| contents.row(index).map(|row| Line::raw(printable(row))))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(rows).block(block), box_area);
    scroll
}

pub fn draw_small_terminal(frame: &mut Frame, area: Rect, minimum: (u16, u16), theme: &Theme) {
    let lines = vec![
        Line::raw(format!(
            "{}: {} x {}",
            text::SMALL_TERMINAL,
            area.width,
            area.height
        )),
        Line::raw(format!(
            "{}: {} x {}",
            text::MINIMUM_SIZE,
            minimum.0,
            minimum.1
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(float_block("SQLN", theme)),
        area,
    );
}
