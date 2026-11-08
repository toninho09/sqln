//! The rows of a text in a window. It reads a row when the view needs it, because a
//! value can hold millions of characters and the window shows only a few of them.

use std::sync::Arc;

use unicode_width::UnicodeWidthChar;

use crate::grid::printable_char;

/// The source of the rows of a window. A large binary value stays as bytes, and each
/// row takes the digits of its own bytes, so opening it costs the rows on screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    Text(String),
    /// Bytes that show as `0x` and hexadecimal digits.
    Hex(Arc<[u8]>),
}

impl Content {
    /// Returns the complete text. A large binary value costs its digits here, so only
    /// the copy of the window reads it.
    pub fn to_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Hex(bytes) => hex_range(bytes, 0..2 + bytes.len() * 2),
        }
    }

    /// Returns the text of `range`, in bytes of the complete text.
    fn slice(&self, range: std::ops::Range<usize>) -> String {
        match self {
            Self::Text(text) => text.get(range).map_or_else(String::new, str::to_owned),
            Self::Hex(bytes) => hex_range(bytes, range),
        }
    }

    /// Returns the end of the row that starts at `start`, and the start of the next row.
    fn row_end(&self, start: usize, width: usize) -> (usize, Option<(usize, bool)>) {
        match self {
            Self::Text(text) => row_end(text, start, width),
            Self::Hex(bytes) => {
                let len = 2 + bytes.len() * 2;
                let end = start.saturating_add(width).min(len);
                (end, (end < len).then_some((end, true)))
            }
        }
    }

    /// Returns the first `count` characters at `start`.
    fn head(&self, start: usize, count: usize) -> String {
        match self {
            Self::Text(text) => text
                .get(start..)
                .unwrap_or_default()
                .chars()
                .take(count)
                .collect(),
            Self::Hex(_) => self.slice(start..start.saturating_add(count)),
        }
    }
}

const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

/// Returns the digits of `range` of the text of `bytes`. A range that starts or ends
/// between the two digits of a byte keeps the digits that the range holds.
fn hex_range(bytes: &[u8], range: std::ops::Range<usize>) -> String {
    let end = range.end.min(2 + bytes.len() * 2);
    let mut out = String::with_capacity(end.saturating_sub(range.start));
    for place in range.start..end {
        out.push(match place {
            0 => '0',
            1 => 'x',
            _ => {
                let byte = bytes[(place - 2) / 2];
                let digit = if (place - 2).is_multiple_of(2) {
                    byte >> 4
                } else {
                    byte & 15
                };
                char::from(HEX_DIGITS[usize::from(digit)])
            }
        });
    }
    out
}

#[derive(Debug)]
pub struct WrappedText {
    source: Content,
    width: usize,
    /// The columns before a row that continues a line of the text.
    indent: usize,
    /// The byte range of each row that the view read, and true for a row that continues
    /// a line of the text.
    rows: Vec<(usize, usize, bool)>,
    /// The text of each row that the view read. A row of a binary value is made here.
    texts: Vec<String>,
    /// The start of the next row, and true when it continues a line. `None` means the
    /// end of the text.
    next: Option<(usize, bool)>,
}

/// Returns the end of the row that starts at `start`, and the start of the next row.
/// A row breaks after a space when it can, so that a word stays in one piece.
pub(crate) fn row_end(text: &str, start: usize, width: usize) -> (usize, Option<(usize, bool)>) {
    let mut used = 0;
    let mut after_space = None;
    for (offset, c) in text[start..].char_indices() {
        let at = start + offset;
        if c == '\n' {
            return (at, Some((at + 1, false)));
        }
        let char_width = printable_char(c).width().unwrap_or(0);
        if used + char_width > width && at > start {
            let end = after_space.unwrap_or(at);
            return (end, Some((end, true)));
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
        Self::from_source(Content::Text(text))
    }

    pub fn with_indent(text: String, indent: usize) -> Self {
        Self::from_source_with_indent(Content::Text(text), indent)
    }

    pub fn from_source(source: Content) -> Self {
        Self::from_source_with_indent(source, 0)
    }

    /// Keeps `indent` columns free before a row that continues a line.
    pub fn from_source_with_indent(source: Content, indent: usize) -> Self {
        Self {
            source,
            width: 1,
            indent,
            rows: Vec::new(),
            texts: Vec::new(),
            next: Some((0, false)),
        }
    }

    /// Returns the complete text. A large binary value costs its digits here.
    pub fn to_text(&self) -> String {
        self.source.to_text()
    }

    /// Sets the columns of a row. The rows of another width stop being valid.
    pub fn set_width(&mut self, width: usize) {
        let width = width.max(1);
        if width != self.width {
            self.width = width;
            self.rows.clear();
            self.texts.clear();
            self.next = Some((0, false));
        }
    }

    /// Returns the text of a row, or `None` after the last row.
    pub fn row(&mut self, index: usize) -> Option<&str> {
        while self.rows.len() <= index {
            let (start, continues) = self.next?;
            let free = if continues { self.hanging() } else { 0 };
            let (end, next) = self.source.row_end(start, self.width - free);
            let text = self.source.slice(start..end);
            self.rows.push((start, end, continues));
            self.texts.push(text);
            self.next = next;
        }
        Some(&self.texts[index])
    }

    /// Returns the indent of a row, in a window with the place for it. A narrow window
    /// gives its columns to the text.
    fn hanging(&self) -> usize {
        if self.indent * 2 <= self.width {
            self.indent
        } else {
            0
        }
    }

    /// Returns the columns before a row that `row` read.
    pub fn row_indent(&self, index: usize) -> usize {
        match self.rows.get(index) {
            Some((_, _, true)) => self.hanging(),
            _ => 0,
        }
    }

    /// Returns the first `count` characters of the text line that holds a row that
    /// `row` read.
    pub fn line_head(&self, index: usize, count: usize) -> String {
        let mut first = index.min(self.rows.len().saturating_sub(1));
        while first > 0 && self.rows.get(first).is_some_and(|row| row.2) {
            first -= 1;
        }
        self.rows
            .get(first)
            .map_or_else(String::new, |row| self.source.head(row.0, count))
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
