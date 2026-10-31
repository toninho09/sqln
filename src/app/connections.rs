//! Connections, their catalogs, the navigator actions, and the form.

use std::sync::Arc;

use super::{App, DialogAction, Picker};
use crate::catalog::{Catalog, Columns, RelationRef};
use crate::connection::{CatalogState, Connection, Definition, Source, name_is_taken};
use crate::connector::sqlite::{SqliteTarget, validate_path};
use crate::connector::{Connector, Namespace};
use crate::console::Focus;
use crate::exec::{ConnectionId, Manager};
use crate::form::{ConnectionForm, Field, FormNote, Kind};
use crate::input::Action;
use crate::navigator::{Navigator, Row, RowKind};
use crate::ui::text;

impl App {
    /// Starts the manager at its first use, so a start without a database has no runtime.
    fn ensure_manager(&mut self) -> bool {
        if self.manager.is_some() {
            return true;
        }
        match Manager::new(self.budget.clone()) {
            Ok((manager, events)) => {
                self.manager = Some(manager);
                self.events = Some(events);
                true
            }
            Err(error) => {
                self.status = Some(error.to_string());
                false
            }
        }
    }

    pub(super) fn add_definition(&mut self, definition: Definition) -> Option<ConnectionId> {
        let name = definition.name.clone();
        let connector = definition.connector();
        self.add_connection(name, Some(definition), connector)
    }

    pub(super) fn add_connection(
        &mut self,
        name: String,
        definition: Option<Definition>,
        connector: Arc<dyn Connector>,
    ) -> Option<ConnectionId> {
        if !self.ensure_manager() {
            return None;
        }
        let settings = match &definition {
            Some(definition) => definition.settings(&self.settings),
            None => self.settings.clone(),
        };
        let manager = self.manager.as_mut()?;
        let id = manager.add_connection(connector.clone(), settings);
        self.connections.push(Connection::new(id, name, definition, connector.as_ref()));
        Some(id)
    }

    pub(super) fn prepare_database(&mut self) {
        let Some(path) = self.database.clone() else { return };
        if let Err(error) = validate_path(&path) {
            self.status = Some(error.0);
            self.database = None;
            return;
        }
        if path.exists() {
            self.add_sqlite(SqliteTarget::new(&path));
        } else {
            self.show_dialog(
                text::TITLE_CREATE,
                text::create_database(&path.display().to_string()),
                vec![
                    (text::OPTION_CANCEL, DialogAction::Dismiss),
                    (text::OPTION_CREATE, DialogAction::CreateDatabase),
                ],
            );
        }
    }

    /// Adds the file of the command line. It follows the limits of the settings.
    fn add_sqlite(&mut self, target: SqliteTarget) -> Option<ConnectionId> {
        let mut definition = Definition::sqlite(target);
        definition.max_connections = self.settings.max_connections;
        definition.connect_timeout = self.settings.connect_timeout;
        self.add_definition(definition)
    }

    pub(super) fn create_database(&mut self) {
        let Some(path) = self.database.clone() else { return };
        let mut target = SqliteTarget::new(&path);
        target.allow_create = true;
        if let Some(connection) = self.add_sqlite(target)
            && self.console().connection.is_none()
        {
            self.bind_console(self.active, connection);
        }
    }

    /// Gives a console a task of the connection. The console must have no connection.
    pub(super) fn bind_console(&mut self, index: usize, connection: ConnectionId) {
        let Some(manager) = &mut self.manager else { return };
        let Some(id) = manager.open_console(connection) else { return };
        let console = &mut self.consoles[index];
        console.id = id;
        console.connection = Some(connection);
    }

    fn connection_index(&self, id: ConnectionId) -> Option<usize> {
        self.connections.iter().position(|connection| connection.id == id)
    }

    /// Returns the connection whose catalog the console reads, and it sets the schema
    /// of that console as the current schema of the catalog.
    pub(super) fn catalog_for(&mut self, index: usize) -> Option<ConnectionId> {
        let schema = self.consoles[index].schema.clone();
        let id = self.consoles[index].connection?;
        let connection = self.connection_mut(id)?;
        let current = schema.or_else(|| connection.default_schema.clone());
        if let Some(catalog) = &mut connection.catalog {
            catalog.current_schema = current;
        }
        Some(id)
    }

    pub(super) fn ensure_active_catalog(&mut self) {
        if let Some(id) = self.console().connection {
            self.ensure_catalog(id);
        }
    }

    /// Reads the catalog at its first use, so the start opens no connection.
    fn ensure_catalog(&mut self, id: ConnectionId) {
        let not_loaded = self
            .connection(id)
            .is_some_and(|connection| connection.catalog_state == CatalogState::NotLoaded);
        if not_loaded {
            self.reload_catalog(id);
        }
    }

    pub(super) fn reload_catalog(&mut self, id: ConnectionId) {
        let Some(manager) = &self.manager else { return };
        manager.load_catalog(id);
        if let Some(connection) = self.connection_mut(id) {
            connection.catalog_state = CatalogState::Loading;
        }
    }

    pub(super) fn on_catalog_loaded(&mut self, id: ConnectionId, catalog: Result<Catalog, String>) {
        let schemas = match catalog {
            Ok(catalog) => {
                let schemas: Vec<String> = catalog.schemas.iter().map(|item| item.name.clone()).collect();
                if let Some(connection) = self.connection_mut(id) {
                    connection.set_catalog(catalog);
                }
                for relation in self.navigator.open_relations(id) {
                    self.request_columns(id, relation);
                }
                self.refresh_consoles(id);
                Some(schemas)
            }
            Err(message) => {
                if let Some(connection) = self.connection_mut(id) {
                    connection.catalog_state = CatalogState::Failed;
                }
                self.status = Some(message);
                None
            }
        };
        self.update_picker(id, schemas);
    }

    pub(super) fn on_columns_loaded(
        &mut self,
        id: ConnectionId,
        relation: &RelationRef,
        columns: Result<Vec<String>, String>,
    ) {
        let Some(connection) = self.connection_mut(id) else { return };
        connection.requested_columns.retain(|requested| requested != relation);
        match columns {
            Ok(columns) => {
                if let Some(catalog) = &mut connection.catalog {
                    catalog.set_columns(relation, columns);
                }
                self.refresh_consoles(id);
            }
            Err(message) => self.status = Some(message),
        }
    }

    /// Gives the new catalog to the open lists of the consoles of a connection.
    fn refresh_consoles(&mut self, id: ConnectionId) {
        for index in 0..self.consoles.len() {
            if self.consoles[index].connection == Some(id) {
                self.catalog_for(index);
                let catalog = self.connections.iter().find(|item| item.id == id);
                let catalog = catalog.and_then(|item| item.catalog.as_ref());
                self.consoles[index].editor.refresh_suggestions(catalog);
                self.request_missing_columns(index);
            }
        }
    }

    pub(super) fn request_missing_columns(&mut self, index: usize) {
        let Some(id) = self.consoles[index].connection else { return };
        for relation in self.consoles[index].editor.take_missing_columns() {
            self.request_columns(id, relation);
        }
    }

    fn request_columns(&mut self, id: ConnectionId, relation: RelationRef) {
        let Some(manager) = &self.manager else { return };
        let Some(connection) = self.connections.iter_mut().find(|item| item.id == id) else { return };
        if !connection.requested_columns.contains(&relation) {
            manager.load_columns(id, relation.clone());
            connection.requested_columns.push(relation);
        }
    }

    pub(super) fn on_navigator_action(&mut self, action: Action) {
        if action == Action::NavigatorAdd {
            self.form = Some(ConnectionForm::new());
            return;
        }
        let Some(row) = self.navigator.current(&self.connections) else {
            self.status = Some(text::NO_SELECTED_CONNECTION.to_owned());
            return;
        };
        let id = self.connections[row.connection].id;
        match action {
            Action::NavigatorUp => self.navigator.move_by(-1, &self.connections),
            Action::NavigatorDown => self.navigator.move_by(1, &self.connections),
            Action::NavigatorCollapse => self.collapse(&row),
            Action::NavigatorExpand => self.expand(&row),
            Action::NavigatorOpen if row.kind == RowKind::Connection => self.open_console(id),
            Action::NavigatorOpen => self.toggle(&row),
            Action::NavigatorNewConsole => self.new_console(id),
            Action::NavigatorRefresh => self.reload_catalog(id),
            Action::NavigatorDisconnect => self.disconnect(id),
            Action::NavigatorEdit => self.edit_connection(id),
            Action::NavigatorDelete => self.confirm_removal(id),
            _ => {}
        }
    }

    pub(super) fn toggle(&mut self, row: &Row) {
        if self.navigator.is_open(row, &self.connections) {
            self.navigator.collapse(row, &self.connections);
        } else {
            self.expand(row);
        }
    }

    fn expand(&mut self, row: &Row) {
        if !Navigator::is_branch(row) {
            return;
        }
        if self.navigator.is_open(row, &self.connections) {
            self.navigator.move_by(1, &self.connections);
            return;
        }
        self.navigator.expand(row, &self.connections);
        let id = self.connections[row.connection].id;
        match row.kind {
            RowKind::Connection => self.ensure_catalog(id),
            RowKind::Table | RowKind::View => self.read_columns(row, id),
            _ => {}
        }
    }

    fn read_columns(&mut self, row: &Row, id: ConnectionId) {
        let Some(relation) = row.relation_ref(&self.connections) else { return };
        let catalog = self.connections[row.connection].catalog.as_ref();
        let found = catalog.and_then(|catalog| catalog.relation(Some(&relation.schema), &relation.relation));
        if found.is_some_and(|(_, item)| item.columns == Columns::NotLoaded) {
            self.request_columns(id, relation);
        }
    }

    fn collapse(&mut self, row: &Row) {
        if self.navigator.is_open(row, &self.connections) {
            self.navigator.collapse(row, &self.connections);
        } else {
            self.navigator.select_parent(row, &self.connections);
        }
    }

    /// Selects a console of the connection, or it opens the first one.
    fn open_console(&mut self, id: ConnectionId) {
        match self.consoles.iter().position(|console| console.connection == Some(id)) {
            Some(index) => {
                self.select_console(index);
                self.leave_navigator();
            }
            None => self.new_console(id),
        }
    }

    fn leave_navigator(&mut self) {
        self.navigator_focused = false;
        self.console_mut().focus = Focus::Editor;
    }

    /// Opens a console. An empty console without a connection takes the connection,
    /// so that the first use does not leave an extra tab.
    pub(super) fn new_console(&mut self, id: ConnectionId) {
        let Some(dialect) = self.connection(id).map(|connection| connection.dialect) else { return };
        let active = self.console();
        if active.connection.is_some() || !active.editor.text().is_empty() {
            let console = self.unbound_console(dialect);
            self.consoles.push(console);
            self.select_console(self.consoles.len() - 1);
        }
        self.console_mut().editor.set_dialect(dialect);
        self.bind_console(self.active, id);
        self.leave_navigator();
    }

    /// Returns the name of a console that must end its work first.
    fn console_in_use(&self, id: ConnectionId) -> Option<&str> {
        self.consoles
            .iter()
            .filter(|console| console.connection == Some(id))
            .find(|console| console.is_busy() || console.in_transaction())
            .map(|console| console.name.as_str())
    }

    fn disconnect(&mut self, id: ConnectionId) {
        if let Some(console) = self.console_in_use(id) {
            self.status = Some(text::connection_in_use(console));
            return;
        }
        let Some(manager) = &self.manager else { return };
        if manager.is_connected(id) {
            manager.disconnect(id);
        } else {
            self.status = Some(text::NOT_CONNECTED.to_owned());
        }
    }

    pub(super) fn on_disconnected(&mut self, id: ConnectionId) {
        let connected = self.manager.as_ref().is_some_and(|manager| manager.is_connected(id));
        if connected {
            self.status = Some(text::SESSION_IN_USE.to_owned());
            return;
        }
        self.status = Some(text::DISCONNECTED.to_owned());
        let definition = self.connection(id).and_then(|item| item.definition.clone());
        if let (Some(form), Some(definition)) = (&mut self.form, definition)
            && form.editing == Some(id)
            && form.name_only
        {
            *form = ConnectionForm::editing(id, &definition, false);
        }
    }

    fn edit_connection(&mut self, id: ConnectionId) {
        let Some(definition) = self.connection(id).and_then(|item| item.definition.as_ref()) else {
            self.status = Some(text::NO_DEFINITION.to_owned());
            return;
        };
        let connected = self.manager.as_ref().is_some_and(|manager| manager.is_connected(id));
        let mut form = ConnectionForm::editing(id, definition, connected);
        if connected {
            form.note = Some(FormNote::Info(text::FORM_CONNECTED.to_owned()));
        }
        self.form = Some(form);
    }

    fn confirm_removal(&mut self, id: ConnectionId) {
        if let Some(console) = self.console_in_use(id) {
            self.status = Some(text::connection_in_use(console));
            return;
        }
        let Some(connection) = self.connection(id) else { return };
        let body = text::remove_connection(&connection.name);
        self.show_dialog(
            text::TITLE_REMOVE_CONNECTION,
            body,
            vec![
                (text::OPTION_CANCEL, DialogAction::Dismiss),
                (text::OPTION_REMOVE, DialogAction::RemoveConnection(id)),
            ],
        );
    }

    pub(super) fn remove_connection(&mut self, id: ConnectionId) {
        // Another console can start to work while the question stays open.
        if let Some(console) = self.console_in_use(id) {
            self.status = Some(text::connection_in_use(console));
            return;
        }
        for index in 0..self.consoles.len() {
            if self.consoles[index].connection == Some(id) {
                self.unbind_console(index);
            }
        }
        if let Some(manager) = &mut self.manager {
            manager.remove_connection(id);
        }
        self.connections.retain(|connection| connection.id != id);
        self.navigator.forget(id);
    }

    /// The console keeps its text and its results, and it cannot run.
    fn unbind_console(&mut self, index: usize) {
        let id = self.next_unbound_console;
        self.next_unbound_console -= 1;
        let console = &mut self.consoles[index];
        if let Some(manager) = &mut self.manager {
            manager.close_console(console.id);
        }
        console.id = id;
        console.connection = None;
        console.schema = None;
    }

    pub(super) fn open_picker(&mut self) {
        let Some(connection) = self.active_connection() else {
            self.status = Some(text::NO_CONNECTION.to_owned());
            return;
        };
        let title = match connection.capabilities.namespace {
            Namespace::None => {
                self.status = Some(text::NO_SCHEMA_CHOICE.to_owned());
                return;
            }
            Namespace::Schema => text::TITLE_SCHEMA,
            Namespace::Database => text::TITLE_DATABASE,
        };
        let id = connection.id;
        let items = connection.catalog.as_ref().map(|catalog| {
            catalog.schemas.iter().map(|schema| schema.name.clone()).collect::<Vec<_>>()
        });
        let console = self.console();
        if console.is_busy() || console.in_transaction() {
            self.status = Some(text::SCHEMA_BUSY.to_owned());
            return;
        }
        let selected = console.schema.clone();
        let loading = items.is_none();
        let items = items.unwrap_or_default();
        let cursor = selected
            .and_then(|schema| items.iter().position(|item| *item == schema))
            .map_or(0, |index| index + 1);
        self.picker = Some(Picker { console: console.id, connection: id, title, items, cursor, loading });
        if loading {
            self.ensure_catalog(id);
            let failed = self.connection(id).is_some_and(|item| item.catalog_state == CatalogState::Failed);
            if failed {
                self.reload_catalog(id);
            }
        }
    }

    fn update_picker(&mut self, id: ConnectionId, schemas: Option<Vec<String>>) {
        let Some(picker) = self.picker.as_mut().filter(|picker| picker.connection == id) else {
            return;
        };
        match schemas {
            Some(schemas) => {
                picker.items = schemas;
                picker.cursor = picker.cursor.min(picker.items.len());
                picker.loading = false;
            }
            None => self.picker = None,
        }
    }

    pub(super) fn accept_picker(&mut self) {
        let Some(picker) = self.picker.take() else { return };
        let schema = picker.cursor.checked_sub(1).and_then(|index| picker.items.get(index)).cloned();
        if let Some(console) = self.console_by_id(picker.console) {
            console.schema = schema;
        }
    }

    pub(super) fn test_form(&mut self) {
        let Some(form) = &self.form else { return };
        let definition = form.definition();
        let form_note = |app: &mut Self, note| {
            if let Some(form) = &mut app.form {
                form.note = Some(note);
            }
        };
        let definition = match definition {
            Ok(definition) => definition,
            Err(message) => {
                form_note(self, FormNote::Error(message));
                return;
            }
        };
        if form.kind == Kind::Sqlite && !form_file_exists(&definition) {
            form_note(self, FormNote::Error(text::FORM_FILE_MISSING.to_owned()));
            return;
        }
        if !self.ensure_manager() {
            return;
        }
        let test = self.next_test;
        self.next_test += 1;
        if let Some(manager) = &self.manager {
            manager.test_connection(test, definition.connector(), definition.connect_timeout);
        }
        if let Some(form) = &mut self.form {
            form.test = Some(test);
            form.note = Some(FormNote::Info(text::FORM_TESTING.to_owned()));
        }
    }

    pub(super) fn on_connection_tested(&mut self, test: u64, result: Result<(), String>) {
        let Some(form) = self.form.as_mut().filter(|form| form.test == Some(test)) else { return };
        form.test = None;
        form.note = Some(match result {
            Ok(()) => FormNote::Info(text::FORM_TEST_OK.to_owned()),
            Err(message) => FormNote::Error(message),
        });
    }

    pub(super) fn disconnect_form(&mut self) {
        let Some(id) = self.form.as_ref().and_then(|form| form.editing) else { return };
        if let Some(console) = self.console_in_use(id) {
            let note = FormNote::Error(text::connection_in_use(console));
            if let Some(form) = &mut self.form {
                form.note = Some(note);
            }
            return;
        }
        if let Some(manager) = &self.manager {
            manager.disconnect(id);
        }
    }

    pub(super) fn save_form(&mut self) {
        let Some(form) = &self.form else { return };
        let editing = form.editing;
        if form.name_only {
            let name = form.text(Field::Name).trim().to_owned();
            self.rename_connection(name);
            return;
        }
        let definition = match form.definition() {
            Ok(definition) => definition,
            Err(message) => {
                self.set_form_error(message);
                return;
            }
        };
        if name_is_taken(&self.connections, &definition.name, editing) {
            self.set_form_error(text::FORM_NAME_TAKEN.to_owned());
            return;
        }
        if let Source::Sqlite(target) = &definition.source
            && !target.path.exists()
            && !target.allow_create
        {
            let path = target.path.display().to_string();
            self.show_dialog(
                text::TITLE_CREATE,
                text::create_database(&path),
                vec![
                    (text::OPTION_CANCEL, DialogAction::Dismiss),
                    (text::OPTION_CREATE, DialogAction::CreateFormFile),
                ],
            );
            return;
        }
        self.apply_definition(definition);
    }

    pub(super) fn create_form_file(&mut self) {
        let definition = self.form.as_ref().map(ConnectionForm::definition);
        match definition {
            Some(Ok(mut definition)) => {
                if let Source::Sqlite(target) = &mut definition.source {
                    target.allow_create = true;
                }
                self.apply_definition(definition);
            }
            Some(Err(message)) => self.set_form_error(message),
            None => {}
        }
    }

    fn set_form_error(&mut self, message: String) {
        if let Some(form) = &mut self.form {
            form.note = Some(FormNote::Error(message));
        }
    }

    fn rename_connection(&mut self, name: String) {
        let Some(id) = self.form.as_ref().and_then(|form| form.editing) else { return };
        if name.is_empty() {
            self.set_form_error(text::FORM_NEEDS_NAME.to_owned());
            return;
        }
        if name_is_taken(&self.connections, &name, Some(id)) {
            self.set_form_error(text::FORM_NAME_TAKEN.to_owned());
            return;
        }
        if let Some(connection) = self.connection_mut(id) {
            connection.name.clone_from(&name);
            if let Some(definition) = &mut connection.definition {
                definition.name = name;
            }
        }
        self.form = None;
    }

    fn apply_definition(&mut self, definition: Definition) {
        match self.form.as_ref().and_then(|form| form.editing) {
            Some(id) => self.replace_definition(id, definition),
            None => {
                if let Some(id) = self.add_definition(definition) {
                    self.navigator.select_connection(id);
                    self.navigator_focused = !self.navigator.collapsed;
                }
            }
        }
        self.form = None;
    }

    /// Applies a changed definition. A session never survives a change of access.
    fn replace_definition(&mut self, id: ConnectionId, definition: Definition) {
        let connected = self.manager.as_ref().is_some_and(|manager| manager.is_connected(id));
        if connected {
            self.set_form_error(text::FORM_CONNECTED.to_owned());
            return;
        }
        let connector = definition.connector();
        let settings = definition.settings(&self.settings);
        if let Some(manager) = &mut self.manager {
            manager.replace_connection(id, connector.clone(), settings);
        }
        let Some(index) = self.connection_index(id) else { return };
        let name = definition.name.clone();
        let dialect = connector.dialect();
        self.connections[index] = Connection::new(id, name, Some(definition), connector.as_ref());
        self.navigator.forget(id);
        for console in self.consoles.iter_mut().filter(|console| console.connection == Some(id)) {
            console.editor.set_dialect(dialect);
            console.schema = None;
        }
    }
}

fn form_file_exists(definition: &Definition) -> bool {
    match &definition.source {
        Source::Sqlite(target) => target.path.exists(),
        Source::Network(_) => true,
    }
}
