//! The rows of the window of the query history.

use crate::filter_list::FilterList;
use crate::grid::printable_start;

/// A statement can hold 64 KiB, and a row of the list shows far less than this width.
const LINE_WIDTH: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryRow {
    pub ok: bool,
    /// The time since the run, in words.
    pub age: String,
    /// The statement in one row, for the list.
    pub line: String,
    /// The text that the editor receives.
    pub sql: String,
}

impl HistoryRow {
    pub fn new(ok: bool, age: String, sql: String) -> Self {
        let words = sql.split_whitespace().collect::<Vec<_>>().join(" ");
        let line = printable_start(&words, LINE_WIDTH);
        Self { ok, age, line, sql }
    }
}

pub type HistoryList = FilterList<HistoryRow>;

/// Returns the list of a window, in the order of `rows`.
pub fn history_list(rows: Vec<HistoryRow>) -> HistoryList {
    FilterList::new(rows, |row| row.sql.clone())
}
