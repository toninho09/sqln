//! Connections, their catalogs, the navigator actions, and the form.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use crossterm::event::{KeyCode, KeyEvent};

use super::{App, DialogAction, Picker, PickerKind, SqlRequest};
use crate::catalog::{Catalog, ColumnNote, Columns, ForeignKey, RelationKind, RelationRef};
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
use crate::store::Scope;
use crate::store::config::{Change, ConnectionIcon, CredentialName, ThemeName};
use crate::table::TableTab;
use crate::ui::text;

mod form;
mod navigator;

impl App {
    /// Starts the manager at its first use, so a start without a database has no runtime.
    pub(super) fn ensure_manager(&mut self) -> bool {
        if self.manager.is_some() {
            return true;
        }
        match Manager::with_wake(self.budget.clone(), self.wake.clone()) {
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
        let connector = crate::connector::open(&definition);
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
        if path
            .to_string_lossy()
            .trim()
            .eq_ignore_ascii_case(":memory:")
        {
            self.add_sqlite(SqliteTarget::in_memory());
            return;
        }
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
            self.bind_console(self.tabs.active, connection);
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
        let previous = self.tabs.consoles[index].id;
        let fresh = (self.persist.is_some()
            && self.tabs.consoles[index].file.is_none()
            && !self.agent.owns(previous))
        .then(|| self.fresh_console_name(connection));
        let console = &mut self.tabs.consoles[index];
        console.id = id;
        console.connection = Some(connection);
        console.messages.start_association();
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
            .tabs
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

    pub(super) fn connection_index(&self, id: ConnectionId) -> Option<usize> {
        self.connections
            .iter()
            .position(|connection| connection.id == id)
    }

    /// Returns the connection whose catalog the console reads, and it sets the schema
    /// of that console as the current schema of the catalog.
    pub(super) fn catalog_for(&mut self, index: usize) -> Option<ConnectionId> {
        let schema = self.tabs.consoles[index].schema.clone();
        let id = self.tabs.consoles[index].connection?;
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
        self.counters.catalog_reads += 1;
        let read = self.counters.catalog_reads;
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
        columns: Result<(Vec<String>, Vec<ColumnNote>), String>,
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
                self.pending.sql = None;
                self.pending.follow = None;
                // A refresh would ask for the same columns again, so the lists only stop.
                for console in &mut self.tabs.consoles {
                    if console.connection == Some(id) {
                        console.editor.stop_waiting(relation);
                    }
                }
                for table in &mut self.tabs.tables {
                    if table.connection == id {
                        table.stop_waiting(relation);
                    }
                }
            }
        }
        self.continue_follow(id, relation);
        let same = |(request, _): &mut (SqlRequest, Template)| {
            request.connection == id && request.relation == *relation
        };
        if let Some((request, template)) = self.pending.sql.take_if(same) {
            self.write_sql(&request, template);
        }
    }

    /// Gives the new catalog to the open lists of the consoles of a connection.
    fn refresh_consoles(&mut self, id: ConnectionId) {
        for index in 0..self.tabs.consoles.len() {
            if self.tabs.consoles[index].connection == Some(id) {
                self.catalog_for(index);
                let catalog = App::catalog_of(&self.connections, id);
                self.tabs.consoles[index]
                    .editor
                    .refresh_suggestions(catalog);
                self.request_missing_columns(index);
            }
        }
        self.refresh_tables(id);
    }

    pub(super) fn request_missing_columns(&mut self, index: usize) {
        let Some(id) = self.tabs.consoles[index].connection else {
            return;
        };
        for relation in self.tabs.consoles[index].editor.take_missing_columns() {
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

    /// Opens a console. An empty console without a connection takes the connection,
    /// so that the first use does not leave an extra tab.
    pub(super) fn new_console(&mut self, id: ConnectionId) {
        let Some(dialect) = self.connection(id).map(|connection| connection.dialect) else {
            return;
        };
        let active = self.console();
        if active.connection.is_some() || !active.editor.text().is_empty() {
            let console = self.unbound_console(dialect);
            self.tabs.consoles.push(console);
            self.select_console(self.tabs.consoles.len() - 1);
        } else {
            // The hidden console takes a tab, in the order of this opening.
            let place = self.next_tab_place();
            let console = self.console_mut();
            if console.hidden {
                console.hidden = false;
                console.opened = place;
            }
            self.select_console(self.tabs.active);
        }
        self.console_mut().editor.set_dialect(dialect);
        self.bind_console(self.tabs.active, id);
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
            self.save_console_meta(self.tabs.active);
        }
        let console = self.console_mut();
        console.focus = Focus::Editor;
        console.editor.on_paste(text);
    }

    /// Returns the name of a console or a table that must end its work first.
    pub(super) fn console_in_use(&self, id: ConnectionId) -> Option<String> {
        let console = self
            .tabs
            .consoles
            .iter()
            .filter(|console| console.connection == Some(id))
            .find(|console| console.is_busy() || console.in_transaction())
            .map(|console| console.name.clone());
        console.or_else(|| {
            let table = self.tabs.tables.iter().find(|tab| {
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
        for index in 0..self.tabs.consoles.len() {
            if self.tabs.consoles[index].connection == Some(id) {
                self.unbind_console(index);
            }
        }
        self.close_tabs_using(id);
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
        self.cancel_agent_console(self.tabs.consoles[index].id);
        self.agent.forget_console(self.tabs.consoles[index].id);
        let id = self.counters.next_unbound_console;
        self.counters.next_unbound_console -= 1;
        let console = &mut self.tabs.consoles[index];
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
}

/// The answers of the vault writes of Schema Registry passwords.
pub(super) struct VaultWrites {
    sender: Sender<Result<(), String>>,
    receiver: Receiver<Result<(), String>>,
}

impl Default for VaultWrites {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Self { sender, receiver }
    }
}

impl App {
    /// Writes a typed Schema Registry password to the vault, as the main password goes
    /// there by the choice of the form. The session keeps the password in memory, so the
    /// connection works before the write ends.
    fn write_registry_password(&self, definition: &Definition) {
        let Source::Network(target) = &definition.source else {
            return;
        };
        let Some((account, password)) = target.kafka.registry.as_ref().and_then(|registry| {
            let password = registry
                .password
                .clone()
                .filter(|_| !registry.user.is_empty())?;
            Some((
                crate::connector::kafka::registry_account(&registry.url, &registry.user),
                password,
            ))
        }) else {
            return;
        };
        let (Some(manager), vault) = (&self.manager, self.ai_vault()) else {
            return;
        };
        let sender = self.background.registry_writes.sender.clone();
        manager.spawn_blocking(move || {
            let result = vault
                .write_api_key(&account, &password)
                .map_err(|error| text::kafka_registry_not_saved(&error.to_string()));
            let _ = sender.send(result);
        });
    }

    /// Shows a failed vault write of a registry password. It returns true for a change.
    pub(super) fn poll_registry_writes(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.background.registry_writes.receiver.try_recv() {
            if let Err(message) = result {
                self.status = Some(message);
                changed = true;
            }
        }
        changed
    }
}
