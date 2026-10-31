//! The rows of a text in a window. It reads a row when the view needs it, because a
//! value can hold millions of characters and the window shows only a few of them.

use unicode_width::UnicodeWidthChar;

use crate::grid::printable_char;

#[derive(Debug)]
pub struct WrappedText {
    text: String,
    width: usize,
    /// The byte range of each row that the view read.
    rows: Vec<(usize, usize)>,
    /// The start of the next row. `None` means the end of the text.
    next: Option<usize>,
}

/// Returns the end of the row that starts at `start`, and the start of the next row.
/// A row breaks after a space when it can, so that a word stays in one piece.
fn row_end(text: &str, start: usize, width: usize) -> (usize, Option<usize>) {
    let mut used = 0;
    let mut after_space = None;
    for (offset, c) in text[start..].char_indices() {
        let at = start + offset;
        if c == '\n' {
            return (at, Some(at + 1));
        }
        let char_width = printable_char(c).width().unwrap_or(0);
        if used + char_width > width && at > start {
            let end = after_space.unwrap_or(at);
            return (end, Some(end));
        }
        used += char_width;
        if c == ' ' {
            after_space = Some(at + 1);
        }
    }
    (text.len(), None)
}

impl WrappedText {
    pub fn new(text: String) -> Self {
        Self {
            text,
            width: 1,
            rows: Vec::new(),
            next: Some(0),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Sets the columns of a row. The rows of another width stop being valid.
    pub fn set_width(&mut self, width: usize) {
        let width = width.max(1);
        if width != self.width {
            self.width = width;
            self.rows.clear();
            self.next = Some(0);
        }
    }

    /// Returns the text of a row, or `None` after the last row.
    pub fn row(&mut self, index: usize) -> Option<&str> {
        while self.rows.len() <= index {
            let start = self.next?;
            let (end, next) = row_end(&self.text, start, self.width);
            self.rows.push((start, end));
            self.next = next;
        }
        let (start, end) = self.rows[index];
        Some(&self.text[start..end])
    }

    /// Returns the last row at or before `index`.
    pub fn last_row_at(&mut self, index: usize) -> usize {
        if self.row(index).is_some() {
            index
        } else {
            self.rows.len().saturating_sub(1)
        }
    }
}
