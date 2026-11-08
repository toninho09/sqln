//! A result grid with its cursor and its search. A console result and a table share it.

use std::sync::Arc;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::exec::ReadLimit;
use crate::grid::json::pretty;
use crate::grid::{Budget, GridView, Reservation, ResultTable, RowSearch, Value, printable};
use crate::input::Action;
use crate::plan::PlanView;
use crate::ui::kit::measure::fit;
use crate::ui::text;
use crate::ui::wrap::Content;

/// The longest name of a column in the row window.
const ROW_NAME_WIDTH: usize = 32;
/// The longest value in the row window. `Enter` on the grid opens a whole value, and a
/// limit here keeps a key fast with a very large value.
const ROW_VALUE_WIDTH: usize = 4096;
/// The text between a name and its value.
const ROW_GAP: &str = " : ";

pub struct ResultTab {
    /// The number of the tab. It stays the same when the user closes another tab.
    pub number: usize,
    pub table: ResultTable,
    pub edits: crate::data_edit::Edits,
    pub view: GridView,
    pub summary: String,
    pub search: Option<RowSearch>,
    /// The position of the statement of this result in its execution.
    pub statement: usize,
    /// What stopped the read at a limit.
    pub limit: ReadLimit,
    /// True when the execution ended before the last row: a stop, an error, or the time.
    pub cut_short: bool,
    /// True when the grid shows the tree of a plan.
    pub plan: bool,
}

/// The action of a key in the results panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GridAction {
    None,
    Copy(String),
    OpenValue {
        title: String,
        text: ValueText,
        footer: String,
        /// The formatted form of a JSON value, with its footer.
        json: Option<(String, String)>,
    },
    /// The window of one row of the grid.
    OpenRow(usize),
    CloseResult,
}

/// The text of the value window. A binary value that is not text stays as bytes, so the
/// window reads only the rows on screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueText {
    Text(String),
    Binary(Arc<[u8]>),
}

impl ValueText {
    /// Returns the number of characters of the complete text.
    fn chars(&self) -> usize {
        match self {
            Self::Text(text) => text.chars().count(),
            Self::Binary(bytes) => 2 + bytes.len() * 2,
        }
    }

    pub fn source(self) -> Content {
        match self {
            Self::Text(text) => Content::Text(text),
            Self::Binary(bytes) => Content::Hex(bytes),
        }
    }
}

/// The text of the window of one row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowWindow {
    pub title: String,
    pub text: String,
    pub footer: String,
    /// The columns before the values, for a value that takes more than one row.
    pub indent: usize,
}

impl ResultTab {
    pub fn new(number: usize, columns: Vec<String>) -> Self {
        Self {
            number,
            table: ResultTable::new(columns),
            edits: crate::data_edit::Edits::default(),
            view: GridView::default(),
            summary: String::new(),
            search: None,
            statement: 0,
            limit: ReadLimit::None,
            cut_short: false,
            plan: false,
        }
    }

    /// Replaces the text of the database with the tree of a plan. The new rows take a
    /// place in the budget first, so a full budget keeps the text.
    pub fn show_plan(&mut self, view: PlanView, budget: &Budget) -> bool {
        let mut bytes = 0;
        for row in &view.rows {
            match budget.reserve(row) {
                Reservation::Accepted(cost) => bytes += cost,
                Reservation::BudgetReached | Reservation::ValueTooLarge => {
                    budget.release(bytes);
                    return false;
                }
            }
        }
        budget.release(self.table.bytes);
        self.table = ResultTable::new(view.columns);
        self.table.append(view.rows, bytes);
        self.table.set_width(0, view.tree_width);
        self.view = GridView::default();
        self.summary = view.summary;
        self.plan = true;
        true
    }

    /// Returns true when the grid holds each row of the result.
    pub fn is_complete(&self) -> bool {
        self.limit == ReadLimit::None && !self.cut_short
    }

    /// Returns the text of the window of a row: one line for each column.
    pub fn row_window(&self, row: usize) -> Option<RowWindow> {
        let values = self.table.rows.get(row)?;
        let names: Vec<(String, usize)> = self
            .table
            .columns
            .iter()
            .map(|name| fit(&printable(name), ROW_NAME_WIDTH))
            .collect();
        let width = names.iter().map(|(_, used)| *used).max().unwrap_or(0);
        let lines: Vec<String> = names
            .iter()
            .zip(values)
            .map(|((name, used), value)| {
                let free = width - used;
                format!("{name}{:free$}{ROW_GAP}{}", "", row_value(value))
            })
            .collect();
        Some(RowWindow {
            title: text::row_title(row, self.table.rows.len()),
            footer: text::row_footer(lines.len()),
            text: lines.join("\n"),
            indent: width + ROW_GAP.len(),
        })
    }

    /// Returns true when the search has rows to read.
    pub fn search_has_work(&self) -> bool {
        self.search
            .as_ref()
            .is_some_and(|search| search.has_work(&self.table))
    }

    /// Continues the search until the deadline.
    pub fn continue_search(&mut self, deadline: Instant) {
        let Some(search) = &mut self.search else {
            return;
        };
        if let Some((row, col)) = search.scan(&self.table, deadline) {
            self.view.move_to(row, col, &self.table, false);
        }
    }

    /// Writes pasted text in the search bar. It returns false without a bar.
    pub fn paste_in_search(&mut self, text: &str) -> bool {
        match &mut self.search {
            Some(search) => {
                search.push_str(text);
                true
            }
            None => false,
        }
    }

    /// Applies a key to the search bar.
    pub fn on_search_key(&mut self, key: KeyEvent) {
        let Some(search) = &mut self.search else {
            return;
        };
        let cell = match key.code {
            KeyCode::Enter | KeyCode::Down => search.advance(true),
            KeyCode::Up => search.advance(false),
            KeyCode::Backspace => {
                search.pop();
                None
            }
            KeyCode::Esc => {
                self.search = None;
                None
            }
            KeyCode::Char(c) if (key.modifiers - KeyModifiers::SHIFT).is_empty() => {
                search.push_str(c.encode_utf8(&mut [0; 4]));
                None
            }
            _ => None,
        };
        if let Some((row, col)) = cell {
            self.view.move_to(row, col, &self.table, false);
        }
    }

    /// Applies an action of the grid region. `extend` moves the end of the selection.
    pub fn on_action(&mut self, action: Action, extend: bool, vim: bool) -> GridAction {
        let table = &mut self.table;
        let view = &mut self.view;
        let extend = extend || (vim && view.anchor.is_some());
        let last_row = table.rows.len().saturating_sub(1);
        let last_col = table.columns.len().saturating_sub(1);
        match action {
            Action::GridFind => self.search = Some(RowSearch::new((view.row, view.col))),
            Action::GridNarrow => table.resize(view.col, -1),
            Action::GridWiden => table.resize(view.col, 1),
            Action::GridAutoWidth => table.reset_width(view.col),
            Action::GridCloseResult => return GridAction::CloseResult,
            Action::GridFirstRow => view.move_to(0, view.col, table, extend),
            Action::GridLastRow => view.move_to(last_row, view.col, table, extend),
            Action::GridFirstColumn => view.move_to(view.row, 0, table, extend),
            Action::GridLastColumn => view.move_to(view.row, last_col, table, extend),
            Action::GridUp => view.move_by(-1, 0, table, extend),
            Action::GridDown => view.move_by(1, 0, table, extend),
            Action::GridLeft => view.move_by(0, -1, table, extend),
            Action::GridRight => view.move_by(0, 1, table, extend),
            Action::GridPageUp => view.move_by(-20, 0, table, extend),
            Action::GridPageDown => view.move_by(20, 0, table, extend),
            Action::GridSelectAll => view.select_all(table),
            Action::GridCopy => return view.copy(table).map_or(GridAction::None, GridAction::Copy),
            Action::GridOpenValue => return open_value(table, view),
            Action::GridOpenRow if view.row < table.rows.len() => {
                return GridAction::OpenRow(view.row);
            }
            Action::GridClearSelection => view.anchor = None,
            Action::GridToggleSelection => view.toggle_selection(),
            _ => {}
        }
        GridAction::None
    }
}

/// Returns the start of a value, with a mark when the value goes on.
fn row_value(value: Value<'_>) -> String {
    let shown = value.preview(ROW_VALUE_WIDTH + 1);
    let (start, _) = fit(&shown, ROW_VALUE_WIDTH);
    if start.len() < shown.len() {
        start + "…"
    } else {
        shown
    }
}

fn open_value(table: &ResultTable, view: &GridView) -> GridAction {
    let Some(value) = table.rows.get(view.row).and_then(|row| row.get(view.col)) else {
        return GridAction::None;
    };
    let column = table.columns.get(view.col).map_or("", String::as_str);
    // A binary value that is not text keeps its bytes, so the window does not make the
    // digits of a large value before it shows a row.
    let text = match value {
        Value::Binary(bytes) if bytes.text().is_none() => {
            ValueText::Binary(Arc::from(bytes.bytes()))
        }
        value => ValueText::Text(value.full_text()),
    };
    let chars = text.chars();
    // Only a value that shows as text can hold JSON.
    let formatted = match (value, &text) {
        (Value::Text(_) | Value::Binary(_), ValueText::Text(contents)) => pretty(contents),
        _ => None,
    };
    let kind = match value {
        _ if formatted.is_some() => text::VALUE_JSON_ORIGINAL,
        Value::Null => text::VALUE_NULL,
        Value::Text(_) => text::VALUE_TEXT,
        Value::Number(_) => text::VALUE_NUMBER,
        Value::Binary(_) => text::VALUE_BINARY,
    };
    GridAction::OpenValue {
        title: text::value_title(&printable(column), view.row),
        footer: text::value_footer(chars, kind),
        text,
        json: formatted.map(|json| (json, text::value_footer(chars, text::VALUE_JSON))),
    }
}
