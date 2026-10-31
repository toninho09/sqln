//! The navigator panel.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::draw_scrollbar;
use super::text;
use super::theme::Theme;
use crate::connection::Connection;
use crate::grid::printable;
use crate::navigator::{Navigator, RowKind, RowNote};

const MARK_OPEN: &str = "▾";
const MARK_CLOSED: &str = "▸";
/// The empty column at the start of each row.
pub const TREE_PAD: u16 = 1;

pub fn draw_navigator(
    frame: &mut Frame,
    area: Rect,
    navigator: &mut Navigator,
    connections: &[Connection],
    connected: &[bool],
    focused: bool,
    theme: &Theme,
) {
    let rows = navigator.rows(connections);
    if rows.is_empty() {
        let lines = vec![
            Line::styled(format!(" {}", text::NO_CONNECTIONS_ROW), theme.tree_detail),
            Line::styled(format!(" {}", text::ADD_CONNECTION_ROW), theme.tree_detail),
        ];
        frame.render_widget(Paragraph::new(lines), area);
        return;
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
            RowKind::Connection => spans.push(Span::styled(Theme::icon(icons.disconnected), detail)),
            RowKind::Schema => spans.push(Span::styled(Theme::icon(icons.schema), detail)),
            RowKind::Table => spans.push(Span::styled(Theme::icon(icons.table), detail)),
            RowKind::View => spans.push(Span::styled(Theme::icon(icons.view), detail)),
            RowKind::Column => spans.push(Span::styled(Theme::icon(icons.column), detail)),
        }
        spans.push(Span::raw(printable(row.name(connections))));
        if row.kind == RowKind::Connection {
            let engine = icons.engine(connections[row.connection].dialect);
            spans.push(Span::styled(format!("  {engine}"), detail));
        }
        if row.kind == RowKind::View && icons.view.is_empty() {
            spans.push(Span::styled(format!(" {}", text::MARK_VIEW), detail));
        }
        match navigator.suffix(row, connections) {
            Some(RowNote::Loading) => spans.push(Span::styled(format!(" {}", text::NOTE_LOADING), detail)),
            Some(RowNote::Failed) => spans.push(Span::styled(format!(" {}", text::NOTE_FAILED), theme.error)),
            None => {}
        }
        if offset == cursor {
            let y = area.y + u16::try_from(offset - navigator.top).unwrap_or(0);
            let style = if focused { theme.tree_selected } else { theme.tree_cursor };
            frame.buffer_mut().set_style(Rect { y, height: 1, ..area }, style);
        }
        lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(lines), area);
    draw_scrollbar(frame.buffer_mut(), area, rows.len(), height, navigator.top, theme);
}
