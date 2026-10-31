//! Table tabs: opening, paging, the filter bar, and their place in the tab row.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::App;
use crate::catalog::RelationRef;
use crate::exec::{ConnectionId, RunOptions};
use crate::input::Action;
use crate::navigator::{Row, RowKind};
use crate::sql::Dialect;
use crate::table::{PageRequest, TableFocus, TableTab};
use crate::ui::text;
use crate::ui::{main_tab_label, table};

impl App {
    pub fn tables(&self) -> &[TableTab] {
        &self.tables
    }

    /// Returns the table tab on screen.
    pub fn table(&self) -> Option<&TableTab> {
        self.table_active.map(|index| &self.tables[index])
    }

    pub(super) fn table_mut(&mut self) -> Option<&mut TableTab> {
        self.table_active.map(|index| &mut self.tables[index])
    }

    /// Lists the labels of the tab row: the consoles, then the tables.
    pub(super) fn tab_labels(&self) -> Vec<String> {
        let consoles = self.consoles.iter().map(|console| main_tab_label(&console.name, console.is_busy()));
        consoles.chain(self.tables.iter().map(table::tab_label)).collect()
    }

    /// Returns the position of the tab on screen in the tab row.
    pub(super) fn active_tab(&self) -> usize {
        self.table_active.map_or(self.active, |index| self.consoles.len() + index)
    }

    pub(super) fn select_tab(&mut self, index: usize) {
        match index.checked_sub(self.consoles.len()) {
            Some(table) => self.select_table(table),
            None => self.select_console(index),
        }
    }

    pub(super) fn select_table(&mut self, index: usize) {
        if index < self.tables.len() {
            self.console_mut().editor.close_suggestions();
            self.table_active = Some(index);
        }
    }

    pub(super) fn switch_tab(&mut self, forward: bool) {
        let count = self.consoles.len() + self.tables.len();
        let next = (self.active_tab() + if forward { 1 } else { count - 1 }) % count;
        self.select_tab(next);
    }

    fn dialect_of(&self, connection: ConnectionId) -> Option<Dialect> {
        self.connection(connection).map(|item| item.dialect)
    }

    /// Opens the table of a navigator row. A table that is open only gets the focus.
    pub(super) fn open_table(&mut self, row: &Row) {
        let Some(relation) = row.relation_ref(&self.connections) else { return };
        let connection = self.connections[row.connection].id;
        if let Some(index) = self.tables.iter().position(|tab| tab.is(connection, &relation)) {
            self.select_table(index);
            self.navigator_focused = false;
            return;
        }
        let Some(id) = self.manager.as_mut().and_then(|manager| manager.open_console(connection)) else {
            return;
        };
        let view = row.kind == RowKind::View;
        self.tables.push(TableTab::new(id, connection, relation, view, self.settings.page_size));
        let index = self.tables.len() - 1;
        self.select_table(index);
        self.navigator_focused = false;
        self.apply_table();
    }

    /// Sends a page to the task of the tab on screen.
    fn run_page(&mut self, request: Option<PageRequest>) {
        let (Some(request), Some(index), Some(manager)) =
            (request, self.table_active, self.manager.as_ref())
        else {
            return;
        };
        let tab = &mut self.tables[index];
        let options = RunOptions { manual: false, schema: None, row_cap: Some(request.row_cap) };
        let execution = manager.run(tab.id, vec![request.sql], options);
        tab.started(execution, request.first);
    }

    /// Returns the tab on screen and its dialect, when it can start a query.
    fn ready_table(&mut self) -> Option<Dialect> {
        let tab = self.table()?;
        if tab.is_busy() {
            self.status = Some(text::TABLE_BUSY.to_owned());
            return None;
        }
        self.dialect_of(tab.connection)
    }

    /// Applies the fields: a new query from the first page.
    pub(super) fn apply_table(&mut self) {
        let Some(dialect) = self.ready_table() else { return };
        let budget = self.budget.clone();
        let request = self.table_mut().and_then(|tab| tab.apply(dialect, &budget));
        self.run_page(request);
    }

    pub(super) fn clear_table(&mut self) {
        let Some(dialect) = self.ready_table() else { return };
        let budget = self.budget.clone();
        let request = self.table_mut().and_then(|tab| tab.clear(dialect, &budget));
        self.run_page(request);
    }

    /// Sorts the table by a column. The click on a header and the `s` key do this.
    pub(super) fn sort_table(&mut self, column: usize) {
        let Some(dialect) = self.ready_table() else { return };
        let budget = self.budget.clone();
        let request = self.table_mut().and_then(|tab| tab.sort_by(column, dialect, &budget));
        self.run_page(request);
    }

    pub(super) fn load_more(&mut self) {
        let Some(dialect) = self.ready_table() else { return };
        let budget = self.budget.clone();
        let Some(tab) = self.table_mut() else { return };
        match tab.next_page(dialect, &budget) {
            Ok(request) => self.run_page(Some(request)),
            Err(reason) => self.status = Some(reason.to_owned()),
        }
    }

    pub(super) fn stop_table(&mut self) {
        let Some(index) = self.table_active else { return };
        let tab = &mut self.tables[index];
        if !tab.is_busy() {
            return;
        }
        if let Some(manager) = &self.manager {
            manager.stop(tab.id);
            tab.phase = crate::console::Phase::Stopping;
        }
    }

    /// Closes a table tab, and its rows leave the budget. The load in progress stops.
    pub(super) fn close_table(&mut self, index: usize) {
        if index >= self.tables.len() {
            return;
        }
        let tab = self.tables.remove(index);
        self.budget.release(tab.result.table.bytes);
        if let Some(manager) = &mut self.manager {
            manager.close_console(tab.id);
        }
        // The tab on the right takes the place of a closed tab on screen. At the end of
        // the row, the tab on the left takes it, and then the console.
        self.table_active = match self.table_active {
            Some(active) if active == index => {
                (index < self.tables.len()).then_some(index).or(index.checked_sub(1))
            }
            Some(active) if active > index => Some(active - 1),
            other => other,
        };
    }

    /// Closes the table tabs of a connection that the user removed.
    pub(super) fn close_tables_of(&mut self, connection: ConnectionId) {
        for index in (0..self.tables.len()).rev() {
            if self.tables[index].connection == connection {
                self.close_table(index);
            }
        }
    }

    /// Applies an action to the tab on screen. It returns false for an action that
    /// the console and the table share.
    pub(super) fn perform_on_table(&mut self, action: Action) -> bool {
        match action {
            // A table has no script, no transaction, and no schema choice.
            Action::Run
            | Action::RunScript
            | Action::ToggleMode
            | Action::Commit
            | Action::Rollback
            | Action::ChooseSchema
            | Action::GridCloseResult
            | Action::GridPreviousResult
            | Action::GridNextResult => {}
            Action::Stop => self.stop_table(),
            Action::CloseTab => {
                if let Some(index) = self.table_active {
                    self.close_table(index);
                }
            }
            Action::FocusUp | Action::GridFocusFilter => self.focus_table(TableFocus::Filter),
            Action::GridFocusOrder => self.focus_table(TableFocus::Order),
            Action::FocusDown => self.focus_table(TableFocus::Grid),
            Action::GridSortColumn => {
                let column = self.table().map(|tab| tab.result.view.col);
                if let Some(column) = column {
                    self.sort_table(column);
                }
            }
            Action::GridLoadMore => self.load_more(),
            _ => return false,
        }
        true
    }

    pub(super) fn focus_table(&mut self, focus: TableFocus) {
        self.navigator_focused = false;
        if let Some(tab) = self.table_mut() {
            tab.list = None;
            tab.focus = focus;
        }
    }

    /// Handles a key for the open list of the filter bar. It returns false when other
    /// handlers must act.
    pub(super) fn on_table_list_key(&mut self, key: KeyEvent) -> bool {
        !self.navigator_focused && self.table_mut().is_some_and(|tab| tab.on_list_key(key))
    }

    /// Applies a key to the text of the filter bar or to the search of the grid.
    pub(super) fn on_table_text_key(&mut self, key: KeyEvent) {
        let Some(tab) = self.table_mut() else { return };
        if tab.focus == TableFocus::Grid {
            tab.result.on_search_key(key);
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Enter => self.apply_table(),
            KeyCode::Esc => self.focus_table(TableFocus::Grid),
            KeyCode::Tab | KeyCode::BackTab => tab.switch_field(),
            KeyCode::Char(' ') if ctrl => self.edit_table_field(None),
            _ => self.edit_table_field(Some(key)),
        }
    }

    /// Edits the focused field, or it opens the list when there is no key.
    fn edit_table_field(&mut self, key: Option<KeyEvent>) {
        let Some(index) = self.table_active else { return };
        let connection = self.tables[index].connection;
        let Some(dialect) = self.dialect_of(connection) else { return };
        let App { tables, connections, .. } = self;
        let catalog = connections.iter().find(|item| item.id == connection).and_then(|item| item.catalog.as_ref());
        let tab = &mut tables[index];
        let missing = match key {
            Some(key) => tab.on_field_key(key, dialect, catalog),
            None => tab.open_list(dialect, catalog),
        };
        self.request_relation_columns(connection, missing);
    }

    fn request_relation_columns(&mut self, connection: ConnectionId, relations: Vec<RelationRef>) {
        for relation in relations {
            self.request_columns(connection, relation);
        }
    }

    /// Gives the new catalog to the open lists of the table tabs of a connection.
    pub(super) fn refresh_tables(&mut self, connection: ConnectionId) {
        let Some(dialect) = self.dialect_of(connection) else { return };
        for index in 0..self.tables.len() {
            if self.tables[index].connection != connection {
                continue;
            }
            let App { tables, connections, .. } = self;
            let catalog =
                connections.iter().find(|item| item.id == connection).and_then(|item| item.catalog.as_ref());
            let missing = tables[index].refresh_list(dialect, catalog);
            self.request_relation_columns(connection, missing);
        }
    }
}
