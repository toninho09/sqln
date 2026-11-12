//! Search in the loaded rows. It never queries the database.
//!
//! The main thread does this work in short slices, so that a large result does not
//! stop the keys.

use std::time::Instant;

use super::{ResultTable, Value};
use crate::find::Matcher;

const CHARS_PER_STEP: usize = 512;

#[cfg(test)]
mod tests;

/// The list stops here, so that a short text does not fill the memory.
const MAX_MATCHES: usize = 1_000_000;

#[derive(Debug)]
pub struct RowSearch {
    query: String,
    matcher: Matcher,
    /// The folded query in bytes, when the query holds only ASCII characters. A search
    /// of text cells then reads bytes instead of characters.
    ascii: Vec<u8>,
    /// The cells that match, ordered by row and column.
    matches: Vec<(usize, usize)>,
    next_row: usize,
    next_col: usize,
    offset: usize,
    /// The first result is the first match at or after this cell.
    origin: (usize, usize),
    current: Option<usize>,
    capped: bool,
}

impl RowSearch {
    pub fn new(origin: (usize, usize)) -> Self {
        Self {
            query: String::new(),
            matcher: Matcher::default(),
            ascii: Vec::new(),
            matches: Vec::new(),
            next_row: 0,
            next_col: 0,
            offset: 0,
            origin,
            current: None,
            capped: false,
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn push_str(&mut self, text: &str) {
        self.query.push_str(text);
        self.restart();
    }

    pub fn pop(&mut self) {
        self.query.pop();
        self.restart();
    }

    fn restart(&mut self) {
        self.matcher = Matcher::new(&self.query);
        self.ascii = if self.query.is_ascii() {
            self.query.as_bytes().to_ascii_lowercase()
        } else {
            Vec::new()
        };
        self.next_col = 0;
        self.offset = 0;
        self.matches.clear();
        self.next_row = 0;
        self.current = None;
        self.capped = false;
    }

    /// Returns true when loaded rows wait for a read.
    pub fn has_work(&self, table: &ResultTable) -> bool {
        !self.matcher.is_empty() && !self.capped && self.next_row < table.rows.len()
    }

    /// Reads rows until the deadline. It returns the first result when it finds it.
    pub fn scan(&mut self, table: &ResultTable, deadline: Instant) -> Option<(usize, usize)> {
        while self.has_work(table) {
            self.scan_step(table, CHARS_PER_STEP);
            if Instant::now() >= deadline {
                break;
            }
        }
        if self.current.is_some() {
            return None;
        }
        let at_origin = self.matches.partition_point(|cell| *cell < self.origin);
        if at_origin < self.matches.len() {
            self.current = Some(at_origin);
        } else if !self.has_work(table) && !self.matches.is_empty() {
            self.current = Some(0);
        }
        self.current.map(|index| self.matches[index])
    }

    /// Reads at most one row or `budget` characters. A text cell with an ASCII query
    /// reads bytes, and one byte counts as one character of the budget.
    fn scan_step(&mut self, table: &ResultTable, mut budget: usize) {
        let mut cells = table.rows[self.next_row].cells().skip(self.next_col);
        let mut cell = cells.next();
        while let Some(value) = cell {
            if let Some(text) = fast_text(value, &self.ascii) {
                let offset = self.offset;
                let (reached, found) = scan_ascii(text, &self.ascii, offset, budget);
                budget = budget.saturating_sub(reached - offset);
                if found {
                    self.matches.push((self.next_row, self.next_col));
                    self.next_cell();
                    cell = cells.next();
                } else if reached < text.len() {
                    self.offset = reached;
                    return;
                } else {
                    self.next_cell();
                    cell = cells.next();
                }
                if budget == 0 {
                    return;
                }
                continue;
            }
            if let Some((c, next)) = value.search_char(self.offset) {
                budget -= 1;
                if let Some(found) = self.matcher.push(c) {
                    self.offset = next;
                    if found {
                        self.matches.push((self.next_row, self.next_col));
                        self.next_cell();
                        cell = cells.next();
                    }
                }
                if budget == 0 {
                    return;
                }
            } else {
                self.next_cell();
                cell = cells.next();
            }
        }
        self.next_row += 1;
        self.next_col = 0;
        self.capped = self.matches.len() >= MAX_MATCHES;
    }

    fn next_cell(&mut self) {
        self.next_col += 1;
        self.offset = 0;
        self.matcher.reset();
    }

    /// Follows the rows after the first `removed` rows of the table go away.
    pub fn drop_front(&mut self, removed: usize) {
        if removed == 0 {
            return;
        }
        let gone = self.matches.partition_point(|(row, _)| *row < removed);
        self.matches.drain(..gone);
        for (row, _) in &mut self.matches {
            *row -= removed;
        }
        self.current = self.current.and_then(|index| index.checked_sub(gone));
        self.origin.0 = self.origin.0.saturating_sub(removed);
        if self.next_row < removed {
            self.next_row = 0;
            self.next_col = 0;
            self.offset = 0;
            self.matcher.reset();
        } else {
            self.next_row -= removed;
        }
        self.capped = self.matches.len() >= MAX_MATCHES;
    }

    /// Moves to the next or the previous result, and it returns the cell.
    pub fn advance(&mut self, forward: bool) -> Option<(usize, usize)> {
        let count = self.matches.len();
        let current = self.current?;
        let next = (current + if forward { 1 } else { count - 1 }) % count;
        self.current = Some(next);
        Some(self.matches[next])
    }

    pub fn is_match(&self, row: usize, col: usize) -> bool {
        self.matches.binary_search(&(row, col)).is_ok()
    }

    pub fn is_current(&self, row: usize, col: usize) -> bool {
        self.current
            .is_some_and(|index| self.matches[index] == (row, col))
    }

    /// Returns the position of the current result, from 1. The value 0 means no result yet.
    pub fn position(&self) -> usize {
        self.current.map_or(0, |index| index + 1)
    }

    pub fn total(&self) -> usize {
        self.matches.len()
    }

    /// Returns true when the total can grow.
    pub fn is_partial(&self, table: &ResultTable) -> bool {
        self.capped || self.has_work(table)
    }
}

/// Returns the text of a value when the ASCII byte search can read it.
fn fast_text<'a>(value: Value<'a>, ascii: &[u8]) -> Option<&'a str> {
    if ascii.is_empty() {
        return None;
    }
    match value {
        Value::Text(text) | Value::Number(text) => Some(text),
        _ => None,
    }
}

/// Returns true when both bytes fold to the same lowercase ASCII character.
#[inline]
fn same_fold(byte: u8, expected: u8) -> bool {
    byte.to_ascii_lowercase() == expected
}

/// Searches `text` from `offset` for an ASCII needle folded to lowercase. The search
/// reads at most `budget` bytes past `offset`. It returns the offset reached and true
/// when the text holds the needle from that point. A new step reads again before
/// `offset`, so a match across the edge of a step cannot escape.
fn scan_ascii(text: &str, needle: &[u8], offset: usize, budget: usize) -> (usize, bool) {
    let bytes = text.as_bytes();
    let start = offset.saturating_sub(needle.len() - 1);
    let limit = offset.saturating_add(budget).min(bytes.len());
    let mut at = start;
    while at < limit {
        let Some(hit) = bytes[at..limit]
            .iter()
            .position(|&byte| same_fold(byte, needle[0]))
            .map(|skip| at + skip)
        else {
            break;
        };
        let whole = needle.len() <= limit - hit
            && needle
                .iter()
                .enumerate()
                .all(|(index, &expected)| same_fold(bytes[hit + index], expected));
        if whole {
            return (limit, true);
        }
        at = hit + 1;
    }
    (limit, false)
}
