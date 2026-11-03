//! Table tabs: opening, paging, the filter bar, and their place in the tab row.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::guard::short;
use super::{App, DialogAction, FRAMES_PER_PICTURE};
use crate::catalog::RelationRef;
use crate::exec::{ConnectionId, ConsoleId};
use crate::input::Action;
use crate::navigator::{Row, RowKind};
use crate::sql::Dialect;
use crate::structure::{Scope, Section, SessionSignal, Structure, structure_sql};
use crate::table::{PageRequest, TableFocus, TableTab};
use crate::ui::text;
use crate::ui::{TabView, main_tab_label, run_badge, table};

/// One tab of the tab row, with its index in the list of its kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tab {
    Console(usize),
    Table(usize),
}

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

    /// Lists the tab row in the order of opening. Consoles and tables stay in one row.
    pub(super) fn tab_order(&self) -> Vec<Tab> {
        let consoles = self
            .consoles
            .iter()
            .enumerate()
            .filter(|(_, console)| !console.hidden);
        let consoles = consoles.map(|(index, console)| (console.opened, Tab::Console(index)));
        let tables = self
            .tables
            .iter()
            .enumerate()
            .map(|(index, tab)| (tab.opened, Tab::Table(index)));
        let mut order: Vec<(u64, Tab)> = consoles.chain(tables).collect();
        order.sort_by_key(|(opened, _)| *opened);
        order.into_iter().map(|(_, tab)| tab).collect()
    }

    /// Lists the tabs of the tab row, with the state of their runs.
    pub(super) fn tab_views(&self) -> Vec<TabView> {
        let view = |tab| {
            let (label, phase, run) = match tab {
                Tab::Console(index) => {
                    let console = &self.consoles[index];
                    let unsaved = console
                        .file
                        .as_ref()
                        .is_some_and(|file| file.trouble.is_some());
                    let label = main_tab_label(self.theme.icons.console, &console.name, unsaved);
                    (label, console.phase, &console.run)
                }
                Tab::Table(index) => {
                    let tab = &self.tables[index];
                    (table::tab_label(tab, &self.theme), tab.phase, &tab.run)
                }
            };
            let picture = self.run_frame / FRAMES_PER_PICTURE;
            TabView {
                label,
                badge: run_badge(phase, run, self.clock, picture, &self.theme),
            }
        };
        self.tab_order().into_iter().map(view).collect()
    }

    /// Returns the position of the tab on screen in the tab row.
    pub(super) fn active_tab(&self) -> usize {
        let active = self
            .table_active
            .map_or(Tab::Console(self.active), Tab::Table);
        self.tab_order()
            .iter()
            .position(|tab| *tab == active)
            .unwrap_or(0)
    }

    pub(super) fn select_tab(&mut self, index: usize) {
        match self.tab_order().get(index) {
            Some(Tab::Table(table)) => self.select_table(*table),
            Some(Tab::Console(console)) => self.select_console(*console),
            None => {}
        }
    }

    /// Closes a tab of the row, with the questions of the key. The tab on screen stays
    /// when another tab closes.
    pub(super) fn close_tab(&mut self, index: usize) {
        self.console_mut().editor.close_suggestions();
        match self.tab_order().get(index) {
            Some(Tab::Table(table)) => self.close_table_tab(*table),
            Some(Tab::Console(console)) => self.close_step(self.consoles[*console].id),
            None => {}
        }
    }

    pub(super) fn select_table(&mut self, index: usize) {
        if index < self.tables.len() {
            self.console_mut().editor.close_suggestions();
            if self.table_active != Some(index) {
                self.leave_tab();
            }
            self.table_active = Some(index);
            if self.tables[index].pending {
                self.apply_table();
            }
        }
    }

    pub(super) fn switch_tab(&mut self, forward: bool) {
        let count = self.tab_order().len();
        let next = (self.active_tab() + if forward { 1 } else { count - 1 }) % count;
        self.select_tab(next);
    }

    fn dialect_of(&self, connection: ConnectionId) -> Option<Dialect> {
        self.connection(connection).map(|item| item.dialect)
    }

    /// Opens the table of a navigator row. A table that is open only gets the focus.
    pub(super) fn open_table(&mut self, row: &Row) {
        if self.config_blocked() {
            return;
        }
        let Some(relation) = row.relation_ref(&self.connections) else {
            return;
        };
        let connection = self.connections[row.connection].id;
        if let Some(index) = self
            .tables
            .iter()
            .position(|tab| tab.is(connection, &relation))
        {
            self.select_table(index);
            self.navigator_focused = false;
            return;
        }
        let Some(id) = self
            .manager
            .as_mut()
            .and_then(|manager| manager.open_console(connection))
        else {
            return;
        };
        let view = row.kind == RowKind::View;
        let tab = self.new_table_tab(id, connection, relation, view);
        self.tables.push(tab);
        let index = self.tables.len() - 1;
        self.select_table(index);
        self.navigator_focused = false;
        self.apply_table();
    }

    /// Opens the structure of a navigator row. A column shows the structure of its table.
    pub(super) fn open_row_structure(&mut self, row: &Row) {
        let item = &self.connections[row.connection];
        let connection = item.id;
        let (scope, relation, name) = match row.kind {
            RowKind::Connection => (Scope::Connection, RelationRef::default(), item.name.clone()),
            RowKind::Schema => {
                let schema = row.name(&self.connections).to_owned();
                let relation = RelationRef {
                    schema: schema.clone(),
                    relation: String::new(),
                };
                (Scope::Schema, relation, schema)
            }
            RowKind::Table | RowKind::View | RowKind::Column => {
                let Some(relation) = row.relation_ref(&self.connections) else {
                    return;
                };
                let name = relation.relation.clone();
                (Scope::Relation, relation, name)
            }
        };
        self.open_structure(connection, scope, relation, name);
    }

    /// Opens the sessions of the server of a navigator row.
    pub(super) fn open_row_sessions(&mut self, row: &Row) {
        let item = &self.connections[row.connection];
        let (connection, name) = (item.id, item.name.clone());
        self.open_structure(connection, Scope::Sessions, RelationRef::default(), name);
    }

    /// Returns true when the navigator row belongs to a source with sessions.
    pub(super) fn can_open_sessions(&self) -> bool {
        self.navigator
            .current(&self.connections)
            .is_some_and(|row| {
                let dialect = self.connections[row.connection].dialect;
                matches!(dialect, Dialect::Postgres | Dialect::MySql)
            })
    }

    /// Returns true when a key can send a signal to the session under the cursor.
    pub(super) fn can_signal_session(&self) -> bool {
        self.table().is_some_and(|tab| {
            tab.shows_sessions()
                && tab.loaded() > 0
                && !tab.is_busy()
                && !self.is_read_only(tab.connection)
        })
    }

    /// Asks before a signal to the session under the cursor.
    fn ask_session_signal(&mut self, signal: SessionSignal) {
        let Some(tab) = self.table().filter(|tab| tab.shows_sessions()) else {
            return;
        };
        if self.is_read_only(tab.connection) {
            self.status = Some(text::SESSIONS_READ_ONLY.to_owned());
            return;
        }
        if tab.is_busy() {
            self.status = Some(text::TABLE_BUSY.to_owned());
            return;
        }
        let console = tab.id;
        let Some(row) = tab.result.table.rows.get(tab.result.view.row) else {
            return;
        };
        let cell = |column: usize| row.get(column).map(|value| value.full_text());
        let Some(session) = cell(0).and_then(|text| text.parse::<u64>().ok()) else {
            return;
        };
        let user = cell(1).unwrap_or_default();
        let query = cell(row.columns().saturating_sub(1)).unwrap_or_default();
        let body = text::session_signal_body(signal, session, &user, &short(&query));
        let (title, option) = match signal {
            SessionSignal::CancelQuery => (text::TITLE_CANCEL_QUERY, text::OPTION_CANCEL_QUERY),
            SessionSignal::EndSession => (text::TITLE_END_SESSION, text::OPTION_END_SESSION),
        };
        self.show_dialog(
            title,
            body,
            vec![
                (text::OPTION_CANCEL, DialogAction::Dismiss),
                (
                    option,
                    DialogAction::SessionSignal(console, signal, session),
                ),
            ],
        );
    }

    /// Sends the signal that the user confirmed. The tab must still be on screen.
    pub(super) fn send_session_signal(
        &mut self,
        console: ConsoleId,
        signal: SessionSignal,
        session: u64,
    ) {
        if self.table().is_none_or(|tab| tab.id != console) {
            return;
        }
        let Some(dialect) = self.ready_table() else {
            return;
        };
        self.run_page(TableTab::signal_request(dialect, signal, session));
    }

    /// Shows the end of a signal and reads the sessions again. A tab that is not on
    /// screen reads them when it comes back.
    pub(super) fn finish_session_signal(&mut self, index: usize) {
        if let Some(note) = self.tables[index].signal_note.take() {
            self.status = Some(note.to_owned());
            if self.table_active == Some(index) {
                self.apply_table();
            }
        }
    }

    /// Opens the structure of the table tab on screen.
    fn open_table_structure(&mut self) {
        if let Some(tab) = self.table().filter(|tab| tab.structure.is_none()) {
            let (connection, relation) = (tab.connection, tab.relation.clone());
            let name = relation.relation.clone();
            self.open_structure(connection, Scope::Relation, relation, name);
        }
    }

    /// Opens a structure tab. A structure that is open only gets the focus. For a source
    /// without structure, the status line gives the reason.
    fn open_structure(
        &mut self,
        connection: ConnectionId,
        scope: Scope,
        relation: RelationRef,
        name: String,
    ) {
        if self.config_blocked() {
            return;
        }
        let Some(dialect) = self.dialect_of(connection) else {
            return;
        };
        if structure_sql(dialect, scope, &relation, Section::Columns).is_none() {
            let reason = match scope {
                Scope::Sessions => text::SESSIONS_REFUSED,
                _ => text::STRUCTURE_REFUSED,
            };
            self.status = Some(reason.to_owned());
            return;
        }
        if let Some(index) = self
            .tables
            .iter()
            .position(|tab| tab.shows_structure(connection, scope, &relation))
        {
            self.select_table(index);
            self.navigator_focused = false;
            return;
        }
        let Some(id) = self
            .manager
            .as_mut()
            .and_then(|manager| manager.open_console(connection))
        else {
            return;
        };
        let mut tab = self.new_table_tab(id, connection, relation, false);
        tab.filters = false;
        tab.structure = Some(Structure::new(scope, name));
        self.tables.push(tab);
        let index = self.tables.len() - 1;
        self.select_table(index);
        self.navigator_focused = false;
        self.apply_table();
    }

    /// Shows a section of the structure tab on screen. The grid reads it again.
    pub(super) fn show_section(&mut self, section: Section) {
        if self.ready_table().is_none() {
            return;
        }
        if let Some(tab) = self.table_mut() {
            tab.set_section(section);
        }
        self.apply_table();
    }

    /// Moves to the next or the previous section. A tab without sections moves a page.
    fn step_table(&mut self, forward: bool) {
        match self.table().and_then(TableTab::section) {
            Some(section) => self.show_section(section.step(forward)),
            None if forward => self.next_table_page(),
            None => self.previous_table_page(),
        }
    }

    /// Builds a tab with the features of its connection, at the end of the tab row.
    pub(super) fn new_table_tab(
        &mut self,
        id: ConsoleId,
        connection: ConnectionId,
        relation: RelationRef,
        view: bool,
    ) -> TableTab {
        let mut tab = TableTab::new(id, connection, relation, view, self.settings.page_size);
        tab.opened = self.next_tab_place();
        tab.filters = self
            .connection(connection)
            .is_none_or(|item| item.capabilities.table_filters);
        tab
    }

    /// Sends a page to the task of the tab on screen.
    fn run_page(&mut self, request: Option<PageRequest>) {
        if request.is_some() && self.config_blocked() {
            return;
        }
        let (Some(request), Some(index)) = (request, self.table_active) else {
            return;
        };
        let tab = &self.tables[index];
        let (console, connection) = (tab.id, tab.connection);
        self.authorize(
            connection,
            super::auth::Use::Page {
                console,
                connection,
                request,
            },
        );
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
        let Some(dialect) = self.ready_table() else {
            return;
        };
        let budget = self.budget.clone();
        let request = self.table_mut().and_then(|tab| tab.apply(dialect, &budget));
        self.run_page(request);
    }

    pub(super) fn clear_table(&mut self) {
        let Some(dialect) = self.ready_table() else {
            return;
        };
        let budget = self.budget.clone();
        let request = self.table_mut().and_then(|tab| tab.clear(dialect, &budget));
        self.run_page(request);
    }

    /// Sorts the table by a column. The click on a header and the `s` key do this.
    pub(super) fn sort_table(&mut self, column: usize) {
        let Some(dialect) = self.ready_table() else {
            return;
        };
        let budget = self.budget.clone();
        let request = self
            .table_mut()
            .and_then(|tab| tab.sort_by(column, dialect, &budget));
        self.run_page(request);
    }

    pub(super) fn load_more(&mut self) {
        self.load_page(None);
    }

    /// Asks for the total of rows of the applied filter.
    fn count_table(&mut self) {
        if !self.command_available(Action::GridCountRows) {
            return;
        }
        let Some(dialect) = self.ready_table() else {
            return;
        };
        let request = self.table().and_then(|tab| tab.count_request(dialect));
        self.run_page(request);
    }

    /// Asks for the next page. `show` is the page to show at the end of the load.
    fn load_page(&mut self, show: Option<usize>) {
        let Some(dialect) = self.ready_table() else {
            return;
        };
        let budget = self.budget.clone();
        let Some(tab) = self.table_mut() else { return };
        match tab.next_page(dialect, &budget) {
            Ok(request) => self.run_page(Some(PageRequest { show, ..request })),
            Err(reason) => self.status = Some(reason.to_owned()),
        }
    }

    /// Shows the page before the cursor. Its rows are on the grid.
    pub(super) fn previous_table_page(&mut self) {
        if let Some(tab) = self.table_mut() {
            tab.show_page(tab.page().saturating_sub(1));
        }
    }

    /// Shows the page after the cursor. A page that is not on the grid loads first.
    pub(super) fn next_table_page(&mut self) {
        let Some(tab) = self.table_mut() else { return };
        let page = tab.page() + 1;
        if page < tab.loaded_pages() {
            tab.show_page(page);
        } else {
            self.load_page(Some(page));
        }
    }

    pub(super) fn stop_table(&mut self) {
        let Some(index) = self.table_active else {
            return;
        };
        if self.stop_export(self.tables[index].id) {
            return;
        }
        if self.cancel_auth_console(self.tables[index].id) {
            return;
        }
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
        self.cancel_auth_console(self.tables[index].id);
        self.discard_export(self.tables[index].id);
        let tab = self.tables.remove(index);
        self.budget.release(tab.result.table.bytes);
        if let Some(manager) = &mut self.manager {
            manager.close_console(tab.id);
        }
        // The tab on the right takes the place of a closed tab on screen. At the end of
        // the row, the tab on the left takes it, and then the console.
        self.table_active = match self.table_active {
            Some(active) if active == index => (index < self.tables.len())
                .then_some(index)
                .or(index.checked_sub(1)),
            Some(active) if active > index => Some(active - 1),
            other => other,
        };
        // The hidden console comes back when a table closes for another reason.
        if self.table_active.is_none() {
            self.console_mut().hidden = false;
        }
    }

    /// Closes a table tab. The close of the last tab ends the application.
    fn close_table_tab(&mut self, index: usize) {
        if self.tables.len() == 1 && self.consoles.iter().all(|console| console.hidden) {
            self.request_exit();
            return;
        }
        self.close_table(index);
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
            | Action::Explain
            | Action::ExplainAnalyze
            | Action::ToggleMode
            | Action::Commit
            | Action::Rollback
            | Action::ChooseSchema
            | Action::RenameConsole
            | Action::ChangeConnection
            | Action::GridCloseResult => {}
            Action::GridPreviousResult => self.step_table(false),
            Action::GridNextResult => self.step_table(true),
            Action::GridStructure => self.open_table_structure(),
            Action::GridReload if self.table().is_some_and(|tab| !tab.filters) => {
                self.apply_table();
            }
            Action::GridCancelQuery => self.ask_session_signal(SessionSignal::CancelQuery),
            Action::GridEndSession => self.ask_session_signal(SessionSignal::EndSession),
            Action::Stop => self.stop_table(),
            Action::CloseTab => {
                if let Some(index) = self.table_active {
                    self.close_table_tab(index);
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
            Action::GridCountRows => self.count_table(),
            Action::GridOpenValue if self.table().is_some_and(|tab| tab.pending) => {
                self.apply_table()
            }
            _ => return false,
        }
        true
    }

    pub(super) fn focus_table(&mut self, focus: TableFocus) {
        self.navigator_focused = false;
        if let Some(tab) = self.table_mut() {
            tab.list = None;
            tab.focus = if tab.filters { focus } else { TableFocus::Grid };
        }
    }

    /// Handles a key for the open list of the filter bar. It returns false when other
    /// handlers must act.
    pub(super) fn on_table_list_key(&mut self, key: KeyEvent) -> bool {
        let keys = self.suggestion_keys;
        !self.navigator_focused
            && self
                .table_mut()
                .is_some_and(|tab| tab.on_list_key(key, keys))
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
        let Some(index) = self.table_active else {
            return;
        };
        let connection = self.tables[index].connection;
        let Some(dialect) = self.dialect_of(connection) else {
            return;
        };
        let App {
            tables,
            connections,
            ..
        } = self;
        let catalog = connections
            .iter()
            .find(|item| item.id == connection)
            .and_then(|item| item.catalog.as_ref());
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
        let Some(dialect) = self.dialect_of(connection) else {
            return;
        };
        for index in 0..self.tables.len() {
            if self.tables[index].connection != connection {
                continue;
            }
            let App {
                tables,
                connections,
                ..
            } = self;
            let catalog = connections
                .iter()
                .find(|item| item.id == connection)
                .and_then(|item| item.catalog.as_ref());
            let missing = tables[index].refresh_list(dialect, catalog);
            self.request_relation_columns(connection, missing);
        }
    }
}
