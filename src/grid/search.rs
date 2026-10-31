//! Search in the loaded rows. It never queries the database.
//!
//! The main thread does this work in short slices, so that a large result does not
//! stop the keys.

use std::time::Instant;

use super::ResultTable;
use crate::find::needle;

/// The list stops here, so that a short text does not fill the memory.
const MAX_MATCHES: usize = 1_000_000;

#[derive(Debug)]
pub struct RowSearch {
    query: String,
    needle: Vec<char>,
    /// The cells that match, ordered by row and column.
    matches: Vec<(usize, usize)>,
    next_row: usize,
    /// The first result is the first match at or after this cell.
    origin: (usize, usize),
    current: Option<usize>,
    capped: bool,
}

impl RowSearch {
    pub fn new(origin: (usize, usize)) -> Self {
        Self {
            query: String::new(),
            needle: Vec::new(),
            matches: Vec::new(),
            next_row: 0,
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
        self.needle = needle(&self.query);
        self.matches.clear();
        self.next_row = 0;
        self.current = None;
        self.capped = false;
    }

    /// Returns true when loaded rows wait for a read.
    pub fn has_work(&self, table: &ResultTable) -> bool {
        !self.needle.is_empty() && !self.capped && self.next_row < table.rows.len()
    }

    /// Reads rows until the deadline. It returns the first result when it finds it.
    pub fn scan(&mut self, table: &ResultTable, deadline: Instant) -> Option<(usize, usize)> {
        while self.has_work(table) {
            for (col, value) in table.rows[self.next_row].iter().enumerate() {
                if value.matches(&self.needle) {
                    self.matches.push((self.next_row, col));
                }
            }
            self.next_row += 1;
            if self.matches.len() >= MAX_MATCHES {
                self.capped = true;
            }
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
