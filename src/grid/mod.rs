//! Result data: values, widths, cursor, selection, budget, and TSV copy.

mod budget;
pub mod json;
mod search;
mod tsv;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub use self::budget::{Budget, DEFAULT_LIMIT, Reservation, row_cost};
pub use self::search::RowSearch;
pub use self::tsv::{delimited_cell, delimited_text, hex};
use crate::find;

/// The limit of a width that the application calculates.
const MAX_COLUMN_WIDTH: usize = 40;
const MIN_COLUMN_WIDTH: usize = 4;
/// The columns between two columns of a grid: a space, a rule, and a space.
pub const COLUMN_GAP: usize = 3;
/// The limit of a width that the user sets.
const MAX_MANUAL_WIDTH: usize = 200;

/// A value keeps the text of the database. The variant only guides the presentation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Null,
    Text(Box<str>),
    Number(Box<str>),
    Binary(Box<[u8]>),
}

pub type Row = Box<[Value]>;

pub fn printable_char(c: char) -> char {
    match c {
        '\n' => '↵',
        '\r' => '␍',
        '\t' => '→',
        c if c.is_control() => '�',
        c => c,
    }
}

/// Replaces characters that the terminal must not interpret.
pub fn printable(text: &str) -> String {
    text.chars().map(printable_char).collect()
}

/// Works as `printable`, and it stops at the characters that fill `width` columns.
fn printable_start(text: &str, width: usize) -> String {
    let mut used = 0;
    text.chars()
        .map(printable_char)
        .take_while(|c| {
            used += c.width().unwrap_or(0);
            used <= width
        })
        .collect()
}

impl Value {
    /// Returns the text of a grid cell. It stays on one line.
    pub fn display(&self) -> String {
        match self {
            Value::Null => "NULL".to_owned(),
            Value::Text(text) | Value::Number(text) => printable(text),
            Value::Binary(bytes) => match std::str::from_utf8(bytes) {
                Ok(text) if !text.contains(char::is_control) => text.to_owned(),
                _ => hex(bytes),
            },
        }
    }

    /// Returns the start of `display`, so a large value does not cost each frame.
    pub fn preview(&self, width: usize) -> String {
        match self {
            Value::Null => "NULL".to_owned(),
            Value::Text(text) | Value::Number(text) => printable_start(text, width),
            Value::Binary(bytes) => match std::str::from_utf8(bytes) {
                Ok(text) if !text.contains(char::is_control) => printable_start(text, width),
                _ => hex(&bytes[..bytes.len().min(width / 2)]),
            },
        }
    }

    fn matches(&self, needle: &[char]) -> bool {
        match self {
            Value::Null => find::contains("NULL", needle),
            Value::Text(text) | Value::Number(text) => find::contains(text, needle),
            Value::Binary(_) => find::contains(&self.display(), needle),
        }
    }

    /// Returns the complete text of the detail window.
    pub fn full_text(&self) -> String {
        match self {
            Value::Null => "NULL".to_owned(),
            Value::Text(text) | Value::Number(text) => text.to_string(),
            Value::Binary(_) => self.display(),
        }
    }
}

#[derive(Debug, Default)]
pub struct ResultTable {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    pub bytes: usize,
    /// The width of each column for the loaded data.
    natural: Vec<usize>,
    /// The widths that the user sets. New rows do not change them.
    manual: Vec<Option<usize>>,
}

impl ResultTable {
    pub fn new(columns: Vec<String>) -> Self {
        let natural: Vec<usize> = columns
            .iter()
            .map(|name| {
                printable(name)
                    .width()
                    .clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH)
            })
            .collect();
        let manual = vec![None; columns.len()];
        Self {
            columns,
            rows: Vec::new(),
            bytes: 0,
            natural,
            manual,
        }
    }

    pub fn append(&mut self, rows: Vec<Row>, bytes: usize) {
        for row in &rows {
            for (width, value) in self.natural.iter_mut().zip(row.iter()) {
                *width = (*width).max(cell_width(value)).min(MAX_COLUMN_WIDTH);
            }
        }
        self.rows.extend(rows);
        self.bytes += bytes;
    }

    pub fn width(&self, column: usize) -> usize {
        self.manual[column].unwrap_or(self.natural[column])
    }

    pub fn set_width(&mut self, column: usize, width: usize) {
        if let Some(manual) = self.manual.get_mut(column) {
            *manual = Some(width.clamp(MIN_COLUMN_WIDTH, MAX_MANUAL_WIDTH));
        }
    }

    pub fn resize(&mut self, column: usize, change: isize) {
        if column < self.manual.len() {
            self.set_width(column, self.width(column).saturating_add_signed(change));
        }
    }

    /// Returns the column to the width of the loaded data.
    pub fn reset_width(&mut self, column: usize) {
        if let Some(manual) = self.manual.get_mut(column) {
            *manual = None;
        }
    }

    pub fn tsv(&self, top: usize, bottom: usize, left: usize, right: usize) -> String {
        let cells: Vec<Vec<Value>> = self.rows[top..=bottom]
            .iter()
            .map(|row| row[left..=right].to_vec())
            .collect();
        tsv::encode(cells.iter().map(Vec::as_slice))
    }
}

/// Stops at the column limit, so a long value does not cost a full scan.
fn cell_width(value: &Value) -> usize {
    let text = match value {
        Value::Null => return 4,
        Value::Text(text) | Value::Number(text) => text,
        Value::Binary(bytes) => return bytes.len().saturating_mul(2).saturating_add(2),
    };
    let mut width = 0;
    for c in text.chars() {
        width += c.width().unwrap_or(1);
        if width >= MAX_COLUMN_WIDTH {
            break;
        }
    }
    width
}

/// The cursor, the selection, and the scroll position of one result.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridView {
    pub row: usize,
    pub col: usize,
    pub anchor: Option<(usize, usize)>,
    pub top: usize,
    pub left: usize,
}

/// A selected range. The limits are inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub top: usize,
    pub bottom: usize,
    pub left: usize,
    pub right: usize,
}

impl GridView {
    pub fn selection(&self) -> Option<Range> {
        let (row, col) = self.anchor?;
        Some(Range {
            top: row.min(self.row),
            bottom: row.max(self.row),
            left: col.min(self.col),
            right: col.max(self.col),
        })
    }

    /// Returns the selection or, without one, the cell at the cursor.
    pub fn target(&self) -> Range {
        self.selection().unwrap_or(Range {
            top: self.row,
            bottom: self.row,
            left: self.col,
            right: self.col,
        })
    }

    pub fn move_to(&mut self, row: usize, col: usize, table: &ResultTable, extend: bool) {
        if table.rows.is_empty() {
            return;
        }
        if extend {
            self.anchor.get_or_insert((self.row, self.col));
        } else {
            self.anchor = None;
        }
        self.row = row.min(table.rows.len() - 1);
        self.col = col.min(table.columns.len().saturating_sub(1));
    }

    pub fn move_by(&mut self, rows: isize, cols: isize, table: &ResultTable, extend: bool) {
        let row = self.row.saturating_add_signed(rows);
        let col = self.col.saturating_add_signed(cols);
        self.move_to(row, col, table, extend);
    }

    pub fn select_all(&mut self, table: &ResultTable) {
        if !table.rows.is_empty() {
            self.anchor = Some((0, 0));
            self.row = table.rows.len() - 1;
            self.col = table.columns.len().saturating_sub(1);
        }
    }

    pub fn toggle_selection(&mut self) {
        self.anchor = if self.anchor.is_some() {
            None
        } else {
            Some((self.row, self.col))
        };
    }

    pub fn copy(&self, table: &ResultTable) -> Option<String> {
        if table.rows.is_empty() {
            return None;
        }
        let range = self.target();
        Some(table.tsv(range.top, range.bottom, range.left, range.right))
    }

    /// Moves the scroll position so that the cursor stays visible.
    pub fn keep_visible(&mut self, rows: usize, table: &ResultTable, width: usize) {
        if self.row < self.top {
            self.top = self.row;
        } else if rows > 0 && self.row >= self.top + rows {
            self.top = self.row + 1 - rows;
        }
        if self.col < self.left {
            self.left = self.col;
        }
        while self.left < self.col {
            let used: usize = (self.left..=self.col)
                .map(|column| table.width(column) + COLUMN_GAP)
                .sum();
            if used <= width {
                break;
            }
            self.left += 1;
        }
    }
}
