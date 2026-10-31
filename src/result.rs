//! A result grid with its cursor and its search. A console result and a table share it.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::grid::{GridView, ResultTable, RowSearch, Value, printable};
use crate::input::Action;
use crate::ui::text;

pub struct ResultTab {
    /// The number of the tab. It stays the same when the user closes another tab.
    pub number: usize,
    pub table: ResultTable,
    pub view: GridView,
    pub summary: String,
    pub search: Option<RowSearch>,
}

/// The action of a key in the results panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GridAction {
    None,
    Copy(String),
    OpenValue { title: String, text: String, footer: String },
    CloseResult,
}

impl ResultTab {
    pub fn new(number: usize, columns: Vec<String>) -> Self {
        Self {
            number,
            table: ResultTable::new(columns),
            view: GridView::default(),
            summary: String::new(),
            search: None,
        }
    }

    /// Returns true when the search has rows to read.
    pub fn search_has_work(&self) -> bool {
        self.search.as_ref().is_some_and(|search| search.has_work(&self.table))
    }

    /// Continues the search until the deadline.
    pub fn continue_search(&mut self, deadline: Instant) {
        let Some(search) = &mut self.search else { return };
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
        let Some(search) = &mut self.search else { return };
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
            Action::GridClearSelection => view.anchor = None,
            Action::GridToggleSelection => view.toggle_selection(),
            _ => {}
        }
        GridAction::None
    }
}

fn open_value(table: &ResultTable, view: &GridView) -> GridAction {
    let Some(value) = table.rows.get(view.row).and_then(|row| row.get(view.col)) else {
        return GridAction::None;
    };
    let column = table.columns.get(view.col).map_or("", String::as_str);
    let contents = value.full_text();
    let kind = match value {
        Value::Null => text::VALUE_NULL,
        Value::Text(_) => text::VALUE_TEXT,
        Value::Number(_) => text::VALUE_NUMBER,
        Value::Binary(_) => text::VALUE_BINARY,
    };
    GridAction::OpenValue {
        title: text::value_title(&printable(column), view.row),
        footer: text::value_footer(contents.chars().count(), kind),
        text: contents,
    }
}
