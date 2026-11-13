//! The rows of a text in a window. It reads a row when the view needs it, because a
//! value can hold millions of characters and the window shows only a few of them.

mod search;
pub use search::TextSearch;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;

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

    fn search_char(&self, at: usize) -> Option<(char, usize)> {
        match self {
            Self::Text(text) => text
                .get(at..)?
                .chars()
                .next()
                .map(|c| (c, at + c.len_utf8())),
            Self::Hex(bytes) if at < 2 + bytes.len() * 2 => {
                Some((hex_character(bytes, at), at + 1))
            }
            _ => None,
        }
    }

    fn adjacent(&self, at: usize, forward: bool) -> usize {
        match self {
            Self::Text(text) if forward => {
                text[at..].chars().next().map_or(at, |c| at + c.len_utf8())
            }
            Self::Text(text) => text[..at]
                .char_indices()
                .next_back()
                .map_or(0, |(byte, _)| byte),
            Self::Hex(bytes) if forward => (at + 1).min(2 + bytes.len() * 2),
            Self::Hex(_) => at.saturating_sub(1),
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

fn hex_character(bytes: &[u8], place: usize) -> char {
    match place {
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
    }
}

/// Returns the digits of `range` of the text of `bytes`. A range that starts or ends
/// between the two digits of a byte keeps the digits that the range holds.
fn hex_range(bytes: &[u8], range: std::ops::Range<usize>) -> String {
    let end = range.end.min(2 + bytes.len() * 2);
    let mut out = String::with_capacity(end.saturating_sub(range.start));
    for place in range.start..end {
        out.push(hex_character(bytes, place));
    }
    out
}

#[derive(Debug)]
pub struct WrappedText {
    source: Content,
    cursor: usize,
    anchor: usize,
    cursor_char: usize,
    anchor_char: usize,
    character_starts: Vec<usize>,
    next_character: usize,
    search: Option<TextSearch>,
    search_target: Option<usize>,
    json: Option<crate::grid::json::Highlights>,
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
            cursor: 0,
            anchor: 0,
            cursor_char: 0,
            anchor_char: 0,
            character_starts: Vec::new(),
            next_character: 0,
            search: None,
            search_target: None,
            json: None,
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

    /// Returns the selected original text, or the complete text without a selection.
    pub fn copy_text(&self) -> String {
        if self.cursor == self.anchor {
            self.to_text()
        } else {
            self.source.slice(self.selection())
        }
    }

    pub fn selected_character_count(&self) -> usize {
        self.cursor_char.abs_diff(self.anchor_char)
    }

    fn selection(&self) -> Range<usize> {
        self.anchor.min(self.cursor)..self.anchor.max(self.cursor)
    }

    /// Returns the selected bytes inside a row that `row` read.
    pub fn row_selection(&self, index: usize) -> Range<usize> {
        let (start, end, _) = self.rows[index];
        let selected = self.selection();
        selected.start.clamp(start, end) - start..selected.end.clamp(start, end) - start
    }

    /// Places the cursor at a display column. `extend` keeps the selection anchor.
    pub fn select_at(&mut self, row: usize, column: usize, extend: bool) {
        let row = self.last_row_at(row);
        let column = column.saturating_sub(self.row_indent(row));
        let mut width = 0;
        let mut byte = self.rows[row].0;
        let mut characters = 0;
        for c in self.texts[row].chars() {
            let next = width + printable_char(c).width().unwrap_or(0);
            if next > column {
                break;
            }
            width = next;
            byte += c.len_utf8();
            characters += 1;
        }
        self.cursor = byte;
        self.cursor_char = self.character_starts[row] + characters;
        if !extend {
            self.anchor = byte;
            self.anchor_char = self.cursor_char;
        }
    }

    /// Returns the row and display column of the cursor.
    pub fn cursor_position(&mut self) -> (usize, usize) {
        while self.next.is_some_and(|(start, _)| start <= self.cursor) {
            self.row(self.rows.len());
        }
        let index = self
            .rows
            .partition_point(|row| row.0 <= self.cursor)
            .saturating_sub(1);
        let start = self.rows[index].0;
        let end = self.cursor.min(self.rows[index].1);
        let width: usize = self
            .source
            .slice(start..end)
            .chars()
            .map(|c| printable_char(c).width().unwrap_or(0))
            .sum();
        (index, self.row_indent(index) + width)
    }

    /// Extends the selection by one character or one displayed row.
    pub fn extend_selection(&mut self, horizontal: isize, vertical: isize) -> usize {
        if horizontal != 0 {
            let next = self.source.adjacent(self.cursor, horizontal > 0);
            if next != self.cursor {
                self.cursor_char = self.cursor_char.saturating_add_signed(horizontal.signum());
                self.cursor = next;
            }
        }
        let (row, column) = self.cursor_position();
        if vertical != 0 {
            self.select_at(row.saturating_add_signed(vertical), column, true);
        }
        self.cursor_position().0
    }

    /// Adds JSON colors after the cell window validates the source.
    pub(crate) fn highlight_json(&mut self) {
        if let Content::Text(text) = &self.source {
            self.json = Some(crate::grid::json::Highlights::validated(text));
        }
    }

    /// Returns JSON token ranges relative to a row that `row` read.
    pub fn row_json_tokens(&self, index: usize) -> Vec<(Range<usize>, crate::grid::json::Kind)> {
        let (start, end, _) = self.rows[index];
        self.json.as_ref().map_or_else(Vec::new, |json| {
            json.in_range(start..end)
                .map(|(range, kind)| (range.start - start..range.end - start, kind))
                .collect()
        })
    }

    pub fn search(&self) -> Option<&TextSearch> {
        self.search.as_ref()
    }

    pub fn open_search(&mut self) {
        if self.search.is_none() {
            let mut search = TextSearch::default();
            if (1..=256).contains(&self.selected_character_count()) {
                let selected = self.copy_text();
                if !selected.contains('\n') {
                    search.input.set(&selected);
                }
            }
            search.restart();
            self.search = Some(search);
        }
    }

    pub fn paste_search(&mut self, text: &str) {
        if let Some(search) = &mut self.search {
            search.input.insert(text);
            search.restart();
            self.search_target = None;
        }
    }

    pub fn on_search_key(&mut self, key: KeyEvent) -> bool {
        let Some(search) = &mut self.search else {
            return false;
        };
        match key.code {
            KeyCode::Esc => {
                self.search = None;
                self.search_target = None;
            }
            KeyCode::Enter | KeyCode::Down | KeyCode::Up => {
                let forward =
                    key.code != KeyCode::Up && !key.modifiers.contains(KeyModifiers::SHIFT);
                self.search_target = search.advance(forward).map(|range| range.start);
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return false,
            _ => {
                let before = search.input.text().to_owned();
                search.input.on_key(key);
                if before != search.input.text() {
                    search.restart();
                    self.search_target = None;
                }
            }
        }
        true
    }

    pub fn search_has_work(&self) -> bool {
        self.search_target.is_some() || self.search.as_ref().is_some_and(TextSearch::has_work)
    }

    pub fn continue_search(&mut self, deadline: Instant) -> Option<usize> {
        if self.search_target.is_none()
            && let Some(search) = &mut self.search
            && let Some(byte) = search.scan(&self.source, deadline)
        {
            self.search_target = Some(byte);
        }
        let byte = self.search_target?;
        while self.next.is_some_and(|(start, _)| start <= byte) {
            if Instant::now() >= deadline {
                return None;
            }
            self.row(self.rows.len());
        }
        self.search_target = None;
        Some(
            self.rows
                .partition_point(|row| row.0 <= byte)
                .saturating_sub(1),
        )
    }

    pub fn row_search_ranges(&self, index: usize) -> Vec<(Range<usize>, bool)> {
        let Some(search) = &self.search else {
            return Vec::new();
        };
        let (start, end, _) = self.rows[index];
        let first = search.matches.partition_point(|range| range.end <= start);
        search.matches[first..]
            .iter()
            .enumerate()
            .take_while(|(_, range)| range.start < end)
            .map(|(index, range)| {
                (
                    (range.start.max(start) - start)..(range.end.min(end) - start),
                    first + index == search.current,
                )
            })
            .collect()
    }

    /// Sets the columns of a row. The rows of another width stop being valid.
    pub fn set_width(&mut self, width: usize) {
        let width = width.max(1);
        if width != self.width {
            self.width = width;
            self.rows.clear();
            self.texts.clear();
            self.character_starts.clear();
            self.next_character = 0;
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
            self.character_starts.push(self.next_character);
            self.next_character += text.chars().count() + next.map_or(0, |(start, _)| start - end);
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
