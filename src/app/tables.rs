//! Table tabs: opening, paging, the filter bar, and their place in the tab row.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::guard::short;
use super::{App, DialogAction, FRAMES_PER_PICTURE, Picker, PickerKind};
use crate::catalog::{Columns, Relation, RelationRef};
use crate::console::{Phase, RunClock};
use crate::exec::{ConnectionId, ConsoleId};
use crate::input::Action;
use crate::navigator::{Row, RowKind};
use crate::sql::Dialect;
use crate::structure::{Scope, Section, SessionSignal, Structure, has_sessions, structure_sql};
use crate::tab_item::TabItem;
use crate::table::{Follow, PageRequest, TableFocus, TableTab, follow_choices};
use crate::ui::text;
use crate::ui::theme::Theme;
use crate::ui::{TabView, main_tab_label, run_badge};

/// One tab of the tab row, with its index in the list of its kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tab {
    Console(usize),
    Table(usize),
    Diagram(usize),
    Compare(usize),
}

/// The tab on screen. `Console` shows the active console.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Screen {
    #[default]
    Console,
    Table(usize),
    Diagram(usize),
    Compare(usize),
}

impl App {
    pub fn tables(&self) -> &[TableTab] {
        &self.tables
    }

    /// Returns the table tab on screen.
    pub fn table(&self) -> Option<&TableTab> {
        self.table_index().map(|index| &self.tables[index])
    }

    pub(super) fn table_mut(&mut self) -> Option<&mut TableTab> {
        self.table_index().map(|index| &mut self.tables[index])
    }

    /// Returns the index of the table tab on screen.
    pub(super) fn table_index(&self) -> Option<usize> {
        match self.screen {
            Screen::Table(index) => Some(index),
            _ => None,
        }
    }

    /// Returns the index of the diagram tab on screen.
    pub(super) fn diagram_index(&self) -> Option<usize> {
        match self.screen {
            Screen::Diagram(index) => Some(index),
            _ => None,
        }
    }

    /// Returns the index of the compare tab on screen.
    pub(super) fn compare_index(&self) -> Option<usize> {
        match self.screen {
            Screen::Compare(index) => Some(index),
            _ => None,
        }
    }

    /// Returns the tab on screen when it is not a console.
    pub(super) fn screen_item_mut(&mut self) -> Option<&mut dyn TabItem> {
        match self.screen {
            Screen::Table(index) => Some(&mut self.tables[index]),
            Screen::Diagram(index) => Some(&mut self.diagrams[index]),
            Screen::Compare(index) => Some(&mut self.compares[index]),
            Screen::Console => None,
        }
    }

    /// Lists the tab row in the order of opening. Consoles and tables stay in one row.
    pub(super) fn tab_order(&self) -> Vec<Tab> {
        let consoles = self
            .consoles
            .iter()
            .enumerate()
            .filter(|(_, console)| !console.hidden);
        let consoles = consoles.map(|(index, console)| (console.opened, Tab::Console(index)));
        let mut order: Vec<(u64, Tab)> = consoles
            .chain(opened(&self.tables, Tab::Table))
            .chain(opened(&self.diagrams, Tab::Diagram))
            .chain(opened(&self.compares, Tab::Compare))
            .collect();
        // Agent tabs come last, in the order of opening.
        order.sort_by_key(|(opened, tab)| (self.agent_tab(*tab), *opened));
        order.into_iter().map(|(_, tab)| tab).collect()
    }

    /// Returns true for the console tab of an agent.
    fn agent_tab(&self, tab: Tab) -> bool {
        matches!(tab, Tab::Console(index) if self.agent.owns(self.consoles[index].id))
    }

    /// Lists the tabs of the tab row, with the state of their runs.
    pub(super) fn tab_views(&self) -> Vec<TabView> {
        let view = |tab| {
            let (label, phase, run) = match tab {
                Tab::Console(index) => {
                    let console = &self.consoles[index];
                    let revision = console.editor.revision();
                    let unsaved = console
                        .file
                        .as_ref()
                        .is_some_and(|file| file.trouble.is_some())
                        || console.external.as_ref().is_some_and(|external| {
                            external.trouble.is_some() || external.is_dirty(revision)
                        });
                    let name = self
                        .agent
                        .console_label(&self.consoles, console.id, &console.name);
                    let label = main_tab_label(self.theme.icons.console, &name, unsaved);
                    (label, console.phase, &console.run)
                }
                Tab::Table(index) => item_view(&self.tables[index], &self.theme),
                Tab::Diagram(index) => item_view(&self.diagrams[index], &self.theme),
                Tab::Compare(index) => item_view(&self.compares[index], &self.theme),
            };
            let picture = self.run_frame / FRAMES_PER_PICTURE;
            TabView {
                label,
                badge: run_badge(phase, run, self.clock, picture, &self.theme),
                right: self.agent_tab(tab),
            }
        };
        self.tab_order().into_iter().map(view).collect()
    }

    /// Returns the position of the tab on screen in the tab row.
    pub(super) fn active_tab(&self) -> usize {
        let active = match self.screen {
            Screen::Compare(index) => Tab::Compare(index),
            Screen::Diagram(index) => Tab::Diagram(index),
            Screen::Table(index) => Tab::Table(index),
            Screen::Console => Tab::Console(self.active),
        };
        self.tab_order()
            .iter()
            .position(|tab| *tab == active)
            .unwrap_or(0)
    }

    pub(super) fn select_tab(&mut self, index: usize) {
        match self.tab_order().get(index) {
            Some(Tab::Table(table)) => self.select_table(*table),
            Some(Tab::Console(console)) => self.select_console(*console),
            Some(Tab::Diagram(diagram)) => self.select_diagram(*diagram),
            Some(Tab::Compare(compare)) => self.select_compare(*compare),
            None => {}
        }
    }

    /// Closes a tab of the row, with the questions of the key. The tab on screen stays
    /// when another tab closes.
    pub(super) fn close_tab(&mut self, index: usize) {
        self.console_mut().editor.close_suggestions();
        match self.tab_order().get(index) {
            Some(Tab::Table(table)) => self.close_table(*table),
            Some(Tab::Console(console)) => self.close_step(self.consoles[*console].id),
            Some(Tab::Diagram(diagram)) => self.close_diagram(*diagram),
            Some(Tab::Compare(compare)) => self.close_compare(*compare),
            None => {}
        }
    }

    pub(super) fn select_table(&mut self, index: usize) {
        if index < self.tables.len() {
            self.console_mut().editor.close_suggestions();
            if self.table_index() != Some(index) {
                self.leave_tab();
            }
            self.screen = Screen::Table(index);
            if self.tables[index].pending {
                if self.tables[index].edit_reload {
                    self.reload_saved_table();
                } else {
                    self.apply_table();
                }
            }
        }
    }

    pub(super) fn switch_tab(&mut self, forward: bool) {
        let count = self.tab_order().len();
        let next = (self.active_tab() + if forward { 1 } else { count - 1 }) % count;
        self.select_tab(next);
    }

    pub(super) fn dialect_of(&self, connection: ConnectionId) -> Option<Dialect> {
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
        self.open_relation(connection, relation, row.kind == RowKind::View, None);
    }

    /// Opens the table tab of a relation. A table that is open only gets the focus. With a
    /// filter, the tab reads the rows of that filter; an open tab must be idle.
    pub(super) fn open_relation(
        &mut self,
        connection: ConnectionId,
        relation: RelationRef,
        view: bool,
        filter: Option<String>,
    ) {
        if let Some(index) = self
            .tables
            .iter()
            .position(|tab| tab.is(connection, &relation))
        {
            self.select_table(index);
            self.navigator_focused = false;
            let Some(filter) = filter else {
                return;
            };
            if self.ready_table().is_none() {
                return;
            }
            if let Some(tab) = self.table_mut() {
                tab.filter.set(&filter);
                tab.focus = TableFocus::Grid;
            }
            self.apply_table();
            return;
        }
        let Some(id) = self
            .manager
            .as_mut()
            .and_then(|manager| manager.open_console(connection))
        else {
            return;
        };
        let mut tab = self.new_table_tab(id, connection, relation, view);
        if let Some(filter) = filter {
            tab.filter.set(&filter);
        }
        self.tables.push(tab);
        let index = self.tables.len() - 1;
        self.select_table(index);
        self.navigator_focused = false;
        self.apply_table();
    }

    /// Returns true when the tab on screen shows the rows of a table, and its keys are not
    /// known to be absent.
    pub(super) fn can_follow_key(&self) -> bool {
        let Some(tab) = self.table() else {
            return false;
        };
        if !tab.filters
            || tab.filter_only
            || tab.is_view
            || tab.structure.is_some()
            || !tab.result.edits.is_empty()
            || tab.edit_uncertain
        {
            return false;
        }
        match self.tab_relation(tab) {
            Some(relation) => {
                relation.columns == Columns::NotLoaded
                    || !relation.foreign_keys.is_empty()
                    || !relation.referencing.is_empty()
            }
            None => false,
        }
    }

    fn tab_relation(&self, tab: &TableTab) -> Option<&Relation> {
        let catalog = self.catalog(tab.connection)?;
        let relation = &tab.relation;
        let (_, found) = catalog.relation(Some(&relation.schema), &relation.relation)?;
        Some(found)
    }

    /// Follows a key of the row of the cursor. The keys of the table arrive with its
    /// columns, so a table without them reads them first.
    pub(super) fn follow_key(&mut self) {
        if !self.can_follow_key() {
            self.status = Some(text::FOLLOW_REFUSED.to_owned());
            return;
        }
        let Some(tab) = self.table() else { return };
        let (id, connection) = (tab.id, tab.connection);
        let Some(relation) = self.tab_relation(tab) else {
            return;
        };
        if relation.columns == Columns::NotLoaded {
            let relation = tab.relation.clone();
            self.pending_follow = Some(id);
            self.request_columns(connection, relation);
            return;
        }
        let Some(dialect) = self.dialect_of(connection) else {
            return;
        };
        let view = &tab.result.view;
        let table = &tab.result.table;
        let mut choices = match table.rows.get(view.row) {
            Some(row) => follow_choices(
                &tab.relation.schema,
                relation,
                &table.columns,
                row,
                view.col,
                dialect,
            ),
            None => Vec::new(),
        };
        match choices.len() {
            0 => self.status = Some(text::FOLLOW_NONE.to_owned()),
            1 => self.go_follow(connection, choices.remove(0)),
            _ => {
                self.modals.picker = Some(Picker {
                    items: choices.iter().map(|choice| choice.label.clone()).collect(),
                    kind: PickerKind::Follow {
                        connection,
                        choices,
                    },
                    title: text::TITLE_FOLLOW.to_owned(),
                    cursor: 0,
                    loading: false,
                });
            }
        }
    }

    /// Opens the tab of the target of a key with the filter of the row.
    pub(super) fn go_follow(&mut self, connection: ConnectionId, follow: Follow) {
        self.open_relation(connection, follow.target, false, Some(follow.filter));
    }

    /// Follows the key of a tab that waited for the keys of its table.
    pub(super) fn continue_follow(&mut self, connection: ConnectionId, relation: &RelationRef) {
        let Some(id) = self.pending_follow else {
            return;
        };
        let Some(tab) = self.tables.iter().find(|tab| tab.id == id) else {
            self.pending_follow = None;
            return;
        };
        if tab.connection != connection || tab.relation != *relation {
            return;
        }
        self.pending_follow = None;
        if self.table().is_some_and(|tab| tab.id == id) {
            self.follow_key();
        }
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
            RowKind::Folder | RowKind::Consoles | RowKind::Console => return,
            RowKind::Group(_) | RowKind::Object(_) => {
                self.status = Some(text::NOT_FOR_OBJECT.to_owned());
                return;
            }
        };
        self.open_structure(connection, scope, relation, name);
    }

    /// Opens the sessions of the server of a navigator row.
    pub(super) fn open_row_sessions(&mut self, row: &Row) {
        self.open_sessions_of(self.connections[row.connection].id);
    }

    /// Opens the sessions of the connection that `Ctrl+W S` names.
    pub(super) fn open_sessions(&mut self) {
        match self.sessions_connection() {
            Some(connection) => self.open_sessions_of(connection),
            None => self.status = Some(text::NO_SELECTED_CONNECTION.to_owned()),
        }
    }

    fn open_sessions_of(&mut self, connection: ConnectionId) {
        let Some(name) = self.connection(connection).map(|item| item.name.clone()) else {
            return;
        };
        self.open_structure(connection, Scope::Sessions, RelationRef::default(), name);
    }

    /// Returns the connection of the open tab. With the focus in the navigator, or
    /// without a connection in the tab, the row decides.
    pub(super) fn sessions_connection(&self) -> Option<ConnectionId> {
        let row = self.navigator.current(&self.connections);
        let row = row.map(|row| self.connections[row.connection].id);
        if self.navigator_focused {
            return row;
        }
        self.tab_connection().map(|item| item.id).or(row)
    }

    /// Returns true when the navigator row belongs to a source with sessions.
    pub(super) fn can_open_sessions(&self) -> bool {
        self.navigator
            .current(&self.connections)
            .is_some_and(|row| has_sessions(self.connections[row.connection].dialect))
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
        let Some(connection) = self
            .table()
            .filter(|tab| tab.shows_sessions())
            .map(|tab| tab.connection)
        else {
            return;
        };
        if self.refuse_read_only_signal(connection) {
            return;
        }
        let Some(tab) = self.table() else {
            return;
        };
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

    /// Returns true when the connection is read-only. The status line then gives the reason.
    fn refuse_read_only_signal(&mut self, connection: ConnectionId) -> bool {
        let read_only = self.is_read_only(connection);
        if read_only {
            self.status = Some(text::SESSIONS_READ_ONLY.to_owned());
        }
        read_only
    }

    /// Sends the signal that the user confirmed. The tab must still be on screen, and the
    /// read-only mode can start while the question is open.
    pub(super) fn send_session_signal(
        &mut self,
        console: ConsoleId,
        signal: SessionSignal,
        session: u64,
    ) {
        let Some(connection) = self
            .table()
            .filter(|tab| tab.id == console)
            .map(|tab| tab.connection)
        else {
            return;
        };
        if self.refuse_read_only_signal(connection) {
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
            if self.table_index() == Some(index) {
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
    pub(super) fn open_structure(
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
        let next = self.table().and_then(|tab| {
            let shown = tab.sections();
            (!shown.is_empty()).then(|| {
                let current = tab.section().unwrap_or(shown[0]);
                current.step(shown, forward)
            })
        });
        match next {
            Some(section) => self.show_section(section),
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
        let dialect = self.dialect_of(connection).unwrap_or(Dialect::Sqlite);
        let kafka = dialect == Dialect::Kafka;
        let page_size = if kafka {
            crate::connector::kafka::TOPIC_TAB_MESSAGES
        } else {
            self.settings.page_size
        };
        let topic = kafka && relation.schema == crate::connector::kafka::TOPICS;
        let group = kafka && relation.schema == crate::connector::kafka::GROUPS;
        let mut tab = TableTab::new(id, connection, relation, view, dialect, page_size);
        tab.opened = self.next_tab_place();
        tab.filters = topic
            || self
                .connection(connection)
                .is_none_or(|item| item.capabilities.table_filters);
        tab.filter_only = topic;
        tab.kind_word = match (topic, group) {
            (true, _) => Some("topic"),
            (_, true) => Some("group"),
            _ => None,
        };
        tab
    }

    /// Sends a page to the task of the tab on screen.
    fn run_page(&mut self, request: Option<PageRequest>) {
        if request.is_some() && self.config_blocked() {
            return;
        }
        let (Some(request), Some(index)) = (request, self.table_index()) else {
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
        if self.pending_edit_guard() {
            return None;
        }
        let tab = self.table()?;
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

    pub(super) fn reload_saved_table(&mut self) {
        let Some(dialect) = self.ready_table() else {
            return;
        };
        let budget = self.budget.clone();
        let request = self.table_mut().and_then(|tab| {
            tab.edit_reload = false;
            tab.reload(dialect, &budget)
        });
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
        if self.pending_edit_guard() {
            return;
        }
        if let Some(tab) = self.table_mut() {
            tab.show_page(tab.page().saturating_sub(1));
        }
    }

    /// Shows the page after the cursor. A page that is not on the grid loads first.
    pub(super) fn next_table_page(&mut self) {
        if self.pending_edit_guard() {
            return;
        }
        let Some(tab) = self.table_mut() else { return };
        let page = tab.page() + 1;
        if page < tab.loaded_pages() {
            tab.show_page(page);
        } else {
            self.load_page(Some(page));
        }
    }

    pub(super) fn stop_table(&mut self) {
        let Some(index) = self.table_index() else {
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
        if self.edit_close_guard(index) {
            return;
        }
        self.discard_export(self.tables[index].id);
        self.close_item(Tab::Table(index), |app| &mut app.tables);
    }

    /// Closes a tab that is not a console: its tasks stop, and its rows leave the
    /// budget. When the tab was on screen, the tab at the same place of the tab row
    /// takes the screen.
    pub(super) fn close_item<T: TabItem>(&mut self, tab: Tab, list: fn(&mut Self) -> &mut Vec<T>) {
        let index = match tab {
            Tab::Table(index) | Tab::Diagram(index) | Tab::Compare(index) => index,
            Tab::Console(_) => return,
        };
        if index >= list(self).len() {
            return;
        }
        let place = self.tab_order().iter().position(|item| *item == tab);
        let ids = list(self)[index].console_ids();
        self.cancel_agent_console(ids[0]);
        for id in &ids {
            self.agent.forget_console(*id);
            self.cancel_auth_console(*id);
        }
        let mut item = list(self).remove(index);
        item.release(&self.budget);
        if let Some(manager) = &mut self.manager {
            for id in ids {
                manager.close_console(id);
            }
        }
        let active = match (tab, &mut self.screen) {
            (Tab::Table(_), Screen::Table(active))
            | (Tab::Diagram(_), Screen::Diagram(active))
            | (Tab::Compare(_), Screen::Compare(active)) => Some(active),
            _ => None,
        };
        match active {
            Some(active) if *active == index => {
                self.screen = Screen::Console;
                let order = self.tab_order();
                if order.is_empty() {
                    self.focus_navigator();
                } else if let Some(place) = place {
                    self.select_tab(place.min(order.len() - 1));
                }
            }
            Some(active) if *active > index => *active -= 1,
            _ => {}
        }
    }

    /// Closes the tabs that use a connection that the user removed.
    pub(super) fn close_tabs_using(&mut self, connection: ConnectionId) {
        for index in (0..self.tables.len()).rev() {
            if self.tables[index].uses(connection) {
                self.close_table(index);
            }
        }
        for index in (0..self.diagrams.len()).rev() {
            if self.diagrams[index].uses(connection) {
                self.close_diagram(index);
            }
        }
        for index in (0..self.compares.len()).rev() {
            if self.compares[index].uses(connection) {
                self.close_compare(index);
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
            | Action::RunSubquery
            | Action::Explain
            | Action::ExplainAnalyze
            | Action::ToggleMode
            | Action::Commit
            | Action::Rollback
            | Action::ChooseSchema
            | Action::RenameConsole
            | Action::ChangeConnection
            | Action::GridCloseResult => {}
            Action::GridEditCell => self.edit_cell(false),
            Action::GridSetNull => self.edit_cell(true),
            Action::GridNewRow => self.new_row(),
            Action::GridDeleteRow => self.toggle_delete_row(),
            Action::GridReviewEdits | Action::GridSaveEdits => self.review_edits(),
            Action::GridDiscardEdits => self.confirm_discard_edits(),
            Action::GridApply => self.apply_table(),
            Action::GridClearFilters => self.clear_table(),
            Action::GridPreviousResult => self.step_table(false),
            Action::GridNextResult => self.step_table(true),
            Action::GridStructure => self.open_table_structure(),
            Action::GridReload if self.table().is_some_and(|tab| !tab.filters) => {
                self.apply_table();
            }
            Action::GridFollowKey | Action::FollowKey => self.follow_key(),
            Action::GridCancelQuery => self.ask_session_signal(SessionSignal::CancelQuery),
            Action::GridEndSession => self.ask_session_signal(SessionSignal::EndSession),
            Action::Stop => self.stop_table(),
            Action::CloseTab => {
                if let Some(index) = self.table_index() {
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
            tab.focus = match focus {
                _ if !tab.filters => TableFocus::Grid,
                TableFocus::Order if tab.filter_only => TableFocus::Filter,
                focus => focus,
            };
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
        let Some(index) = self.table_index() else {
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
        let catalog = App::catalog_of(connections, connection);
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
            let catalog = App::catalog_of(connections, connection);
            let missing = tables[index].refresh_list(dialect, catalog);
            self.request_relation_columns(connection, missing);
        }
    }
}

/// Lists the tabs of one kind with their places in the order of opening.
fn opened<T: TabItem>(items: &[T], tab: fn(usize) -> Tab) -> impl Iterator<Item = (u64, Tab)> {
    items
        .iter()
        .enumerate()
        .map(move |(index, item)| (item.opened(), tab(index)))
}

/// Returns the label, the phase, and the run of a tab that is not a console.
fn item_view<'a>(item: &'a dyn TabItem, theme: &Theme) -> (String, Phase, &'a RunClock) {
    (item.label(theme), item.phase(), item.run())
}
