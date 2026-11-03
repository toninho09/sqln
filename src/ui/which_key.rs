//! The menu of `Ctrl+W`: the second keys of the prefix, in groups.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::float_block;
use super::theme::Theme;

const COLUMNS: u16 = 3;
const MAX_WIDTH: u16 = 78;
const KEY_WIDTH: usize = 4;

/// One group of the menu. A key that the configuration removes has no row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyGroup {
    pub title: &'static str,
    pub keys: Vec<(String, &'static str)>,
}

fn group_lines<'a>(group: &'a KeyGroup, theme: &Theme) -> Vec<Line<'a>> {
    let mut lines = vec![Line::styled(group.title, theme.panel_title_focused)];
    for (key, label) in &group.keys {
        let key = format!("{key:<KEY_WIDTH$}");
        lines.push(Line::from(vec![
            Span::styled(key, theme.key),
            Span::raw(" "),
            Span::raw(*label),
        ]));
    }
    lines
}

/// Divides the groups into columns, with the fewest rows that hold them all.
fn columns_of<'a>(groups: &'a [KeyGroup], theme: &Theme) -> Vec<Vec<Line<'a>>> {
    let shown = || groups.iter().filter(|group| !group.keys.is_empty());
    let total: usize = shown().map(|group| group.keys.len() + 2).sum();
    let target = total.div_ceil(usize::from(COLUMNS));
    let mut columns: Vec<Vec<Line>> = vec![Vec::new()];
    for group in shown() {
        let lines = group_lines(group, theme);
        let full = columns
            .last()
            .is_some_and(|column| column.len() + lines.len() > target);
        if full && columns.len() < usize::from(COLUMNS) {
            columns.push(Vec::new());
        }
        if let Some(column) = columns.last_mut() {
            if !column.is_empty() {
                column.push(Line::raw(""));
            }
            column.extend(lines);
        }
    }
    columns
}

/// Draws a page of the menu at the bottom of `bounds`.
pub fn draw_which_key(
    frame: &mut Frame,
    bounds: Rect,
    title: &str,
    groups: &[KeyGroup],
    theme: &Theme,
) {
    let columns = columns_of(groups, theme);
    let rows = columns.iter().map(Vec::len).max().unwrap_or(0);
    let width = MAX_WIDTH.min(bounds.width);
    let height = u16::try_from(rows)
        .unwrap_or(u16::MAX)
        .saturating_add(2)
        .min(bounds.height);
    let x = bounds.x + (bounds.width - width) / 2;
    let area = Rect::new(x, bounds.bottom() - height, width, height);
    frame.render_widget(Clear, area);
    let block = float_block(title, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let column_width = inner.width / COLUMNS;
    for (index, lines) in (0u16..).zip(columns) {
        let place = Rect {
            x: inner.x + index * column_width + 1,
            width: column_width.saturating_sub(1),
            ..inner
        };
        frame.render_widget(Paragraph::new(lines), place);
    }
}

/// Returns the width of the widest row, for the checks of the minimum terminal.
pub fn widest_row(groups: &[KeyGroup]) -> usize {
    let row = |(key, label): &(String, &str)| key.width().max(KEY_WIDTH) + 1 + label.width();
    groups
        .iter()
        .flat_map(|group| group.keys.iter().map(row))
        .max()
        .unwrap_or(0)
}
