//! The files of the running application: the configuration, the autosave of the
//! consoles, the changes that other programs make, and the state of the session.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::history::HistoryLog;
use super::{App, DialogAction, Options};
use crate::connection::{Connection, ConnectionKey, Definition, refresh_labels};
use crate::editor::{KeyboardMode, SqlEditor};
use crate::exec::{ConnectionId, ConsoleId};
use crate::export::ExportChoices;
use crate::import::ImportChoices;
use crate::script_file::{SaveWorker, Trouble};
use crate::settings::SettingsForm;
use crate::store::config::{self, Change, Config, ConfigError, CredentialName, Loaded, Stored};
use crate::store::scripts::{self, ScriptMeta};
use crate::store::workspace::{self, EditorState, Position};
use crate::store::{self, Root, Scope, Store};
use crate::store::{file, history};
use crate::ui::text;

/// The time between two checks of the files for a change by another program.
const POLL_EVERY: Duration = Duration::from_secs(1);

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct SaveIntent {
    manual: bool,
    close: bool,
    associate: Option<ConnectionId>,
    overwrite: bool,
    metadata: bool,
    association_running: Option<ConnectionId>,
    rename: Option<(String, String)>,
}

pub(super) struct Persist {
    worker: SaveWorker,
    pub(super) saves: HashMap<ConsoleId, SaveIntent>,
    pub store: Store,
    /// The hash of each configuration file at the last load or check.
    pub hashes: Vec<(Scope, Option<u64>)>,
    /// The error of the last load. It blocks the connections to databases.
    pub error: Option<ConfigError>,
    /// The scripts whose problem the user already saw. They stay paused.
    pub presented: HashSet<String>,
    next_poll: Instant,
    /// True while a question about a changed configuration is open.
    config_prompt: bool,
    /// The folder where the user can start a project.
    project_offer: Option<PathBuf>,
    /// True when the start opens the tabs of the last session.
    pub reopen_tabs: bool,
    /// False when the list of tabs in the file must stay as it is.
    pub tabs_known: bool,
    /// True when each statement of a console goes to the history file.
    pub query_history: bool,
    /// True when the value window opens a JSON value in its formatted form.
    pub format_json: bool,
    /// A run of this time or more rings the bell at its end. The value 0 stops the bell.
    pub bell_after_seconds: u64,
    pub history: HistoryLog,
}

impl Persist {
    /// Reads the configuration for the start. A good configuration sets the preferences
    /// in the options. It returns the saved connections to add.
    pub(super) fn start(options: &mut Options) -> Option<(Self, Vec<Stored>)> {
        let store = options.store.take()?;
        let Loaded { hashes, config } = config::load(&store);
        let mut error = None;
        let mut stored = Vec::new();
        let mut reopen_tabs = false;
        let mut query_history = false;
        let mut format_json = config::DEFAULT_FORMAT_JSON;
        let mut bell_after_seconds = config::DEFAULT_BELL_AFTER_SECONDS;
        match config {
            Ok(config) => {
                reopen_tabs = config.reopen_tabs;
                query_history = config.query_history;
                format_json = config.format_json;
                bell_after_seconds = config.bell_after_seconds;
                stored = Self::apply_preferences(options, config);
            }
            Err(failure) => error = Some(failure),
        }
        let session_target = options.session_target;
        let persist = Self {
            worker: SaveWorker::new(),
            saves: HashMap::new(),
            store,
            hashes,
            error,
            presented: HashSet::new(),
            next_poll: Instant::now() + POLL_EVERY,
            config_prompt: false,
            project_offer: options.offer_project.take().filter(|_| !session_target),
            reopen_tabs: reopen_tabs && !session_target,
            tabs_known: !session_target,
            query_history,
            format_json,
            bell_after_seconds,
            history: HistoryLog::default(),
        };
        Some((persist, stored))
    }

    fn apply_preferences(options: &mut Options, config: Config) -> Vec<Stored> {
        let settings = &mut options.settings;
        settings.page_size = config.page_size.unwrap_or(settings.page_size);
        settings.row_cap = config.row_cap.unwrap_or(settings.row_cap);
        settings.cap_in_sql = config.cap_in_sql;
        settings.budget_bytes = config.budget_bytes.unwrap_or(settings.budget_bytes);
        options.keyboard = config.keyboard;
        options.undo_limit = config.undo_limit_bytes;
        options.keymap = Some(config.keymap);
        options.appearance = (config.theme, config.icons);
        options.suggestion_keys = config.suggestion_keys;
        options.show_system_commands = config.show_system_commands;
        options.agent = config.agent;
        options.ai_completion = config.ai_completion;
        options.export_choices = config.export;
        options.import_choices = config.import;
        config.connections
    }
}

fn editor_state(editor: &SqlEditor) -> EditorState {
    let cursor = editor.cursor();
    let (column, row) = editor.scroll();
    EditorState {
        cursor: Position {
            row: cursor.row,
            col: cursor.col,
        },
        scroll: Position { row, col: column },
    }
}

impl App {
    pub(super) fn persist_root(&self, scope: Scope) -> Option<Root> {
        self.persist.as_ref()?.store.root(scope).cloned()
    }

    /// Returns true when a project is open, so a connection can have its scope.
    pub(super) fn has_project(&self) -> bool {
        self.persist
            .as_ref()
            .is_some_and(|persist| persist.store.project.is_some())
    }

    /// Returns true, with a message, while an invalid configuration blocks the databases.
    pub(super) fn config_blocked(&mut self) -> bool {
        let blocked = self
            .persist
            .as_ref()
            .is_some_and(|persist| persist.error.is_some());
        if blocked {
            self.status = Some(text::CONFIG_BLOCKED.to_owned());
        }
        blocked
    }

    /// The last steps of the start: old temporary files, the layout, and the questions.
    pub(super) fn finish_start(&mut self) {
        let Some(persist) = &mut self.persist else {
            return;
        };
        for (_, root) in persist.store.scopes() {
            file::remove_old_temporary_files(&root.dir);
            file::remove_old_temporary_files(&root.consoles_dir());
        }
        let offer = persist.project_offer.clone();
        let error = persist.error.clone();
        // The trim only protects the disk, so a failure stays out of the way of the start.
        let _ = history::trim(&persist.store.global);
        self.restore_layout();
        self.reopen_tabs();
        if let Some(folder) = offer {
            self.show_dialog(
                text::TITLE_CREATE_PROJECT,
                text::create_project(&folder.display().to_string()),
                vec![
                    (text::OPTION_GLOBAL_ONLY, DialogAction::Dismiss),
                    (text::OPTION_CREATE_PROJECT, DialogAction::CreateProject),
                ],
            );
        }
        if let Some(error) = error {
            self.show_config_error(&error);
        }
    }

    fn show_config_error(&mut self, error: &ConfigError) {
        self.show_dialog(
            text::TITLE_CONFIG_ERROR,
            text::config_error(&error.message, &error.file.display().to_string()),
            vec![
                (text::OPTION_CLOSE, DialogAction::Dismiss),
                (text::OPTION_RELOAD, DialogAction::ReloadConfig),
            ],
        );
    }

    /// Creates the project that the start offered.
    pub(super) fn create_project(&mut self) {
        let Some(persist) = &mut self.persist else {
            return;
        };
        let Some(folder) = persist.project_offer.take() else {
            return;
        };
        match store::create_project(&folder) {
            Ok(root) => {
                persist.store.project = Some(root);
                persist.hashes.push((Scope::Project, None));
                self.status = Some(text::PROJECT_CREATED.to_owned());
            }
            Err(error) => self.status = Some(text::project_failed(&error.to_string())),
        }
    }

    // ---- configuration ----

    /// Adds a connection of a file.
    pub(super) fn add_stored(&mut self, item: Stored) -> Option<ConnectionId> {
        let id = self.add_definition(item.definition)?;
        if let Some(connection) = self.connection_mut(id) {
            connection.key = Some(ConnectionKey {
                scope: item.scope,
                id: item.id,
            });
            connection.credential = item.credential;
        }
        refresh_labels(&mut self.connections);
        Some(id)
    }

    /// Reads the files again. Open sessions do not change.
    pub(super) fn reload_config(&mut self) {
        let Some(persist) = &mut self.persist else {
            self.status = Some(text::NO_STORE.to_owned());
            return;
        };
        persist.config_prompt = false;
        let Loaded { hashes, config } = config::load(&persist.store);
        persist.hashes = hashes;
        match config {
            Err(error) => {
                persist.error = Some(error.clone());
                self.show_config_error(&error);
            }
            Ok(config) => {
                persist.error = None;
                self.apply_config(config);
                self.status = Some(text::CONFIG_RELOADED.to_owned());
            }
        }
    }

    fn apply_config(&mut self, config: Config) {
        self.set_ai_preferences(config.ai_completion.clone());
        self.settings.page_size = config.page_size.unwrap_or(self.settings.page_size);
        self.settings.row_cap = config.row_cap.unwrap_or(self.settings.row_cap);
        self.settings.cap_in_sql = config.cap_in_sql;
        if let Some(bytes) = config.budget_bytes {
            self.settings.budget_bytes = bytes;
            self.budget.set_limit(bytes);
        }
        self.undo_limit = config.undo_limit_bytes;
        for console in &mut self.consoles {
            console.editor.set_undo_limit(config.undo_limit_bytes);
        }
        self.pending_key = None;
        self.command_page = None;
        self.keymap = config.keymap;
        self.suggestion_keys = config.suggestion_keys;
        self.show_system_commands = config.show_system_commands;
        if let Some(persist) = &mut self.persist {
            persist.query_history = config.query_history;
            persist.format_json = config.format_json;
            persist.bell_after_seconds = config.bell_after_seconds;
        }
        self.export_choices = config.export;
        self.import_choices = config.import;
        self.appearance = (config.theme, config.icons);
        if config.keyboard != self.keyboard {
            self.set_keyboard(config.keyboard);
        }
        self.apply_connections(config.connections);
        self.update_connection_settings();
        self.apply_agent_settings(config.agent);
    }

    fn set_keyboard(&mut self, keyboard: KeyboardMode) {
        self.keyboard = keyboard;
        for console in &mut self.consoles {
            console.editor.set_keyboard(keyboard);
        }
    }

    /// Gives a console a new editor with a text. The cursor stays when it fits.
    pub(super) fn replace_editor(&mut self, index: usize, text: &str) {
        let dialect = self.consoles[index].editor.dialect();
        let editor = self.new_editor(text, dialect);
        let console = &mut self.consoles[index];
        let was_dirty = console
            .file
            .as_ref()
            .is_some_and(|file| file.is_dirty(console.editor.revision()));
        let cursor = console.editor.cursor();
        console.editor = editor;
        let lines = text.split('\n').collect::<Vec<_>>();
        if let Some(line) = lines.get(cursor.row)
            && cursor.col <= line.chars().count()
        {
            console.editor.set_cursor(cursor);
        }
        let revision = console.editor.revision();
        if let Some(file) = &mut console.file {
            file.rebase(revision, was_dirty);
        }
    }

    /// Applies the connections of the files. A connection with a session stays as it is
    /// until it disconnects, because a load never changes an active connection.
    fn apply_connections(&mut self, items: Vec<Stored>) {
        let mut keys = HashSet::new();
        for item in items {
            let key = ConnectionKey {
                scope: item.scope,
                id: item.id.clone(),
            };
            keys.insert(key.clone());
            match self
                .connections
                .iter()
                .position(|c| c.key.as_ref() == Some(&key))
            {
                None => {
                    self.add_stored(item);
                }
                Some(index) => self.update_stored(index, item),
            }
        }
        let stale: Vec<ConnectionId> = self
            .connections
            .iter()
            .filter(|c| c.key.as_ref().is_some_and(|key| !keys.contains(key)))
            .map(|c| c.id)
            .collect();
        for id in stale {
            if !self.is_in_use(id) {
                self.drop_connection(id);
            }
        }
        refresh_labels(&mut self.connections);
        self.refresh_tree_consoles();
    }

    fn is_in_use(&self, id: ConnectionId) -> bool {
        let connected = self
            .manager
            .as_ref()
            .is_some_and(|manager| manager.is_connected(id));
        connected || self.console_in_use(id).is_some()
    }

    fn update_stored(&mut self, index: usize, item: Stored) {
        let id = self.connections[index].id;
        let mut definition = item.definition;
        let same_credential = self.connections[index].credential == item.credential;
        if let Some(current) = &mut self.connections[index].definition {
            // These values need no new session, so they apply to a connection in use too.
            current.theme = definition.theme;
            current.icon = definition.icon;
            if same_credential {
                definition.keep_password_of(current);
            }
            if *current == definition && same_credential {
                return;
            }
        }
        if !self.is_in_use(id) {
            self.replace_definition(id, definition);
            if let Some(connection) = self.connection_mut(id) {
                connection.credential = item.credential;
            }
        }
    }

    /// Writes the connection to the file of its scope. It returns the error text.
    pub(super) fn save_connection(
        &mut self,
        key: &ConnectionKey,
        definition: &Definition,
        credential: CredentialName,
    ) -> Result<(), String> {
        let mut entry = config::to_entry(&key.id, definition);
        if matches!(definition.source, crate::connection::Source::Network(_)) {
            entry.credential = credential;
        }
        self.change_config(key.scope, Change::Upsert(Box::new(entry)))
    }

    pub(super) fn change_config(&mut self, scope: Scope, change: Change) -> Result<(), String> {
        let Some(persist) = &mut self.persist else {
            return Ok(());
        };
        let Some(root) = persist.store.root(scope) else {
            return Err(text::NO_PROJECT.to_owned());
        };
        let hash = config::change_connections(root, scope, change)?;
        if let Some((_, known)) = persist.hashes.iter_mut().find(|(item, _)| *item == scope) {
            *known = Some(hash);
        }
        Ok(())
    }

    // ---- settings ----

    /// Opens the dialog with the values of the files. It needs a valid configuration.
    pub(super) fn open_settings(&mut self) {
        let Some(persist) = &self.persist else {
            self.status = Some(text::NO_STORE.to_owned());
            return;
        };
        if persist.error.is_some() {
            self.status = Some(text::CONFIG_BLOCKED.to_owned());
            return;
        }
        let global = config::preferences(&persist.store.global, Scope::Global);
        let project = persist
            .store
            .project
            .as_ref()
            .map(|root| config::preferences(root, Scope::Project));
        let project = project.transpose();
        match (global, project) {
            (Ok(global), Ok(project)) => {
                self.settings_form = Some(SettingsForm::new(global, project))
            }
            (Err(message), _) | (_, Err(message)) => self.status = Some(message),
        }
    }

    /// Writes the scope of the dialog, and it applies the files as a reload does.
    pub(super) fn save_settings(&mut self) {
        let Some(form) = &mut self.settings_form else {
            return;
        };
        let preferences = match form.preferences() {
            Ok(preferences) => preferences,
            Err(message) => {
                form.note = Some(message);
                return;
            }
        };
        let scope = form.scope;
        let Some(persist) = &mut self.persist else {
            return;
        };
        // Use the current files for the other scope, not only the dialog snapshot.
        let validation = match scope {
            Scope::Global => persist
                .store
                .project
                .as_ref()
                .map(|root| config::preferences(root, Scope::Project))
                .transpose()
                .and_then(|project| config::validate_keymaps(&preferences, project.as_ref())),
            Scope::Project => config::preferences(&persist.store.global, Scope::Global)
                .and_then(|global| config::validate_keymaps(&global, Some(&preferences))),
        };
        if let Err(message) = validation {
            form.note = Some(message);
            return;
        }
        let Some(root) = persist.store.root(scope) else {
            return;
        };
        match config::change_preferences(root, scope, preferences) {
            Ok(hash) => {
                if let Some((_, known)) = persist.hashes.iter_mut().find(|(item, _)| *item == scope)
                {
                    *known = Some(hash);
                }
                self.settings_form = None;
                self.reload_config();
                if self
                    .persist
                    .as_ref()
                    .is_some_and(|persist| persist.error.is_none())
                {
                    self.status = Some(text::SETTINGS_SAVED.to_owned());
                }
            }
            Err(message) => form.note = Some(message),
        }
    }

    /// Keeps the choices of the export dialog. A write that fails leaves a note, and
    /// the export goes on.
    pub(super) fn save_export_choices(&mut self, choices: ExportChoices) {
        if self.export_choices == choices {
            return;
        }
        self.export_choices = choices.clone();
        self.change_global(|root| config::change_export(root, choices));
    }

    /// Keeps the choices of the import dialog, as `save_export_choices` does.
    pub(super) fn save_import_choices(&mut self, choices: ImportChoices) {
        if self.import_choices == choices {
            return;
        }
        self.import_choices = choices.clone();
        self.change_global(|root| config::change_import(root, choices));
    }

    /// Writes a change to the global file, and keeps its new hash.
    fn change_global(&mut self, change: impl FnOnce(&Root) -> Result<u64, String>) {
        let Some(persist) = &mut self.persist else {
            return;
        };
        let Some(root) = persist.store.root(Scope::Global) else {
            return;
        };
        match change(root) {
            Ok(hash) => {
                let global = persist.hashes.iter_mut();
                if let Some((_, known)) =
                    global.into_iter().find(|(item, _)| *item == Scope::Global)
                {
                    *known = Some(hash);
                }
            }
            Err(message) => self.status = Some(text::export_choices_failed(&message)),
        }
    }

    // ---- consoles ----

    /// Returns the script of the console, and it creates the script at the first edit.
    pub(super) fn ensure_file(&mut self, index: usize) {
        if self.persist.is_none()
            || self.consoles[index].file.is_some()
            || self.agent.owns(self.consoles[index].id)
        {
            return;
        }
        let console = &self.consoles[index];
        let revision = console.editor.revision();
        let Some(connection) = console.connection.and_then(|id| self.connection(id)) else {
            return;
        };
        let Some(key) = connection.key.clone() else {
            return;
        };
        if revision == 0 {
            return;
        }
        if self.persist_root(key.scope).is_none() {
            return;
        }
        let id = scripts::new_id();
        let lock = None;
        let mut meta = ScriptMeta::new(id, console.name.clone(), key.id, key.scope);
        meta.schema.clone_from(&console.schema);
        let file = crate::script_file::ScriptFile::new(key.scope, meta, lock, 0);
        self.consoles[index].file = Some(file);
    }

    /// Writes a console during emergency shutdown, after the worker completes.
    pub(super) fn save_console(&mut self, index: usize, now: Instant) -> Result<(), Trouble> {
        let console = &mut self.consoles[index];
        let Some(file) = &mut console.file else {
            return Ok(());
        };
        let Some(root) = self
            .persist
            .as_ref()
            .and_then(|persist| persist.store.root(file.scope))
        else {
            return Ok(());
        };
        let text = console.editor.text();
        let revision = console.editor.revision();
        let name = console.name.clone();
        let result = file.save(root, &text, revision, now);
        if let Err(Trouble::Failed(message)) = &result {
            self.status = Some(text::save_failed(&name, message));
            // The failure is new, so the message starts its time again.
            self.status_shown = None;
        }
        result
    }

    /// The save of the key `Ctrl+S`.
    pub(super) fn save_now(&mut self) {
        if self.agent.owns(self.console().id) {
            self.status = Some(text::AGENT_CONSOLE_TEMPORARY.to_owned());
            return;
        }
        if self.persist.is_none() {
            self.status = Some(text::NOTHING_TO_SAVE.to_owned());
            return;
        }
        let index = self.active;
        self.ensure_file(index);
        let console = &self.consoles[index];
        let Some(file) = &console.file else {
            self.status = Some(text::NOTHING_TO_SAVE.to_owned());
            return;
        };
        if file.trouble.as_ref().is_some_and(Trouble::needs_choice) {
            let id = file.meta.id.clone();
            if let Some(persist) = &mut self.persist {
                persist.presented.remove(&id);
            }
            self.open_conflicts();
            return;
        }
        self.persist
            .as_mut()
            .unwrap()
            .saves
            .entry(self.consoles[index].id)
            .or_default()
            .manual = true;
        self.start_console_save(index, false);
    }

    /// Returns true while a script write or its continuation waits.
    pub fn has_pending_saves(&self) -> bool {
        self.persist
            .as_ref()
            .is_some_and(|persist| !persist.saves.is_empty())
            || self
                .consoles
                .iter()
                .any(|console| console.file.as_ref().is_some_and(|file| file.is_saving()))
    }

    pub(super) fn association_pending(&self, id: ConsoleId) -> bool {
        self.persist
            .as_ref()
            .and_then(|persist| persist.saves.get(&id))
            .is_some_and(|intent| intent.associate.is_some())
    }

    fn start_console_save(&mut self, index: usize, autosave: bool) {
        let target = self
            .persist
            .as_ref()
            .and_then(|persist| persist.saves.get(&self.consoles[index].id))
            .and_then(|intent| intent.associate)
            .and_then(|id| {
                self.connections
                    .iter()
                    .find(|connection| connection.id == id)
            })
            .and_then(|connection| connection.key.clone());
        let Some(persist) = &mut self.persist else {
            return;
        };
        let console = &mut self.consoles[index];
        let Some(file) = &mut console.file else {
            return;
        };
        if file.is_saving() || !persist.worker.ready() {
            return;
        }
        let intent = persist.saves.get_mut(&console.id);
        let overwrite = intent.as_ref().is_some_and(|intent| intent.overwrite);
        if !autosave && intent.is_none() {
            return;
        }
        if !overwrite && file.trouble.as_ref().is_some_and(Trouble::needs_choice) {
            return;
        }
        let Some(root) = persist.store.root(file.scope) else {
            return;
        };
        if let Some(intent) = intent.as_ref() {
            if intent.metadata && !overwrite && file.meta_written() {
                file.start_metadata(&persist.worker, root);
                persist.saves.get_mut(&console.id).unwrap().metadata = false;
                return;
            }
            if intent.associate.is_some() && !file.is_dirty(console.editor.revision()) {
                let Some(key) = target else {
                    persist.saves.remove(&console.id);
                    return;
                };
                let Some(to) = persist.store.root(key.scope) else {
                    return;
                };
                let mut meta = file.meta.clone();
                meta.connection_id = key.id;
                meta.connection_scope = key.scope;
                meta.schema = None;
                file.start_relocate(&persist.worker, root, to, key.scope, meta);
                persist
                    .saves
                    .get_mut(&console.id)
                    .unwrap()
                    .association_running = intent.associate;
                return;
            }
        }
        let text = console.editor.text();
        file.start_save(
            &persist.worker,
            root,
            text,
            console.editor.revision(),
            overwrite,
        );
        if let Some(intent) = intent {
            intent.overwrite = false;
        }
    }

    fn poll_saves(&mut self, now: Instant) -> bool {
        let mut completed = Vec::new();
        for console in &mut self.consoles {
            if let Some((result, metadata_name)) =
                console.file.as_mut().and_then(|file| file.poll_save(now))
            {
                completed.push((console.id, result, metadata_name));
            }
        }
        let changed = !completed.is_empty();
        for (id, result, metadata_name) in completed {
            let Some(index) = self.console_index(id) else {
                continue;
            };
            let console = &self.consoles[index];
            let id = console.id;
            let name = console.name.clone();
            let dirty = console
                .file
                .as_ref()
                .is_some_and(|file| file.is_dirty(console.editor.revision()));
            if let Err(Trouble::Failed(message)) = &result {
                self.status = Some(text::save_failed(&name, message));
                self.status_shown = None;
            }
            let intent = self
                .persist
                .as_mut()
                .and_then(|persist| persist.saves.remove(&id));
            let Some(mut intent) = intent else { continue };
            if let Err(problem) = result {
                if let Some((previous, requested)) = &intent.rename {
                    let console = &mut self.consoles[index];
                    console.name.clone_from(previous);
                    if let Some(file) = &mut console.file {
                        file.meta.name.clone_from(previous);
                    }
                    self.status = Some(text::script_failed(requested, &format!("{problem:?}")));
                    self.refresh_tree_consoles();
                }
                if intent.close {
                    self.show_unsaved(index, problem);
                } else if intent.associate.is_some() {
                    self.status = Some(text::ASSOCIATE_SAVE_FIRST.to_owned());
                }
                continue;
            }
            if let Some((previous, _)) = &mut intent.rename
                && let Some(name) = metadata_name
            {
                *previous = name;
            }
            if let Some(connection) = intent.association_running.take() {
                if intent.associate == Some(connection) {
                    intent.associate = None;
                }
                let name = self.consoles[index].name.clone();
                self.consoles[index].file.as_mut().unwrap().meta.name = name;
                if let Some(target) = self.connection(connection) {
                    let dialect = target.dialect;
                    let name = target.name.clone();
                    self.bind_keeping_name(index, connection, dialect);
                    self.status = Some(text::associated(&name));
                }
            }
            let id = self.consoles[index].id;
            if intent.metadata
                || intent.overwrite
                || intent.associate.is_some()
                || (dirty && !self.consoles[index].discard_text && (intent.manual || intent.close))
            {
                self.persist.as_mut().unwrap().saves.insert(id, intent);
                continue;
            }
            if intent.rename.is_some() {
                self.status = Some(text::CONSOLE_RENAMED.to_owned());
            }
            if intent.manual {
                self.status = Some(text::SAVED.to_owned());
            }
            if intent.close {
                self.close_step(id);
            }
        }
        changed
    }

    pub(super) fn queue_save_for_move(&mut self, index: usize, connection: ConnectionId) {
        let console = &self.consoles[index];
        if console
            .file
            .as_ref()
            .is_some_and(|file| file.trouble.is_some())
        {
            self.status = Some(text::ASSOCIATE_SAVE_FIRST.to_owned());
            return;
        }
        self.persist
            .as_mut()
            .unwrap()
            .saves
            .entry(console.id)
            .or_default()
            .associate = Some(connection);
        self.start_console_save(index, false);
    }

    pub(super) fn queue_metadata(&mut self, index: usize, rename: Option<(String, String)>) {
        let intent = self
            .persist
            .as_mut()
            .unwrap()
            .saves
            .entry(self.consoles[index].id)
            .or_default();
        intent.metadata = true;
        if let Some((previous, requested)) = rename {
            let previous = intent
                .rename
                .as_ref()
                .map_or(previous, |(old, _)| old.clone());
            intent.rename = Some((previous, requested));
        }
        self.start_console_save(index, false);
    }

    pub(super) fn rekey_saves(&mut self, previous: ConsoleId, current: ConsoleId) {
        if let Some(persist) = &mut self.persist
            && let Some(intent) = persist.saves.remove(&previous)
        {
            persist.saves.insert(current, intent);
        }
    }

    pub(super) fn cancel_save_close(&mut self, id: ConsoleId) {
        if let Some(intent) = self
            .persist
            .as_mut()
            .and_then(|persist| persist.saves.get_mut(&id))
        {
            intent.close = false;
        }
    }

    /// The step of the main loop. It ends old messages, it writes the texts after a pause
    /// in the edits, and it looks for changes made by other programs. It returns true
    /// when the screen changes.
    pub fn tick_at(&mut self, now: Instant) -> bool {
        let mut changed = self.advance_clock(now);
        changed |= self.expire_status(now);
        if self.console_on_screen() {
            let console = &mut self.consoles[self.active];
            let revision = console.editor.revision();
            let dialect = console.editor.dialect();
            let supported = console
                .connection
                .and_then(|id| self.connections.iter().find(|item| item.id == id))
                .is_none_or(|item| item.capabilities.parameters);
            let dialect = if supported {
                dialect
            } else {
                crate::sql::Dialect::Redis
            };
            changed |= console.parameters.poll(
                revision,
                dialect,
                now,
                || console.editor.text(),
                |operation| {
                    if let Some(manager) = &self.manager {
                        manager.spawn_blocking(operation);
                    }
                },
            );
            if console.parameters.fields.is_empty()
                && console.focus == crate::console::Focus::Parameters
            {
                console.focus = crate::console::Focus::Editor;
            }
        }
        if self.persist.is_none() {
            return changed;
        }
        changed |= self.poll_saves(now);
        for index in 0..self.consoles.len() {
            self.ensure_file(index);
            let console = &mut self.consoles[index];
            let revision = console.editor.revision();
            let Some(file) = &mut console.file else {
                continue;
            };
            let due = file.due(revision, now);
            self.start_console_save(index, due);
        }
        if self
            .persist
            .as_ref()
            .is_some_and(|persist| now >= persist.next_poll)
        {
            changed |= self.poll_files(now);
        }
        changed
    }

    pub fn tick(&mut self) -> bool {
        self.tick_at(Instant::now())
    }

    fn poll_files(&mut self, now: Instant) -> bool {
        if let Some(persist) = &mut self.persist {
            persist.next_poll = now + POLL_EVERY;
        }
        let mut changed = false;
        for index in 0..self.consoles.len() {
            let Some(file) = &mut self.consoles[index].file else {
                continue;
            };
            let Some(root) = self
                .persist
                .as_ref()
                .and_then(|persist| persist.store.root(file.scope))
            else {
                continue;
            };
            let before = file.trouble.clone();
            file.poll(root);
            changed |= before != file.trouble;
        }
        changed |= self.poll_config();
        changed |= self.open_conflicts();
        changed
    }

    fn poll_config(&mut self) -> bool {
        let Some(persist) = &mut self.persist else {
            return false;
        };
        if persist.config_prompt {
            return false;
        }
        let mut changed = false;
        for (scope, root) in persist.store.scopes() {
            let hash = std::fs::read(root.config_file())
                .ok()
                .map(|bytes| file::hash(&bytes));
            if let Some((_, known)) = persist.hashes.iter_mut().find(|(item, _)| *item == scope)
                && *known != hash
            {
                *known = hash;
                changed = true;
            }
        }
        if changed {
            persist.config_prompt = true;
            self.show_dialog(
                text::TITLE_CONFIG_CHANGED,
                text::BODY_CONFIG_CHANGED.to_owned(),
                vec![
                    (text::OPTION_IGNORE, DialogAction::IgnoreConfigChange),
                    (text::OPTION_RELOAD, DialogAction::ReloadConfig),
                ],
            );
        }
        changed
    }

    pub(super) fn ignore_config_change(&mut self) {
        if let Some(persist) = &mut self.persist {
            persist.config_prompt = false;
        }
    }

    // ---- close and exit ----

    /// Writes the pending text before a close. It returns false after it asked a
    /// question, because the text has no safe place to go.
    pub(super) fn flush_console(&mut self, index: usize) -> bool {
        if self.persist.is_none() {
            return true;
        }
        if self.consoles[index].discard_text {
            let console = &self.consoles[index];
            let saving = console.file.as_ref().is_some_and(|file| file.is_saving());
            let persist = self.persist.as_mut().unwrap();
            persist.saves.remove(&console.id);
            if saving {
                persist.saves.entry(console.id).or_default().close = true;
                return false;
            }
            return true;
        }
        self.ensure_file(index);
        let console = &self.consoles[index];
        let revision = console.editor.revision();
        let id = console.id;
        let saving = console.file.as_ref().is_some_and(|file| file.is_saving())
            || self
                .persist
                .as_ref()
                .and_then(|persist| persist.saves.get(&id))
                .is_some_and(|intent| intent.overwrite);
        if saving {
            self.queue_save_close(index);
            return false;
        }
        let problem = match &console.file {
            Some(file) if file.trouble.as_ref().is_some_and(Trouble::needs_choice) => {
                file.trouble.clone()
            }
            Some(file) if file.is_dirty(revision) => {
                self.queue_save_close(index);
                return false;
            }
            Some(_) => None,
            None if console.editor.text().is_empty() => None,
            None => Some(Trouble::Failed(text::NO_PLACE.to_owned())),
        };
        let Some(problem) = problem else { return true };
        self.show_unsaved(index, problem);
        false
    }

    fn queue_save_close(&mut self, index: usize) {
        self.persist
            .as_mut()
            .unwrap()
            .saves
            .entry(self.consoles[index].id)
            .or_default()
            .close = true;
        self.start_console_save(index, false);
    }

    fn show_unsaved(&mut self, index: usize, problem: Trouble) {
        let console = &self.consoles[index];
        let name = &console.name;
        let id = console.id;
        let body = match &problem {
            Trouble::Failed(message) => text::unsaved_failed(name, message),
            Trouble::Changed => text::unsaved_changed(name),
            Trouble::Removed => text::unsaved_removed(name),
        };
        let discard = if problem == Trouble::Removed {
            text::OPTION_CLOSE_DISCARD
        } else {
            text::OPTION_CLOSE_UNSAVED
        };
        self.show_dialog(
            text::TITLE_UNSAVED,
            body,
            vec![
                (text::OPTION_CANCEL, DialogAction::CancelClose(id)),
                (discard, DialogAction::DiscardAndClose(id)),
            ],
        );
    }

    /// Writes what it can for a signal or a panic. These cases allow no question, so a
    /// console that waits for a choice keeps its file as it is.
    pub fn save_without_questions(&mut self) {
        if self.persist.is_none() {
            return;
        }
        let now = Instant::now();
        for console in &mut self.consoles {
            if let Some(file) = &mut console.file {
                let _ = file.wait_save(now);
            }
        }
        for index in 0..self.consoles.len() {
            self.ensure_file(index);
            let intent = self
                .persist
                .as_mut()
                .and_then(|persist| persist.saves.remove(&self.consoles[index].id));
            if let Some(intent) = intent
                && let Some(file) = &mut self.consoles[index].file
                && let Some(root) = self
                    .persist
                    .as_ref()
                    .and_then(|persist| persist.store.root(file.scope))
            {
                if intent.metadata {
                    let _ = file.write_meta(root);
                }
                if intent.overwrite {
                    let console = &mut self.consoles[index];
                    let text = console.editor.text();
                    let revision = console.editor.revision();
                    let _ = console
                        .file
                        .as_mut()
                        .unwrap()
                        .overwrite(root, &text, revision);
                }
            }
            let console = &self.consoles[index];
            let revision = console.editor.revision();
            let waits = |file: &crate::script_file::ScriptFile| {
                file.trouble.as_ref().is_some_and(Trouble::needs_choice) || !file.is_dirty(revision)
            };
            if console.file.as_ref().is_some_and(|file| !waits(file)) {
                let _ = self.save_console(index, now);
            }
        }
        self.save_editor_states(None);
        self.save_open_tabs();
    }

    /// Writes the pending text of every console. It stops at the first question.
    pub(super) fn flush_all(&mut self) -> bool {
        for index in 0..self.consoles.len() {
            if !self.flush_console(index) {
                self.select_console(index);
                return false;
            }
        }
        true
    }

    /// Writes the cursor and the scroll of the consoles that have a script.
    pub(super) fn save_editor_states(&mut self, only: Option<usize>) {
        for scope in [Scope::Global, Scope::Project] {
            let Some(root) = self.persist_root(scope) else {
                continue;
            };
            let states: Vec<(String, EditorState)> = self
                .consoles
                .iter()
                .enumerate()
                .filter(|(index, _)| only.is_none_or(|wanted| wanted == *index))
                .filter_map(|(_, console)| {
                    let file = console.file.as_ref().filter(|file| file.scope == scope)?;
                    Some((file.meta.id.clone(), editor_state(&console.editor)))
                })
                .collect();
            if !states.is_empty() {
                let _ = workspace::update(&root, |workspace| workspace.consoles.extend(states));
            }
        }
    }

    pub(super) fn layout_root(&self) -> Option<Root> {
        self.persist_root(Scope::Project)
            .or_else(|| self.persist_root(Scope::Global))
    }

    fn restore_layout(&mut self) {
        let roots = [
            self.persist_root(Scope::Project),
            self.persist_root(Scope::Global),
        ];
        if roots.iter().flatten().any(workspace::is_unreadable) {
            self.status = Some(text::WORKSPACE_UNREADABLE.to_owned());
        }
        let [project, global] = roots.map(|root| root.map(|root| workspace::load(&root)));
        let layout = project
            .and_then(|item| item.layout)
            .or_else(|| global.and_then(|item| item.layout));
        let Some(layout) = layout else { return };
        self.navigator.collapsed = layout.navigator_collapsed;
        self.navigator.width = layout.navigator_width.max(crate::navigator::MIN_WIDTH);
        let percent = (layout.editor_ratio * 100.0).round().clamp(0.0, 100.0);
        // The value is between 0 and 100, so the conversion keeps it.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let percent = percent as u16;
        self.editor_ratio = percent.clamp(super::MIN_EDITOR_RATIO, super::MAX_EDITOR_RATIO);
    }

    /// Writes the state of the session. The application calls it once, at the end.
    pub(super) fn save_session(&mut self) {
        if self.persist.is_none() {
            return;
        }
        self.save_editor_states(None);
        self.save_open_tabs();
        let Some(root) = self.layout_root() else {
            return;
        };
        let layout = workspace::Layout {
            navigator_collapsed: self.navigator.collapsed,
            navigator_width: self.navigator.width,
            editor_ratio: f64::from(self.editor_ratio) / 100.0,
        };
        let _ = workspace::update(&root, |workspace| workspace.layout = Some(layout));
    }

    /// Writes the identity file again after a change of the schema of a console.
    pub(super) fn save_console_meta(&mut self, index: usize) {
        let console = &mut self.consoles[index];
        let Some(file) = &mut console.file else {
            return;
        };
        file.meta.schema.clone_from(&console.schema);
        let Some(root) = self
            .persist
            .as_ref()
            .and_then(|persist| persist.store.root(file.scope))
        else {
            return;
        };
        if !file.meta_written() && !file.is_saving() {
            return;
        }
        let _ = root;
        self.queue_metadata(index, None);
    }

    /// Replaces the text of a console with the text of its file. The user chose it.
    pub(super) fn reload_script(&mut self, index: usize) {
        let Some(file) = &mut self.consoles[index].file else {
            return;
        };
        let Some(root) = self
            .persist
            .as_ref()
            .and_then(|persist| persist.store.root(file.scope))
        else {
            return;
        };
        match file.reload(root) {
            Ok(text) => self.replace_editor(index, &text),
            Err(error) => {
                let name = self.consoles[index].name.clone();
                self.status = Some(text::save_failed(&name, &error.to_string()));
            }
        }
    }

    /// Writes the text of a console over its file. The user chose it.
    pub(super) fn overwrite_script(&mut self, index: usize) {
        let console = &mut self.consoles[index];
        let Some(file) = &mut console.file else {
            return;
        };
        let Some(root) = self
            .persist
            .as_ref()
            .and_then(|persist| persist.store.root(file.scope))
        else {
            return;
        };
        let _ = root;
        self.persist
            .as_mut()
            .unwrap()
            .saves
            .entry(self.consoles[index].id)
            .or_default()
            .overwrite = true;
        self.start_console_save(index, false);
    }

    pub(super) fn connection_by_key(&self, scope: Scope, id: &str) -> Option<&Connection> {
        self.connections.iter().find(|c| {
            c.key
                .as_ref()
                .is_some_and(|key| key.scope == scope && key.id == id)
        })
    }
}
