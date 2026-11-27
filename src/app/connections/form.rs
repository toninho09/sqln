//! The connection form: the type picker, tests, validation, and saves.

use super::*;

impl App {
    /// A form that only shows a connection accepts changes after the disconnection.
    pub(super) fn unlock_form(&mut self, id: ConnectionId) {
        let definition = self.connection(id).and_then(|item| item.definition.clone());
        let credential = self
            .connection(id)
            .map(|item| item.credential)
            .unwrap_or_default();
        let key = self.connection(id).and_then(|item| item.key.clone());
        if let (Some(form), Some(definition)) = (&mut self.modals.form, definition)
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

    pub(in crate::app) fn edit_connection(&mut self, id: ConnectionId) {
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
        form.themes = self.appearance.custom_themes.clone();
        if let Some(connection) = self.connection(id) {
            form.credential = connection.credential;
            if let Some(key) = &connection.key {
                form.connection_id.clone_from(&key.id);
            }
        }
        if connected {
            form.note = Some(FormNote::Info(text::FORM_CONNECTED.to_owned()));
        }
        self.modals.form = Some(form);
    }

    pub(in crate::app) fn open_picker(&mut self) {
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
        self.modals.picker = Some(Picker {
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

    pub(super) fn update_picker(&mut self, id: ConnectionId, schemas: Option<Vec<String>>) {
        let Some(picker) = self.modals.picker.as_mut().filter(|picker| {
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
            None => self.modals.picker = None,
        }
    }

    pub(in crate::app) fn accept_picker(&mut self) {
        let Some(picker) = self.modals.picker.take() else {
            return;
        };
        let cursor = picker.cursor;
        match picker.kind {
            PickerKind::Analysis(choice) => self.choose_analysis(choice, cursor),
            PickerKind::Agent(choices) => {
                if let Some(choice) = choices.get(cursor) {
                    self.accept_agent_choice(choice.clone());
                }
            }
            PickerKind::Schema { console, .. } => {
                // The first row is the default of the connection, and it has no schema.
                let schema = picker.items.get(cursor).filter(|_| cursor > 0).cloned();
                if let Some(index) = self.console_index(console) {
                    self.tabs.consoles[index].schema = schema;
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
            PickerKind::SqlFile { path, connections } => {
                let connection = connections.get(cursor).copied();
                if let Some(connection) = connection {
                    self.picked_sql_file_connection(connection, path);
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
            PickerKind::Folder {
                connection,
                mut choices,
            } => {
                if cursor < choices.len() {
                    self.choose_folder(connection, choices.swap_remove(cursor));
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

    pub(in crate::app) fn test_form(&mut self) {
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
        let test = self.counters.next_test;
        self.counters.next_test += 1;
        if let Some(form) = &mut self.modals.form {
            form.test = Some(test);
            form.note = Some(FormNote::Info(text::FORM_TESTING.to_owned()));
        }
        self.authorize_test(definition, test, false);
    }

    /// Opens the list of the schemas, and it reads their names with the definition of the
    /// form, as a test does.
    pub(in crate::app) fn open_schema_list(&mut self) {
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
        let test = self.counters.next_test;
        self.counters.next_test += 1;
        if let Some(form) = &mut self.modals.form {
            form.test = Some(test);
            form.note = None;
            form.open_schema_list();
        }
        self.authorize_test(definition, test, true);
    }

    pub(in crate::app) fn on_connection_tested(
        &mut self,
        test: u64,
        result: Result<Option<Vec<String>>, ConnectError>,
    ) {
        if self.on_password_verified(test, &result) {
            return;
        }
        let Some(form) = self
            .modals
            .form
            .as_mut()
            .filter(|form| form.test == Some(test))
        else {
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
                Some(FormNote::Error(error.into()))
            }
        };
    }

    pub(in crate::app) fn disconnect_form(&mut self) {
        let Some(id) = self.modals.form.as_ref().and_then(|form| form.editing) else {
            return;
        };
        if let Some(console) = self.console_in_use(id) {
            let note = FormNote::Error(text::connection_in_use(&console).into());
            if let Some(form) = &mut self.modals.form {
                form.note = Some(note);
            }
            return;
        }
        if let Some(manager) = &self.manager {
            manager.disconnect(id);
        }
    }

    pub(in crate::app) fn save_form(&mut self) {
        let Some(form) = &self.modals.form else {
            return;
        };
        let editing = form.editing;
        if form.display_only {
            let name = form.text(Field::Name).trim().to_owned();
            let display = Display {
                name,
                theme: form.theme.clone(),
                icon: form.icon,
                production: form.production,
                folder: None,
                schemas: Vec::new(),
            };
            match form
                .folder()
                .and_then(|folder| Ok((folder, form.schemas()?)))
            {
                Ok((folder, schemas)) => self.save_display(Display {
                    folder,
                    schemas,
                    ..display
                }),
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
    pub(in crate::app) fn form_definition(&self) -> Option<Result<Definition, Invalid>> {
        let form = self.modals.form.as_ref()?;
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
    pub(in crate::app) fn form_scope(&self) -> Option<Scope> {
        self.persist.as_ref()?;
        let form = self.modals.form.as_ref()?;
        match form.editing {
            Some(id) => self.connection(id).and_then(Connection::scope),
            None if self.has_project() => Some(form.scope),
            None => Some(Scope::Global),
        }
    }

    pub(in crate::app) fn create_form_file(&mut self) {
        match self.form_definition() {
            Some(Ok(mut definition)) => {
                // The user accepted the creation, so the connection may make the file.
                match &mut definition.source {
                    Source::Sqlite(target) => target.allow_create = true,
                    Source::Duckdb(target) => target.allow_create = true,
                    Source::Network(_) => {}
                }
                self.apply_definition(definition);
            }
            Some(Err(invalid)) => self.show_invalid(invalid),
            None => {}
        }
    }

    pub(super) fn set_form_error(&mut self, message: String) {
        if let Some(form) = &mut self.modals.form {
            form.note = Some(FormNote::Error(message.into()));
        }
    }

    /// Shows the error and opens the section of its field, with the focus on the field.
    pub(super) fn show_invalid(&mut self, invalid: Invalid) {
        if let Some(form) = &mut self.modals.form {
            form.focus_on(invalid.field);
            form.note = Some(FormNote::Error(invalid.message.into()));
        }
    }

    /// Saves the values that need no new session. The sessions stay open. A change of
    /// the visible schemas reads the catalog again.
    pub(super) fn save_display(&mut self, display: Display) {
        let Display {
            name,
            theme,
            icon,
            production,
            folder,
            schemas,
        } = display;
        let Some(id) = self.modals.form.as_ref().and_then(|form| form.editing) else {
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
            definition.folder = folder;
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
        self.modals.form = None;
        if schemas_changed {
            self.reload_catalog(id);
        }
    }

    pub(super) fn apply_definition(&mut self, mut definition: Definition) {
        let editing = self.modals.form.as_ref().and_then(|form| form.editing);
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
            None => self
                .form_scope()
                .zip(self.modals.form.as_ref())
                .map(|(scope, form)| ConnectionKey {
                    scope,
                    id: form.connection_id.clone(),
                }),
        };
        let mut credential = self
            .modals
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
        if credential == CredentialName::Vault {
            self.write_registry_password(&definition);
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
            crate::arrange::sort_by_scope(&mut self.connections);
            refresh_labels(&mut self.connections);
            if editing.is_none() {
                self.navigator.select_connection(id);
                self.navigator_focused = !self.navigator.collapsed;
            }
            if let Some(generation) = self.connection(id).map(|item| item.generation) {
                self.auth.local.insert(id, (generation, editing.is_none()));
            }
        }
        self.modals.form = None;
        if let Some(id) = id {
            self.authorize(id, crate::app::auth::Use::Save);
        }
    }

    /// Applies a changed definition. A session never survives a change of access.
    pub(in crate::app) fn replace_definition(&mut self, id: ConnectionId, definition: Definition) {
        let connected = self
            .manager
            .as_ref()
            .is_some_and(|manager| manager.is_connected(id));
        if connected {
            self.set_form_error(text::FORM_CONNECTED.to_owned());
            return;
        }
        let connector = crate::connector::open(&definition);
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
        for index in 0..self.tabs.consoles.len() {
            if self.tabs.consoles[index].connection == Some(id) {
                self.tabs.consoles[index].editor.set_dialect(dialect);
                self.tabs.consoles[index].schema = None;
                self.save_console_meta(index);
            }
        }
    }
}

/// Returns the full path of a file that does not exist yet.
pub(super) fn missing_file(definition: &Definition) -> Option<String> {
    match definition.resolved_source() {
        Source::Sqlite(target)
            if matches!(
                target.source,
                crate::connector::sqlite::SqliteSource::File(_)
            ) && !target.source_exists()
                && !target.allow_create =>
        {
            Some(target.file_display())
        }
        Source::Duckdb(target)
            if matches!(
                target.source,
                crate::connector::duckdb::DuckdbSource::File(_)
            ) && !target.source_exists()
                && !target.allow_create =>
        {
            Some(target.file_display())
        }
        _ => None,
    }
}

pub(super) fn name_taken() -> Invalid {
    Invalid {
        field: Field::Name,
        message: text::FORM_NAME_TAKEN.to_owned(),
    }
}

/// The values that the form of a connected connection can change.
pub(super) struct Display {
    name: String,
    theme: Option<ThemeName>,
    icon: Option<ConnectionIcon>,
    production: bool,
    folder: Option<String>,
    schemas: Vec<String>,
}
