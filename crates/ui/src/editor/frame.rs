//! One frame of the editor. EdTUI tests each character against each highlight and reads
//! each row to its end, so the time of a full screen grows with the square of its size.
//! Here EdTUI gets no highlights and rows that stop at the end of the window, and the
//! styles go on its cells after the draw.

use edtui::{EditorState, Index2, RowIndex};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{TAB_WIDTH, buffer};

/// The part of the text that one frame shows.
pub(super) struct Window {
    pub top: usize,
    pub left: usize,
    /// The first column after the window on each row, from the row `top`.
    pub ends: Vec<usize>,
}

impl Window {
    /// Lists each row with the first column after the window.
    pub fn rows(&self) -> impl Iterator<Item = (usize, usize)> {
        (self.top..).zip(self.ends.iter().copied())
    }
}

/// Returns the column after the characters that `width` cells show from `left`. Only a
/// plain character counts, because the cells of another one depend on its neighbors.
pub(super) fn window_end(line: &[char], left: usize, width: usize) -> usize {
    let mut used = 0;
    let rest = line.get(left..).unwrap_or_default();
    let shown = rest.iter().take_while(|c| {
        used += usize::from(c.is_ascii_graphic() || **c == ' ');
        used <= width
    });
    (left + shown.count()).min(line.len())
}

/// The style of each column of a window. The first style on a column stays.
pub(super) struct Styles {
    top: usize,
    left: usize,
    rows: Vec<Vec<Option<Style>>>,
}

impl Styles {
    pub fn new(window: &Window) -> Self {
        let columns = |end: &usize| vec![None; end.saturating_sub(window.left)];
        Self {
            top: window.top,
            left: window.left,
            rows: window.ends.iter().map(columns).collect(),
        }
    }

    /// Marks the columns from `start` to `end`. The column `end` is part of the range.
    pub fn mark(&mut self, start: Index2, end: Index2, style: Style) {
        let (top, left) = (self.top, self.left);
        let rows = start.row.max(top)..(end.row + 1).min(top + self.rows.len());
        for row in rows {
            let columns = &mut self.rows[row - top];
            let first = if row == start.row { start.col } else { 0 };
            let stop = if row == end.row {
                (end.col + 1).saturating_sub(left).min(columns.len())
            } else {
                columns.len()
            };
            let marked = columns.get_mut(first.saturating_sub(left)..stop);
            for column in marked.unwrap_or_default() {
                column.get_or_insert(style);
            }
        }
    }
}

/// The state of one draw. Each row stops at the end of the window until this value drops.
pub struct FrameState<'a> {
    pub(super) state: &'a mut EditorState,
    /// The complete rows, with their numbers.
    pub(super) rows: Vec<(usize, Vec<char>)>,
    pub(super) styles: Styles,
}

impl FrameState<'_> {
    pub fn state(&mut self) -> &mut EditorState {
        self.state
    }

    /// Writes the styles on the cells that EdTUI drew in `area`. A selection stays over
    /// these styles, and the cursor stays over all of them.
    pub fn paint(&self, area: Rect, cells: &mut Buffer, cursor: Style) {
        let rows = self.styles.rows.iter().enumerate();
        for (offset, columns) in rows.take(usize::from(area.height)) {
            if columns.iter().any(Option::is_some) {
                let row = Rect {
                    y: area.y + offset as u16,
                    height: 1,
                    ..area
                };
                self.paint_row(self.styles.top + offset, columns, row, cells);
            }
        }
        let place = self.state.cursor_screen_position();
        if let Some(cell) = place.and_then(|place| cells.cell_mut(place)) {
            cell.set_style(cursor);
        }
    }

    /// Follows the rule of the draw of a row: each group of characters that the user sees
    /// as one has its own width, a tab has `TAB_WIDTH` cells, and a control character has
    /// no cell.
    fn paint_row(&self, row: usize, columns: &[Option<Style>], area: Rect, cells: &mut Buffer) {
        let left = self.styles.left;
        let line = buffer::row(&self.state.lines, row);
        let text: String = line.get(left..).unwrap_or_default().iter().collect();
        let selection = self.state.selection.as_ref();
        let (mut x, mut column) = (area.x, left);
        for group in text.graphemes(true) {
            let width = match group {
                "\t" => TAB_WIDTH,
                group if group.chars().any(char::is_control) => 0,
                group => group.width(),
            };
            let next = x.saturating_add(width as u16);
            if next > area.right() {
                break;
            }
            let count = group.chars().count();
            let shown = column - left..(column - left + count).min(columns.len());
            let style = columns
                .get(shown)
                .unwrap_or_default()
                .iter()
                .flatten()
                .next();
            let place = Index2::new(row, column);
            if let Some(style) = style
                && width > 0
                && !selection.is_some_and(|selection| selection.contains(&place))
            {
                for x in x..next {
                    cells[(x, area.y)].set_style(*style);
                }
            }
            x = next;
            column += count;
        }
    }
}

/// A panic also comes here, because the last save of a panic reads these rows.
impl Drop for FrameState<'_> {
    fn drop(&mut self) {
        for (row, line) in self.rows.drain(..) {
            if let Some(cut) = self.state.lines.get_mut(RowIndex::new(row)) {
                *cut = line;
            }
        }
    }
}
