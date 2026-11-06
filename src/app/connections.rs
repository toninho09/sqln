//! Connections, their catalogs, the navigator actions, and the form.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent};

use super::{App, DialogAction, Picker, PickerKind, SqlRequest};
use crate::catalog::{Catalog, Columns, ForeignKey, RelationKind, RelationRef};
use crate::connection::{
    CatalogState, Connection, ConnectionKey, Definition, Source, name_is_taken, refresh_labels,
};
use crate::connector::sqlite::{SqliteTarget, validate_path};
use crate::connector::{ConnectError, Connector, Namespace};
use crate::console::Focus;
use crate::creds::Identity;
use crate::exec::{ConnectionId, Manager, Settings};
use crate::form::{ConnectionForm, Field, FormNote, Invalid};
use crate::generate::{Template, generate, qualified_name};
use crate::grid::printable;
use crate::input::Action;
use crate::navigator::{Navigator, Row, RowKind, is_relation};
use crate::sql::Dialect;
use crate::store::Scope;
use crate::store::config::{Change, ConnectionIcon, CredentialName, ThemeName};
use crate::table::TableTab;
use crate::ui::text;

impl App {
    /// Starts the manager at its first use, so a start without a database has no runtime.
    pub(super) fn ensure_manager(&mut self) -> bool {
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
        let settings = self.connection_settings(definition.as_ref());
        let manager = self.manager.as_mut()?;
        let id = manager.add_connection(connector.clone(), settings);
        self.connections
            .push(Connection::new(id, name, definition, connector.as_ref()));
        Some(id)
    }

    /// Returns the limits of a connection: its definition on top of the settings.
    fn connection_settings(&self, definition: Option<&Definition>) -> Settings {
        definition.map_or_else(
            || self.settings.clone(),
            |definition| definition.settings(&self.settings),
        )
    }

    /// Gives the saved settings to each connection. A running execution keeps its values.
    pub(super) fn update_connection_settings(&self) {
        let Some(manager) = &self.manager else {
            return;
        };
        for connection in &self.connections {
            let settings = self.connection_settings(connection.definition.as_ref());
            manager.update_settings(connection.id, settings);
        }
    }

    pub(super) fn prepare_database(&mut self) {
        let Some(path) = self.database.clone() else {
            return;
        };
        if let Err(error) = validate_path(&path) {
            self.status = Some(error.message);
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
        definition.sql_timeout = self.settings.sql_timeout;
        definition.idle_timeout = self.settings.idle_timeout;
        self.add_definition(definition)
    }

    pub(super) fn create_database(&mut self) {
        let Some(path) = self.database.clone() else {
            return;
        };
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
        let Some(manager) = &mut self.manager else {
            return;
        };
        let Some(id) = manager.open_console(connection) else {
            return;
        };
        // The name comes first, while the console has no connection to count. An agent tab
        // keeps the name of its agent.
        let previous = self.consoles[index].id;
        let fresh = (self.persist.is_some()
            && self.consoles[index].file.is_none()
            && !self.agent.owns(previous))
        .then(|| self.fresh_console_name(connection));
        let console = &mut self.consoles[index];
        console.id = id;
        console.connection = Some(connection);
        if let Some(name) = fresh {
            console.name = name;
        }
        self.agent.rekey_console(previous, id);
        self.rekey_saves(previous, id);
        self.refresh_tree_consoles();
    }

    /// Returns `console-N` with the lowest number that no console of the connection uses,
    /// open or saved.
    fn fresh_console_name(&self, connection: ConnectionId) -> String {
        let mut used: Vec<String> = self
            .consoles
            .iter()
            .filter(|console| console.connection == Some(connection))
            .map(|console| console.name.clone())
            .collect();
        used.extend(
            self.consoles_of(connection)
                .into_iter()
                .map(|entry| entry.meta.name),
        );
        (1..)
            .map(text::console_name)
            .find(|name| !used.contains(name))
            .unwrap_or_default()
    }

    fn connection_index(&self, id: ConnectionId) -> Option<usize> {
        self.connections
            .iter()
            .position(|connection| connection.id == id)
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
    pub(super) fn ensure_catalog(&mut self, id: ConnectionId) {
        let not_loaded = self
            .connection(id)
            .is_some_and(|connection| connection.catalog_state == CatalogState::NotLoaded);
        if not_loaded {
            self.reload_catalog(id);
        }
    }

    pub(super) fn reload_catalog(&mut self, id: ConnectionId) {
        self.invalidate_ai_schema(id);
        if self.config_blocked() {
            return;
        }
        self.catalog_reads += 1;
        let read = self.catalog_reads;
        if let Some(connection) = self.connection_mut(id) {
            connection.catalog_read = read;
            connection.catalog_state = CatalogState::Loading;
        }
        self.note_ai_catalog_read(id);
        self.authorize(id, super::auth::Use::Catalog(id, read));
    }

    pub(super) fn on_catalog_loaded(
        &mut self,
        id: ConnectionId,
        read: u64,
        catalog: Result<Catalog, String>,
    ) {
        // Reads can overlap, and an older read can miss the result of a DDL statement.
        if self
            .connection(id)
            .is_none_or(|connection| connection.catalog_read != read)
        {
            return;
        }
        let schemas = match catalog {
            Ok(catalog) => {
                let schemas: Vec<String> = catalog
                    .schemas
                    .iter()
                    .map(|item| item.name.clone())
                    .collect();
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
        if schemas.is_some() {
            self.refresh_ai_schema(id);
        }
        self.open_pending_diagram(id, schemas.is_some());
        self.update_picker(id, schemas);
        self.sync_compare_form();
    }

    pub(super) fn on_columns_loaded(
        &mut self,
        id: ConnectionId,
        relation: &RelationRef,
        columns: Result<Vec<String>, String>,
        (foreign_keys, referencing): (Vec<ForeignKey>, Vec<ForeignKey>),
    ) {
        let Some(connection) = self.connection_mut(id) else {
            return;
        };
        connection
            .requested_columns
            .retain(|requested| requested != relation);
        match columns {
            Ok(columns) => {
                if let Some(catalog) = &mut connection.catalog {
                    catalog.set_columns(relation, columns, foreign_keys, referencing);
                }
                self.refresh_consoles(id);
            }
            Err(message) => {
                self.status = Some(message);
                self.pending_sql = None;
                self.pending_follow = None;
            }
        }
        self.continue_follow(id, relation);
        let same = |(request, _): &mut (SqlRequest, Template)| {
            request.connection == id && request.relation == *relation
        };
        if let Some((request, template)) = self.pending_sql.take_if(same) {
            self.write_sql(&request, template);
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
        self.refresh_tables(id);
    }

    pub(super) fn request_missing_columns(&mut self, index: usize) {
        let Some(id) = self.consoles[index].connection else {
            return;
        };
        for relation in self.consoles[index].editor.take_missing_columns() {
            self.request_columns(id, relation);
        }
    }

    pub(super) fn request_columns(&mut self, id: ConnectionId, relation: RelationRef) {
        if self
            .persist
            .as_ref()
            .is_some_and(|persist| persist.error.is_some())
        {
            return;
        }
        let Some(connection) = self.connections.iter_mut().find(|item| item.id == id) else {
            return;
        };
        if !connection.requested_columns.contains(&relation) {
            connection.requested_columns.push(relation.clone());
            self.authorize(id, super::auth::Use::Columns(id, relation));
        }
    }

    pub(super) fn on_navigator_action(&mut self, action: Action) {
        if action == Action::NavigatorAdd {
            if !self.config_blocked() {
                self.form = Some(ConnectionForm::new_in(self.has_project()));
            }
            return;
        }
        if action == Action::NavigatorScripts {
            self.open_orphans();
            return;
        }
        if action == Action::NavigatorFilter {
            self.focus_navigator();
            if self.navigator_focused {
                self.navigator.open_filter();
            }
            return;
        }
        if action == Action::NavigatorClearFilter {
            self.navigator.clear_filter(&self.connections);
            self.refresh_tree_consoles();
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
            Action::NavigatorOpen if row.kind == RowKind::Console => self.open_tree_console(&row),
            Action::NavigatorOpen if is_relation(&row) => self.open_table(&row),
            Action::NavigatorOpen if matches!(row.kind, RowKind::Object(_)) => {
                self.open_row_definition(&row);
            }
            Action::NavigatorOpen => self.toggle(&row),
            Action::NavigatorNewConsole => self.request_console(id),
            Action::NavigatorStructure => self.open_row_structure(&row),
            Action::NavigatorDiagram => self.open_row_diagram(&row),
            Action::NavigatorCompare => self.open_row_compare(&row),
            Action::NavigatorSessions => self.open_row_sessions(&row),
            Action::NavigatorGenerate => self.open_templates(&row),
            Action::NavigatorImport => self.import_row(&row),
            Action::NavigatorCopyName => {
                let object = row.object(&self.connections);
                let name = object.map_or_else(|| row.name(&self.connections), |item| &item.name);
                let name = name.to_owned();
                self.copy(&name);
            }
            Action::NavigatorRefresh if row.kind == RowKind::Console => {
                self.rename_tree_console(&row);
            }
            Action::NavigatorRefresh => {
                self.refresh_tree_consoles();
                if !self.ask_new_password(id) {
                    self.reload_catalog(id);
                }
            }
            Action::NavigatorDeletePassword => self.confirm_password_deletion(id),
            Action::NavigatorDisconnect => self.disconnect(id),
            Action::NavigatorEdit => self.edit_connection(id),
            Action::NavigatorDelete if row.kind == RowKind::Console => {
                self.delete_tree_console(&row);
            }
            Action::NavigatorDelete => self.confirm_removal(id),
            _ => {}
        }
    }

    /// Returns the request of the templates of a row: a table, a view, or the table of a
    /// column, with SQL text.
    fn row_templates(&self, row: &Row) -> Option<(SqlRequest, &'static [Template])> {
        let item = &self.connections[row.connection];
        if item.dialect == Dialect::Redis
            || matches!(row.kind, RowKind::Connection | RowKind::Schema)
        {
            return None;
        }
        let relation = row.relation_ref(&self.connections)?;
        let found = item
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.relation(Some(&relation.schema), &relation.relation));
        let view = found.is_some_and(|(_, found)| found.kind == RelationKind::View);
        let templates: &[Template] = if view {
            &Template::READS
        } else {
            &Template::ALL
        };
        let request = SqlRequest {
            connection: item.id,
            relation,
        };
        Some((request, templates))
    }

    /// Returns true when the row of the navigator has templates or a DDL.
    pub(super) fn can_generate(&self) -> bool {
        self.navigator
            .current(&self.connections)
            .is_some_and(|row| {
                self.row_templates(&row).is_some() || self.row_definition(&row).is_some()
            })
    }

    /// Opens the list of SQL templates of a table, of a view, or of the table of a column,
    /// with the DDL entry last. An object has only the DDL entry.
    fn open_templates(&mut self, row: &Row) {
        if !self.can_generate() || self.config_blocked() {
            return;
        }
        let (request, templates) = match self.row_templates(row) {
            Some((request, templates)) => (Some(request), templates.to_vec()),
            None => (None, Vec::new()),
        };
        let definition = self.row_definition(row);
        let mut items: Vec<String> = templates
            .iter()
            .map(|template| text::template_name(*template).to_owned())
            .collect();
        if definition.is_some() {
            items.push(text::TEMPLATE_DDL.to_owned());
        }
        self.picker = Some(Picker {
            kind: PickerKind::Generate {
                request,
                templates,
                definition,
            },
            title: text::TITLE_GENERATE.to_owned(),
            items,
            cursor: 0,
            loading: false,
        });
    }

    /// Writes a template. A template with columns first reads them.
    fn start_template(&mut self, request: SqlRequest, template: Template) {
        let found = self
            .connection(request.connection)
            .and_then(|item| item.catalog.as_ref())
            .and_then(|catalog| {
                catalog.relation(Some(&request.relation.schema), &request.relation.relation)
            });
        let missing = found.is_some_and(|(_, found)| found.columns == Columns::NotLoaded);
        if template.needs_columns() && missing {
            let (connection, relation) = (request.connection, request.relation.clone());
            self.pending_sql = Some((request, template));
            self.request_columns(connection, relation);
        } else {
            self.write_sql(&request, template);
        }
    }

    /// Selects the console of the connection, and inserts the text at its cursor. It does
    /// not run the text, as the query history does not.
    fn write_sql(&mut self, request: &SqlRequest, template: Template) {
        let SqlRequest {
            connection,
            relation,
        } = request;
        let index = if self.console().connection == Some(*connection) {
            Some(self.active)
        } else {
            self.consoles
                .iter()
                .position(|console| console.connection == Some(*connection))
        };
        let columns = self
            .connection(*connection)
            .and_then(|item| item.catalog.as_ref())
            .and_then(|catalog| catalog.relation(Some(&relation.schema), &relation.relation))
            .and_then(|(_, found)| match &found.columns {
                Columns::Loaded(columns) => Some(columns.clone()),
                Columns::NotLoaded => None,
            })
            .unwrap_or_default();
        if template.needs_columns() && columns.is_empty() {
            self.status = Some(text::NO_COLUMNS_TO_LIST.to_owned());
            return;
        }
        match index {
            Some(index) => self.select_console(index),
            None => self.new_console(*connection),
        }
        self.catalog_for(self.active);
        let Some(item) = self.connection(*connection) else {
            return;
        };
        let current = item.catalog.as_ref().is_some_and(|catalog| {
            catalog
                .schema(&relation.schema)
                .is_some_and(|schema| catalog.is_current(schema))
        });
        let schema = (!current).then_some(relation.schema.as_str());
        let name = qualified_name(schema, &relation.relation, item.dialect);
        let sql = generate(template, item.dialect, &name, &columns);
        self.leave_navigator();
        self.on_paste(&sql);
    }

    /// Writes in the field of the filter of the tree. The tree under the field changes
    /// with each key, and `Enter` opens the table of the cursor.
    pub(super) fn on_tree_filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.navigator.cancel_filter(),
            KeyCode::Tab => self.navigator.leave_filter_field(),
            KeyCode::Up => self.step_filtered(false),
            KeyCode::Down => self.step_filtered(true),
            KeyCode::Enter => self.accept_tree_filter(),
            _ => self.navigator.on_filter_key(key, &self.connections),
        }
    }

    fn step_filtered(&mut self, forward: bool) {
        if self.navigator.is_filtered() {
            self.navigator.step_relation(forward, &self.connections);
        } else {
            let rows = if forward { 1 } else { -1 };
            self.navigator.move_by(rows, &self.connections);
        }
    }

    /// Opens the table of the cursor. The filter stays, so the next table is near.
    fn accept_tree_filter(&mut self) {
        let row = self.navigator.current(&self.connections);
        let object = |row: &Row| matches!(row.kind, RowKind::Object(_));
        match row.filter(|row| is_relation(row) || object(row)) {
            Some(row) if self.navigator.is_filtered() => {
                self.navigator.leave_filter_field();
                if object(&row) {
                    self.open_row_definition(&row);
                } else {
                    self.open_table(&row);
                }
            }
            _ if !self.navigator.is_filtered() => self.navigator.leave_filter_field(),
            _ => {}
        }
    }

    pub(super) fn toggle(&mut self, row: &Row) {
        if self.navigator.is_filtered() {
            return;
        }
        if self.navigator.is_open(row, &self.connections) {
            self.navigator.collapse(row, &self.connections);
        } else {
            self.expand(row);
        }
    }

    fn expand(&mut self, row: &Row) {
        if !Navigator::is_branch(row) || self.navigator.is_filtered() {
            return;
        }
        if self.navigator.is_open(row, &self.connections) {
            self.navigator.move_by(1, &self.connections);
            return;
        }
        self.navigator.expand(row, &self.connections);
        let id = self.connections[row.connection].id;
        match row.kind {
            RowKind::Connection => {
                self.refresh_tree_consoles();
                self.ensure_catalog(id);
            }
            RowKind::Table | RowKind::View => self.read_columns(row, id),
            _ => {}
        }
    }

    fn read_columns(&mut self, row: &Row, id: ConnectionId) {
        let Some(relation) = row.relation_ref(&self.connections) else {
            return;
        };
        let catalog = self.connections[row.connection].catalog.as_ref();
        let found = catalog
            .and_then(|catalog| catalog.relation(Some(&relation.schema), &relation.relation));
        if found.is_some_and(|(_, item)| item.columns == Columns::NotLoaded) {
            self.request_columns(id, relation);
        }
    }

    fn collapse(&mut self, row: &Row) {
        if self.navigator.is_filtered() {
            return;
        }
        if self.navigator.is_open(row, &self.connections) {
            self.navigator.collapse(row, &self.connections);
        } else {
            self.navigator.select_parent(row, &self.connections);
        }
    }

    /// Selects a console of the connection, or it asks for one.
    fn open_console(&mut self, id: ConnectionId) {
        match self
            .consoles
            .iter()
            .position(|console| console.connection == Some(id))
        {
            Some(index) => {
                self.select_console(index);
                self.leave_navigator();
            }
            None => self.request_console(id),
        }
    }

    pub(super) fn leave_navigator(&mut self) {
        self.navigator_focused = false;
        self.console_mut().focus = Focus::Editor;
    }

    /// Opens a console. An empty console without a connection takes the connection,
    /// so that the first use does not leave an extra tab.
    pub(super) fn new_console(&mut self, id: ConnectionId) {
        let Some(dialect) = self.connection(id).map(|connection| connection.dialect) else {
            return;
        };
        let active = self.console();
        if active.connection.is_some() || !active.editor.text().is_empty() {
            let console = self.unbound_console(dialect);
            self.consoles.push(console);
            self.select_console(self.consoles.len() - 1);
        } else {
            // The hidden console takes a tab, in the order of this opening.
            let place = self.next_tab_place();
            let console = self.console_mut();
            if console.hidden {
                console.hidden = false;
                console.opened = place;
            }
            self.select_console(self.active);
        }
        self.console_mut().editor.set_dialect(dialect);
        self.bind_console(self.active, id);
        self.leave_navigator();
    }

    /// Opens a new console of a connection with a text, and with a schema when the
    /// connection has a schema choice. Nothing runs.
    pub(super) fn open_text_console(
        &mut self,
        id: ConnectionId,
        schema: Option<String>,
        text: &str,
    ) {
        let namespace = self
            .connection(id)
            .is_some_and(|item| item.capabilities.namespace != Namespace::None);
        // A new console never takes the text of a console of the user.
        self.new_console(id);
        if namespace && schema.is_some() {
            self.console_mut().schema = schema;
            self.save_console_meta(self.active);
        }
        let console = self.console_mut();
        console.focus = Focus::Editor;
        console.editor.on_paste(text);
    }

    /// Returns the name of a console or a table that must end its work first.
    pub(super) fn console_in_use(&self, id: ConnectionId) -> Option<String> {
        let console = self
            .consoles
            .iter()
            .filter(|console| console.connection == Some(id))
            .find(|console| console.is_busy() || console.in_transaction())
            .map(|console| console.name.clone());
        console.or_else(|| {
            let table = self.tables.iter().find(|tab| {
                tab.connection == id
                    && (tab.is_busy() || !tab.result.edits.is_empty() || tab.edit_uncertain)
            });
            table.map(TableTab::label)
        })
    }

    fn disconnect(&mut self, id: ConnectionId) {
        if let Some(console) = self.console_in_use(id) {
            self.status = Some(text::connection_in_use(&console));
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
        self.invalidate_ai_schema(id);
        self.forget_ai_schema(id);
        let connected = self
            .manager
            .as_ref()
            .is_some_and(|manager| manager.is_connected(id));
        if connected {
            self.status = Some(text::SESSION_IN_USE.to_owned());
            return;
        }
        self.status = Some(text::DISCONNECTED.to_owned());
        self.unlock_form(id);
    }

    /// The idle limit closed sessions. The explorer redraws, and no message appears.
    pub(super) fn on_idle_closed(&mut self, id: ConnectionId) {
        let connected = self
            .manager
            .as_ref()
            .is_some_and(|manager| manager.is_connected(id));
        if !connected {
            self.unlock_form(id);
        }
    }

    /// A form that only shows a connection accepts changes after the disconnection.
    fn unlock_form(&mut self, id: ConnectionId) {
        let definition = self.connection(id).and_then(|item| item.definition.clone());
        let credential = self
            .connection(id)
            .map(|item| item.credential)
            .unwrap_or_default();
        let key = self.connection(id).and_then(|item| item.key.clone());
        if let (Some(form), Some(definition)) = (&mut self.form, definition)
            && form.editing == Some(id)
            && form.display_only
        {
            *form = ConnectionForm::editing(id, &definition, false);
            form.credential = credential;
            if let Some(key) = key {
                form.connection_id = key.id;
            }
        }
    }

    fn edit_connection(&mut self, id: ConnectionId) {
        let Some(definition) = self
            .connection(id)
            .and_then(|item| item.definition.as_ref())
        else {
            self.status = Some(text::NO_DEFINITION.to_owned());
            return;
        };
        let connected = self
            .manager
            .as_ref()
            .is_some_and(|manager| manager.is_connected(id));
        let mut form = ConnectionForm::editing(id, definition, connected);
        if let Some(connection) = self.connection(id) {
            form.credential = connection.credential;
            if let Some(key) = &connection.key {
                form.connection_id.clone_from(&key.id);
            }
        }
        if connected {
            form.note = Some(FormNote::Info(text::FORM_CONNECTED.to_owned()));
        }
        self.form = Some(form);
    }

    fn confirm_removal(&mut self, id: ConnectionId) {
        if let Some(console) = self.console_in_use(id) {
            self.status = Some(text::connection_in_use(&console));
            return;
        }
        let Some(connection) = self.connection(id) else {
            return;
        };
        let file = connection
            .scope()
            .and_then(|scope| self.persist_root(scope))
            .map(|root| root.config_file().display().to_string());
        let body = text::remove_connection(&connection.name, file.as_deref());
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
            self.status = Some(text::connection_in_use(&console));
            return;
        }
        // The removal does not delete a password, so the user keeps a separate choice.
        let saved = self
            .connection(id)
            .filter(|item| {
                matches!(
                    item.credential,
                    CredentialName::Vault | CredentialName::GlobalPlainText
                )
            })
            .and_then(|item| {
                self.saved_identity(item)
                    .map(|identity| (item.plain_name().to_owned(), identity))
            });
        // The scripts of the connection stay on disk. Only its definition goes.
        if let Some(key) = self.connection(id).and_then(|item| item.key.clone())
            && let Err(message) = self.change_config(key.scope, Change::Remove(key.id))
        {
            self.status = Some(message);
            return;
        }
        self.drop_connection(id);
        if let Some((name, identity)) = saved {
            self.ask_password_deletion(&name, identity);
        }
    }

    pub(super) fn saved_identity(&self, connection: &Connection) -> Option<Identity> {
        Identity::of(connection.key.as_ref()?, connection.definition.as_ref()?)
    }

    fn confirm_password_deletion(&mut self, id: ConnectionId) {
        let found = self.connection(id).and_then(|item| {
            self.saved_identity(item)
                .map(|identity| (item.plain_name().to_owned(), identity))
        });
        match found {
            Some((name, identity)) => self.ask_password_deletion(&name, identity),
            None => self.status = Some(text::PASSWORD_NOT_FOUND.to_owned()),
        }
    }

    fn ask_password_deletion(&mut self, name: &str, identity: Identity) {
        self.show_dialog(
            text::TITLE_DELETE_PASSWORD,
            text::delete_password_question(&printable(name)),
            vec![
                (text::OPTION_KEEP_PASSWORD, DialogAction::Dismiss),
                (
                    text::OPTION_DELETE_PASSWORD,
                    DialogAction::DeletePassword(identity),
                ),
            ],
        );
    }

    /// Removes a connection from the running application. Its consoles keep their text.
    pub(super) fn drop_connection(&mut self, id: ConnectionId) {
        for index in 0..self.consoles.len() {
            if self.consoles[index].connection == Some(id) {
                self.unbind_console(index);
            }
        }
        self.close_tables_of(id);
        self.close_diagrams_of(id);
        self.close_compares_of(id);
        self.cancel_definitions_of(id);
        if let Some(manager) = &mut self.manager {
            manager.remove_connection(id);
        }
        self.connections.retain(|connection| connection.id != id);
        self.navigator.forget(id);
        refresh_labels(&mut self.connections);
    }

    /// The console keeps its text and its results, and it cannot run.
    pub(super) fn unbind_console(&mut self, index: usize) {
        self.cancel_agent_console(self.consoles[index].id);
        self.agent.forget_console(self.consoles[index].id);
        let id = self.next_unbound_console;
        self.next_unbound_console -= 1;
        let console = &mut self.consoles[index];
        if let Some(manager) = &mut self.manager {
            manager.close_console(console.id);
        }
        let previous = console.id;
        console.id = id;
        console.connection = None;
        console.schema = None;
        self.rekey_saves(previous, id);
        self.refresh_tree_consoles();
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
        let names = connection.catalog.as_ref().map(|catalog| {
            catalog
                .schemas
                .iter()
                .map(|schema| schema.name.clone())
                .collect::<Vec<_>>()
        });
        let console = self.console();
        if console.is_busy() || console.in_transaction() {
            self.status = Some(text::SCHEMA_BUSY.to_owned());
            return;
        }
        let selected = console.schema.clone();
        let loading = names.is_none();
        let names = names.unwrap_or_default();
        let cursor = selected
            .and_then(|schema| names.iter().position(|item| *item == schema))
            .map_or(0, |index| index + 1);
        let items = std::iter::once(text::SCHEMA_DEFAULT.to_owned())
            .chain(names)
            .collect();
        let kind = PickerKind::Schema {
            console: console.id,
            connection: id,
        };
        self.picker = Some(Picker {
            kind,
            title: title.to_owned(),
            items,
            cursor,
            loading,
        });
        if loading {
            self.ensure_catalog(id);
            let failed = self
                .connection(id)
                .is_some_and(|item| item.catalog_state == CatalogState::Failed);
            if failed {
                self.reload_catalog(id);
            }
        }
    }

    fn update_picker(&mut self, id: ConnectionId, schemas: Option<Vec<String>>) {
        let Some(picker) = self.picker.as_mut().filter(|picker| {
            matches!(picker.kind, PickerKind::Schema { connection, .. } if connection == id)
        }) else {
            return;
        };
        match schemas {
            Some(schemas) => {
                picker.items = std::iter::once(text::SCHEMA_DEFAULT.to_owned())
                    .chain(schemas)
                    .collect();
                picker.cursor = picker.cursor.min(picker.items.len() - 1);
                picker.loading = false;
            }
            None => self.picker = None,
        }
    }

    pub(super) fn accept_picker(&mut self) {
        let Some(picker) = self.picker.take() else {
            return;
        };
        let cursor = picker.cursor;
        match picker.kind {
            PickerKind::Agent(choices) => {
                if let Some(choice) = choices.get(cursor) {
                    self.accept_agent_choice(choice.clone());
                }
            }
            PickerKind::Schema { console, .. } => {
                // The first row is the default of the connection, and it has no schema.
                let schema = picker.items.get(cursor).filter(|_| cursor > 0).cloned();
                if let Some(index) = self.console_index(console) {
                    self.consoles[index].schema = schema;
                    self.save_console_meta(index);
                }
            }
            PickerKind::Consoles {
                connection,
                scripts,
            } => match cursor.checked_sub(1) {
                None => self.new_console(connection),
                Some(index) => {
                    if let Some(entry) = scripts.get(index) {
                        self.open_script(entry);
                    }
                }
            },
            PickerKind::Orphans { scripts } => {
                if let Some(entry) = scripts.get(cursor) {
                    self.open_script(entry);
                }
            }
            PickerKind::Associate {
                console,
                connections,
            } => {
                if let Some(connection) = connections.get(cursor) {
                    self.confirm_association(console, *connection);
                }
            }
            PickerKind::Follow {
                connection,
                mut choices,
            } => {
                if cursor < choices.len() {
                    self.go_follow(connection, choices.swap_remove(cursor));
                }
            }
            PickerKind::Generate {
                request,
                templates,
                definition,
            } => match (request, templates.get(cursor), definition) {
                (Some(request), Some(template), _) => self.start_template(request, *template),
                (_, None, Some((connection, target))) => {
                    self.start_definition(connection, target);
                }
                _ => {}
            },
        }
    }

    pub(super) fn test_form(&mut self) {
        if self.config_blocked() {
            return;
        }
        let Some(definition) = self.form_definition() else {
            return;
        };
        let definition = match definition {
            Ok(definition) => definition,
            Err(invalid) => {
                self.show_invalid(invalid);
                return;
            }
        };
        if let Some(path) = missing_file(&definition) {
            self.show_invalid(Invalid {
                field: Field::File,
                message: text::form_file_missing(&path),
            });
            return;
        }
        if !self.ensure_manager() {
            return;
        }
        let test = self.next_test;
        self.next_test += 1;
        if let Some(form) = &mut self.form {
            form.test = Some(test);
            form.note = Some(FormNote::Info(text::FORM_TESTING.to_owned()));
        }
        self.authorize_test(definition, test, false);
    }

    /// Opens the list of the schemas, and it reads their names with the definition of the
    /// form, as a test does.
    pub(super) fn open_schema_list(&mut self) {
        if self.config_blocked() {
            return;
        }
        let definition = match self.form_definition() {
            Some(Ok(definition)) => definition,
            Some(Err(invalid)) => {
                self.show_invalid(invalid);
                return;
            }
            None => return,
        };
        if !self.ensure_manager() {
            return;
        }
        let test = self.next_test;
        self.next_test += 1;
        if let Some(form) = &mut self.form {
            form.test = Some(test);
            form.note = None;
            form.open_schema_list();
        }
        self.authorize_test(definition, test, true);
    }

    pub(super) fn on_connection_tested(
        &mut self,
        test: u64,
        result: Result<Option<Vec<String>>, ConnectError>,
    ) {
        if self.on_password_verified(test, &result) {
            return;
        }
        let Some(form) = self.form.as_mut().filter(|form| form.test == Some(test)) else {
            return;
        };
        form.test = None;
        form.note = match result {
            Ok(Some(names)) => {
                form.fill_schema_list(names);
                None
            }
            Ok(None) => Some(FormNote::Success(text::FORM_TEST_OK.to_owned())),
            Err(error) => {
                form.close_schema_list();
                Some(FormNote::Error(error.message))
            }
        };
    }

    pub(super) fn disconnect_form(&mut self) {
        let Some(id) = self.form.as_ref().and_then(|form| form.editing) else {
            return;
        };
        if let Some(console) = self.console_in_use(id) {
            let note = FormNote::Error(text::connection_in_use(&console));
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
        if form.display_only {
            let name = form.text(Field::Name).trim().to_owned();
            let (theme, icon, production) = (form.theme, form.icon, form.production);
            match form.schemas() {
                Ok(schemas) => self.save_display(name, theme, icon, production, schemas),
                Err(invalid) => self.show_invalid(invalid),
            }
            return;
        }
        let definition = match self.form_definition() {
            Some(Ok(definition)) => definition,
            Some(Err(invalid)) => {
                self.show_invalid(invalid);
                return;
            }
            None => return,
        };
        if name_is_taken(
            &self.connections,
            &definition.name,
            self.form_scope(),
            editing,
        ) {
            self.show_invalid(name_taken());
            return;
        }
        if let Some(path) = missing_file(&definition) {
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

    /// Builds the definition of the form with the folder of its scope, so that a saved
    /// relative path opens here the same file that the connection opens.
    pub(super) fn form_definition(&self) -> Option<Result<Definition, Invalid>> {
        let form = self.form.as_ref()?;
        let base = self
            .form_scope()
            .and_then(|scope| self.persist_root(scope))
            .map(|root| root.base);
        Some(
            form.definition()
                .map(|definition| Definition { base, ..definition }),
        )
    }

    /// Returns the scope that the form saves to. A session without files has none.
    pub(super) fn form_scope(&self) -> Option<Scope> {
        self.persist.as_ref()?;
        let form = self.form.as_ref()?;
        match form.editing {
            Some(id) => self.connection(id).and_then(Connection::scope),
            None if self.has_project() => Some(form.scope),
            None => Some(Scope::Global),
        }
    }

    pub(super) fn create_form_file(&mut self) {
        match self.form_definition() {
            Some(Ok(mut definition)) => {
                if let Source::Sqlite(target) = &mut definition.source {
                    target.allow_create = true;
                }
                self.apply_definition(definition);
            }
            Some(Err(invalid)) => self.show_invalid(invalid),
            None => {}
        }
    }

    fn set_form_error(&mut self, message: String) {
        if let Some(form) = &mut self.form {
            form.note = Some(FormNote::Error(message));
        }
    }

    /// Shows the error and opens the section of its field, with the focus on the field.
    fn show_invalid(&mut self, invalid: Invalid) {
        if let Some(form) = &mut self.form {
            form.focus_on(invalid.field);
            form.note = Some(FormNote::Error(invalid.message));
        }
    }

    /// Saves the values that need no new session. The sessions stay open. A change of
    /// the visible schemas reads the catalog again.
    fn save_display(
        &mut self,
        name: String,
        theme: Option<ThemeName>,
        icon: Option<ConnectionIcon>,
        production: bool,
        schemas: Vec<String>,
    ) {
        let Some(id) = self.form.as_ref().and_then(|form| form.editing) else {
            return;
        };
        if name.is_empty() {
            self.show_invalid(Invalid {
                field: Field::Name,
                message: text::FORM_NEEDS_NAME.to_owned(),
            });
            return;
        }
        if name_is_taken(&self.connections, &name, self.form_scope(), Some(id)) {
            self.show_invalid(name_taken());
            return;
        }
        let Some(connection) = self.connection(id) else {
            return;
        };
        let key = connection.key.clone();
        let credential = connection.credential;
        let mut renamed = connection.definition.clone();
        let schemas_changed = renamed
            .as_ref()
            .is_some_and(|definition| definition.schemas != schemas);
        if let Some(definition) = &mut renamed {
            definition.name.clone_from(&name);
            definition.theme = theme;
            definition.icon = icon;
            definition.production = production;
            definition.schemas = schemas;
            if let Some(key) = &key
                && let Err(message) = self.save_connection(key, definition, credential)
            {
                self.set_form_error(message);
                return;
            }
        }
        if let Some(connection) = self.connection_mut(id) {
            connection.definition = renamed;
            connection.name = name;
        }
        refresh_labels(&mut self.connections);
        self.form = None;
        if schemas_changed {
            self.reload_catalog(id);
        }
    }

    fn apply_definition(&mut self, mut definition: Definition) {
        let editing = self.form.as_ref().and_then(|form| form.editing);
        let connected = editing.is_some_and(|id| {
            self.manager
                .as_ref()
                .is_some_and(|manager| manager.is_connected(id))
        });
        if connected {
            self.set_form_error(text::FORM_CONNECTED.to_owned());
            return;
        }
        // The file of the scope comes first, so a failed write leaves the form open.
        let key = match editing {
            Some(id) => self.connection(id).and_then(|item| item.key.clone()),
            None => self.form_scope().map(|scope| ConnectionKey {
                scope,
                id: self.form.as_ref().unwrap().connection_id.clone(),
            }),
        };
        let mut credential = self
            .form
            .as_ref()
            .map_or(CredentialName::Session, |form| form.credential);
        if credential != CredentialName::Vault {
            credential = match &definition.source {
                Source::Network(target) if target.password.is_some() => CredentialName::Session,
                _ => CredentialName::None,
            };
        }
        if let Some(key) = &key {
            definition.base = self.persist_root(key.scope).map(|root| root.base);
            if let Err(message) = self.save_connection(key, &definition, credential) {
                self.set_form_error(message);
                return;
            }
        }
        let id = match editing {
            Some(id) => {
                self.replace_definition(id, definition);
                Some(id)
            }
            None => self.add_definition(definition),
        };
        if let Some(id) = id {
            if let Some(connection) = self.connection_mut(id) {
                connection.key = key;
                connection.credential = credential;
            }
            refresh_labels(&mut self.connections);
            if editing.is_none() {
                self.navigator.select_connection(id);
                self.navigator_focused = !self.navigator.collapsed;
            }
            let generation = self.connection(id).unwrap().generation;
            self.auth.local.insert(id, (generation, editing.is_none()));
        }
        self.form = None;
        if let Some(id) = id {
            self.authorize(id, super::auth::Use::Save);
        }
    }

    /// Applies a changed definition. A session never survives a change of access.
    pub(super) fn replace_definition(&mut self, id: ConnectionId, definition: Definition) {
        let connected = self
            .manager
            .as_ref()
            .is_some_and(|manager| manager.is_connected(id));
        if connected {
            self.set_form_error(text::FORM_CONNECTED.to_owned());
            return;
        }
        let connector = definition.connector();
        let settings = definition.settings(&self.settings);
        if let Some(manager) = &mut self.manager {
            manager.replace_connection(id, connector.clone(), settings);
        }
        let Some(index) = self.connection_index(id) else {
            return;
        };
        let name = definition.name.clone();
        let dialect = connector.dialect();
        let key = self.connections[index].key.clone();
        let credential = self.connections[index].credential;
        let generation = self.connections[index].generation + 1;
        self.connections[index] = Connection::new(id, name, Some(definition), connector.as_ref());
        self.connections[index].key = key;
        self.connections[index].credential = credential;
        self.connections[index].generation = generation;
        refresh_labels(&mut self.connections);
        self.navigator.forget(id);
        for index in 0..self.consoles.len() {
            if self.consoles[index].connection == Some(id) {
                self.consoles[index].editor.set_dialect(dialect);
                self.consoles[index].schema = None;
                self.save_console_meta(index);
            }
        }
    }
}

/// Returns the full path of a SQLite file that does not exist yet.
fn missing_file(definition: &Definition) -> Option<String> {
    match definition.resolved_source() {
        Source::Sqlite(target) if !target.path.exists() && !target.allow_create => {
            Some(target.path.display().to_string())
        }
        _ => None,
    }
}

fn name_taken() -> Invalid {
    Invalid {
        field: Field::Name,
        message: text::FORM_NAME_TAKEN.to_owned(),
    }
}
