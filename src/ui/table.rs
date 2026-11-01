//! The table view: the title, the filter bar, the grid, and the status lines.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use super::console::message_style;
use super::console::{
    BUTTON_GAP, draw_buttons, draw_grid, draw_panel_title, draw_tabs, label_at, search_line,
};
use super::theme::Theme;
use super::{draw_suggestions, input_spans, text};
use crate::console::Phase;
use crate::exec::ReadLimit;
use crate::grid::printable;
use crate::line_input::LineInput;
use crate::structure::{Scope, Section};
use crate::table::{TableFocus, TableTab};

const LABEL_WIDTH: u16 = 10;
const SUMMARY_GAP: &str = " · ";

/// The buttons of the pages, with the text and the state of each one.
type Pager = Vec<(TableControl, String, bool)>;

/// A button of the filter bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableControl {
    Apply,
    Clear,
    LoadMore,
    Stop,
    PreviousPage,
    /// The place of the page, between the two buttons. A click on it does nothing.
    PageLabel,
    NextPage,
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
    /// The row of the sections of a structure. It has no height in other tabs.
    pub sections: Rect,
    pub grid: Rect,
    /// The row under the grid, with the buttons of the pages.
    pub summary: Rect,
    pub pager: Vec<(u16, u16, TableControl)>,
}

pub struct TableBar<'a> {
    pub connection: &'a str,
    /// True when the data budget refuses a new page.
    pub blocked: bool,
    /// True on a tall screen, which has a title row.
    pub title: bool,
}

fn section_labels() -> Vec<String> {
    Section::ALL
        .map(|section| {
            match section {
                Section::Columns => text::SECTION_COLUMNS,
                Section::Indexes => text::SECTION_INDEXES,
                Section::ForeignKeys => text::SECTION_FOREIGN_KEYS,
            }
            .to_owned()
        })
        .to_vec()
}

/// Returns the section at a screen column.
pub fn section_at(area: Rect, column: u16) -> Option<Section> {
    label_at(&section_labels(), area, column).map(|index| Section::ALL[index])
}

/// Writes the label of a table tab.
pub fn tab_label(tab: &TableTab, theme: &Theme) -> String {
    let icon = if tab.is_view {
        theme.icons.view
    } else {
        theme.icons.table
    };
    super::main_tab_label(icon, &tab.label(), false)
}

fn control_items(
    tab: &TableTab,
    blocked: bool,
    theme: &Theme,
) -> Vec<(TableControl, String, bool)> {
    let idle = !tab.is_busy();
    let icons = &theme.icons;
    let apply = if tab.filters {
        text::BUTTON_APPLY
    } else {
        text::BUTTON_RELOAD
    };
    let mut items = vec![(TableControl::Apply, theme.tool(icons.apply, apply), idle)];
    if tab.filters {
        items.push((
            TableControl::Clear,
            theme.tool(icons.clear, text::BUTTON_CLEAR),
            idle,
        ));
    }
    items.extend([
        (
            TableControl::LoadMore,
            theme.tool(icons.load_more, text::BUTTON_LOAD_MORE),
            idle && tab.has_more && !blocked,
        ),
        (
            TableControl::Stop,
            theme.tool(icons.stop, text::CONTROL_STOP),
            !idle,
        ),
    ]);
    items
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
    (
        Rect {
            x: field.x + 1,
            width: field.width.saturating_sub(2),
            ..field
        },
        first,
    )
}

/// Lists the buttons of the pages, with words or in the short form.
fn pager_items(tab: &TableTab, blocked: bool, wide: bool, theme: &Theme) -> Pager {
    let page = tab.page();
    let pages = tab.loaded_pages();
    let can_load = tab.has_more && !blocked && !tab.is_busy();
    let icons = &theme.icons;
    let short = |icon: &str, mark: &str| theme.button(if icon.is_empty() { mark } else { icon });
    let (previous, label, next) = if wide {
        (
            theme.tool(icons.previous_page, text::BUTTON_PREVIOUS),
            text::table_page(page + 1, pages, tab.has_more),
            theme.tool(icons.next_page, text::BUTTON_NEXT),
        )
    } else {
        (
            short(icons.previous_page, "<"),
            text::table_page_short(page + 1, pages, tab.has_more),
            short(icons.next_page, ">"),
        )
    };
    vec![
        (TableControl::PreviousPage, previous, page > 0),
        (TableControl::PageLabel, label, false),
        (TableControl::NextPage, next, page + 1 < pages || can_load),
    ]
}

fn pager_width(pager: &Pager) -> usize {
    pager
        .iter()
        .map(|(_, label, _)| label.width() + usize::from(BUTTON_GAP))
        .sum()
}

/// Chooses the parts of the row under the grid. The state of the load always stays, the
/// buttons come next, and the note about more rows goes first. A long state leaves no
/// place for the buttons, and the keys still work.
fn summary_row(tab: &TableTab, blocked: bool, width: usize, theme: &Theme) -> (String, Pager) {
    let (state, note) = summary_parts(tab);
    let full = note.map_or_else(
        || state.clone(),
        |note| format!("{state}{SUMMARY_GAP}{note}"),
    );
    if !tab.has_pages() {
        return (full, Vec::new());
    }
    let wide = pager_items(tab, blocked, true, theme);
    let short = pager_items(tab, blocked, false, theme);
    for (text, pager) in [
        (&full, &wide),
        (&full, &short),
        (&state, &wide),
        (&state, &short),
    ] {
        if 1 + text.width() + 1 + pager_width(pager) <= width {
            return (text.clone(), pager.clone());
        }
    }
    (full, Vec::new())
}

/// Returns the state of the loaded rows, and a note that can leave the row.
fn summary_parts(tab: &TableTab) -> (String, Option<&'static str>) {
    let rows = tab.total.map_or_else(
        || text::rows_loaded(tab.loaded()),
        |total| text::rows_of_total(tab.loaded(), total),
    );
    let mut parts = vec![rows];
    let mut note = None;
    match tab.phase {
        Phase::Waiting => parts.push(text::TABLE_WAITING.to_owned()),
        Phase::Running if tab.is_counting() => parts.push(text::TABLE_COUNTING.to_owned()),
        Phase::Running => parts.push(text::TABLE_LOADING.to_owned()),
        Phase::Stopping => parts.push(text::TABLE_STOPPING.to_owned()),
        Phase::Idle => match tab.limit {
            ReadLimit::Budget => parts.push(text::TABLE_BUDGET.to_owned()),
            ReadLimit::ValueTooLarge => parts.push(text::TABLE_VALUE_TOO_LARGE.to_owned()),
            _ if tab.has_more => note = Some(text::TABLE_MORE),
            _ if tab.message.is_none() => note = Some(text::TABLE_NO_MORE_ROWS),
            _ => {}
        },
    }
    (parts.join(SUMMARY_GAP), note)
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
    let [
        title,
        filter,
        order,
        controls,
        sections,
        grid,
        summary,
        notice,
    ] = Layout::vertical([
        Constraint::Length(u16::from(bar.title)),
        Constraint::Length(u16::from(tab.filters)),
        Constraint::Length(u16::from(tab.filters)),
        Constraint::Length(1),
        Constraint::Length(u16::from(tab.section().is_some())),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    if bar.title {
        let heading = match &tab.structure {
            Some(structure) if structure.scope == Scope::Connection => {
                text::structure_title(bar.connection, text::ALL_SCHEMAS)
            }
            Some(structure) => text::structure_title(bar.connection, &structure.name),
            None => text::table_title(
                bar.connection,
                &tab.relation.schema,
                &tab.relation.relation,
                tab.kind(),
            ),
        };
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
    let buttons = draw_buttons(
        frame,
        controls,
        control_items(tab, bar.blocked, theme),
        theme,
    );

    if let Some(section) = tab.section() {
        let labels = section_labels();
        draw_tabs(frame, sections, &labels, section.index(), focused, theme);
    }

    let message_in_grid = tab.result.table.columns.is_empty() && tab.message.is_some();
    if tab.result.table.columns.is_empty() {
        draw_empty(frame, grid, tab, theme);
    } else {
        let grid_focused = focused && tab.focus == TableFocus::Grid;
        draw_grid(
            frame.buffer_mut(),
            grid,
            &mut tab.result,
            grid_focused,
            theme,
        );
    }

    let (state_text, pager_items) =
        summary_row(tab, bar.blocked, usize::from(summary.width), theme);
    let pager_columns = u16::try_from(pager_width(&pager_items)).unwrap_or(0);
    let searching = tab.result.search.is_some();
    let [state, buttons_area] = Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(if searching { 0 } else { pager_columns }),
    ])
    .areas(summary);
    let line = match &tab.result.search {
        Some(search) => search_line(search, &tab.result.table, theme),
        None => Line::styled(format!(" {state_text}"), theme.hint),
    };
    frame.render_widget(Paragraph::new(line), state);
    let pager = draw_buttons(frame, buttons_area, pager_items, theme);
    let note = match &tab.message {
        Some(message) if !message_in_grid => Some(Line::styled(
            format!(" {}", printable(&message.text)),
            message_style(message, theme),
        )),
        _ if tab.filters && tab.applied_order().is_empty() => Some(Line::styled(
            format!(" {}", text::TABLE_NO_ORDER),
            theme.hint,
        )),
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
        let start = input.text()[..list.completion.replace.start.min(input.text().len())]
            .chars()
            .count();
        let offset = u16::try_from(start.saturating_sub(first)).unwrap_or(0);
        let anchor = Position::new(field.x.saturating_add(offset).min(field.right()), field.y);
        draw_suggestions(frame, area, anchor, list, theme);
    }
    TableLayout {
        filter,
        order,
        controls,
        buttons,
        sections,
        grid,
        summary,
        pager,
    }
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
