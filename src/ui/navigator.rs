//! The navigator panel.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::draw_scrollbar;
use super::icons::{Icons, connection_symbol};
use super::palette::draw_prompt_field;
use super::text;
use super::theme::Theme;
use crate::connection::Connection;
use crate::grid::printable;
use crate::navigator::{Navigator, RowKind, RowNote};

const MARK_OPEN: &str = "▾";
const MARK_CLOSED: &str = "▸";
/// The empty column at the start of each row.
pub const TREE_PAD: u16 = 1;

/// Returns the marks before and after the name of a connection. An icon of the connection
/// takes the first place, and the symbol of the engine stays after the name.
fn connection_marks(icons: &Icons, connection: &Connection) -> (&'static str, &'static str) {
    let engine = icons.engine(connection.dialect);
    let icon = connection.definition.as_ref().and_then(|item| item.icon);
    match icon {
        Some(icon) => (connection_symbol(icon), engine),
        None if icons.logos => (engine, ""),
        None => (icons.database, engine),
    }
}

/// Draws the field of the filter on the first row, and returns the row and the tree.
fn draw_filter(
    frame: &mut Frame,
    area: Rect,
    navigator: &Navigator,
    focused: bool,
    theme: &Theme,
) -> (Rect, Rect) {
    let Some(input) = navigator.filter_input() else {
        return (Rect::default(), area);
    };
    let [field, tree] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    let editing = focused && navigator.filter_is_editing();
    draw_prompt_field(frame, input, field, editing, theme);
    (field, tree)
}

/// Draws the panel, and returns the row of the filter and the rows of the tree.
pub fn draw_navigator(
    frame: &mut Frame,
    area: Rect,
    navigator: &mut Navigator,
    connections: &[Connection],
    connected: &[bool],
    focused: bool,
    theme: &Theme,
) -> (Rect, Rect) {
    let (field, area) = draw_filter(frame, area, navigator, focused, theme);
    let rows = navigator.rows(connections);
    if rows.is_empty() {
        let notes = if navigator.is_filtered() {
            [text::NO_TREE_MATCH, text::LOADED_CATALOGS_ONLY]
        } else {
            [text::NO_CONNECTIONS_ROW, text::ADD_CONNECTION_ROW]
        };
        let lines = notes.map(|note| Line::styled(format!(" {note}"), theme.tree_detail));
        frame.render_widget(Paragraph::new(lines.to_vec()), area);
        return (field, area);
    }
    let height = usize::from(area.height);
    let cursor = navigator.cursor_index(&rows, connections);
    if cursor < navigator.top {
        navigator.top = cursor;
    } else if cursor >= navigator.top + height {
        navigator.top = cursor + 1 - height;
    }
    navigator.top = navigator.top.min(rows.len().saturating_sub(height));

    let icons = &theme.icons;
    let detail = theme.tree_detail;
    let mut lines = Vec::with_capacity(height);
    for (offset, row) in rows.iter().enumerate().skip(navigator.top).take(height) {
        let indent = "  ".repeat(row.depth());
        let mark = if !Navigator::is_branch(row) {
            " "
        } else if navigator.is_open(row, connections) {
            MARK_OPEN
        } else {
            MARK_CLOSED
        };
        let mut spans = vec![Span::raw(format!(" {indent}{mark} "))];
        match row.kind {
            RowKind::Connection if connected[row.connection] => {
                spans.push(Span::styled(Theme::icon(icons.connected), theme.mark_good));
            }
            RowKind::Connection => {
                spans.push(Span::styled(Theme::icon(icons.disconnected), detail))
            }
            RowKind::Consoles | RowKind::Schema => {
                spans.push(Span::styled(Theme::icon(icons.schema), detail));
            }
            RowKind::Console => spans.push(Span::styled(Theme::icon(icons.console), detail)),
            RowKind::Table => spans.push(Span::styled(Theme::icon(icons.table), detail)),
            RowKind::View => spans.push(Span::styled(Theme::icon(icons.view), detail)),
            RowKind::Column => spans.push(Span::styled(Theme::icon(icons.column), detail)),
            RowKind::Group(_) => spans.push(Span::styled(Theme::icon(icons.schema), detail)),
            RowKind::Object(group) => {
                spans.push(Span::styled(Theme::icon(icons.object(group)), detail));
            }
        }
        let (before_name, after_name) = if row.kind == RowKind::Connection {
            connection_marks(icons, &connections[row.connection])
        } else {
            ("", "")
        };
        spans.push(Span::styled(Theme::icon(before_name), detail));
        spans.push(Span::raw(printable(row.name(connections))));
        if !after_name.is_empty() {
            spans.push(Span::styled(format!("  {after_name}"), detail));
        }
        if row.kind == RowKind::View && icons.view.is_empty() {
            spans.push(Span::styled(format!(" {}", text::MARK_VIEW), detail));
        }
        match navigator.suffix(row, connections) {
            Some(RowNote::Loading) => {
                spans.push(Span::styled(format!(" {}", text::NOTE_LOADING), detail))
            }
            Some(RowNote::Failed) => {
                spans.push(Span::styled(format!(" {}", text::NOTE_FAILED), theme.error))
            }
            Some(RowNote::Partial) => spans.push(Span::styled(
                format!(" {}", text::NOTE_PARTIAL_KEYS),
                detail,
            )),
            Some(RowNote::Open) => spans.push(Span::styled(text::MARK_OPEN, detail)),
            Some(RowNote::Filtered) => {
                spans.push(Span::styled(format!(" {}", text::NOTE_FILTERED), detail))
            }
            Some(RowNote::Count(count)) => {
                spans.push(Span::styled(format!(" ({count})"), detail));
            }
            None => {}
        }
        if offset == cursor {
            let y = area.y + u16::try_from(offset - navigator.top).unwrap_or(0);
            let style = if focused {
                theme.tree_selected
            } else {
                theme.tree_cursor
            };
            frame.buffer_mut().set_style(
                Rect {
                    y,
                    height: 1,
                    ..area
                },
                style,
            );
        }
        lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(lines), area);
    draw_scrollbar(
        frame.buffer_mut(),
        area,
        rows.len(),
        height,
        navigator.top,
        theme,
    );
    (field, area)
}
