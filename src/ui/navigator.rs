//! The navigator panel.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::theme::Theme;
use super::text;
use crate::connection::Connection;
use crate::grid::printable;
use crate::navigator::{Navigator, RowKind, RowNote};

const MARK_OPEN: &str = "▾";
const MARK_CLOSED: &str = "▸";
const MARK_CONNECTED: &str = "●";

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
            Line::styled(text::NO_CONNECTIONS_ROW, theme.tree_detail),
            Line::styled(text::ADD_CONNECTION_ROW, theme.tree_detail),
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
        let mut spans = vec![Span::raw(format!("{indent}{mark} {}", printable(row.name(connections))))];
        let detail = theme.tree_detail;
        if row.kind == RowKind::Connection && connected[row.connection] {
            spans.push(Span::styled(format!(" {MARK_CONNECTED}"), theme.mark_good));
        }
        if row.kind == RowKind::View {
            spans.push(Span::styled(format!(" {}", text::MARK_VIEW), detail));
        }
        match navigator.suffix(row, connections) {
            Some(RowNote::Loading) => spans.push(Span::styled(format!(" {}", text::NOTE_LOADING), detail)),
            Some(RowNote::Failed) => spans.push(Span::styled(format!(" {}", text::NOTE_FAILED), theme.error)),
            None => {}
        }
        let style = if offset == cursor && focused { theme.tree_selected } else { Style::new() };
        lines.push(Line::from(spans).style(style));
    }
    frame.render_widget(Paragraph::new(lines), area);
}
