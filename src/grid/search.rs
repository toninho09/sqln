//! Search in the loaded rows. It never queries the database.
//!
//! The main thread does this work in short slices, so that a large result does not
//! stop the keys.

use std::time::Instant;

use super::ResultTable;
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

    /// Reads at most one row or `chars` characters.
    fn scan_step(&mut self, table: &ResultTable, mut chars: usize) {
        let row = &table.rows[self.next_row];
        while self.next_col < row.len() {
            if let Some((c, next)) = row[self.next_col].search_char(self.offset) {
                chars -= 1;
                if let Some(found) = self.matcher.push(c) {
                    self.offset = next;
                    if found {
                        self.matches.push((self.next_row, self.next_col));
                        self.next_cell();
                    }
                }
                if chars == 0 {
                    return;
                }
            } else {
                self.next_cell();
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
