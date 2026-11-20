//! The files of the running application: the configuration, the autosave of the
//! consoles, the changes that other programs make, and the state of the session.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::history::HistoryLog;
use super::{App, DialogAction, Options};
use crate::arrange::Arrangement;
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
use crate::ui::schemes::Scheme;
use crate::ui::text;
use crate::ui::theme::{CustomTheme, ThemeCatalog};

mod saves;

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
    /// The hash of the themes folder of each scope at the last load or check.
    pub(super) theme_hashes: Vec<(Scope, Option<u64>)>,
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

/// Reads the custom themes of both scopes. A project id wins over a global id
/// with the same name, and a file that cannot load gives a note.
pub(super) fn load_themes(store: &Store) -> (ThemeCatalog, Vec<String>) {
    let mut catalog = ThemeCatalog::default();
    let mut notes = Vec::new();
    for (scope, root) in store.scopes() {
        let (themes, mut trouble) = crate::store::themes::load(root);
        notes.append(&mut trouble);
        for theme in themes {
            catalog.insert(CustomTheme {
                id: theme.id,
                name: theme.name,
                scheme: Scheme::from_colors(&theme.colors),
                scope,
                file: Some(theme.hash),
            });
        }
    }
    (catalog, notes)
}

/// The manifest hash of the themes folder of each scope.
pub(super) fn theme_manifests(store: &Store) -> Vec<(Scope, Option<u64>)> {
    store
        .scopes()
        .into_iter()
        .map(|(scope, root)| (scope, crate::store::themes::manifest(root)))
        .collect()
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
        let (custom_themes, theme_notes) = load_themes(&store);
        let theme_hashes = theme_manifests(&store);
        options.custom_themes = custom_themes;
        options.theme_notes = theme_notes;
        let persist = Self {
            worker: SaveWorker::new(),
            saves: HashMap::new(),
            store,
            hashes,
            theme_hashes,
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
        options.explorer_side = config.explorer_side;
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
        crate::arrange::sort_by_scope(&mut self.connections);
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
        let (custom_themes, notes) = load_themes(&persist.store);
        self.appearance.custom_themes = custom_themes;
        persist.theme_hashes = theme_manifests(&persist.store);
        match config {
            Err(error) => {
                persist.error = Some(error.clone());
                self.show_config_error(&error);
            }
            Ok(config) => {
                persist.error = None;
                self.apply_config(config);
                self.status = Some(text::CONFIG_RELOADED.to_owned());
                self.missing_theme_note();
                if let Some(note) = notes.first() {
                    self.status = Some(note.clone());
                }
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
        for console in &mut self.tabs.consoles {
            console.editor.set_undo_limit(config.undo_limit_bytes);
        }
        self.input.pending_key = None;
        self.input.command_page = None;
        self.keymap = config.keymap;
        self.suggestion_keys = config.suggestion_keys;
        self.explorer_side = config.explorer_side;
        self.show_system_commands = config.show_system_commands;
        if let Some(persist) = &mut self.persist {
            persist.query_history = config.query_history;
            persist.format_json = config.format_json;
            persist.bell_after_seconds = config.bell_after_seconds;
        }
        self.choices.export = config.export;
        self.choices.import = config.import;
        self.appearance.choices = (config.theme, config.icons);
        if config.keyboard != self.keyboard {
            self.set_keyboard(config.keyboard);
        }
        self.apply_connections(config.connections);
        self.update_connection_settings();
        self.apply_agent_settings(config.agent);
    }

    fn set_keyboard(&mut self, keyboard: KeyboardMode) {
        self.keyboard = keyboard;
        for console in &mut self.tabs.consoles {
            console.editor.set_keyboard(keyboard);
        }
    }

    /// Gives a console a new editor with a text. The cursor stays when it fits.
    pub(super) fn replace_editor(&mut self, index: usize, text: &str) {
        let dialect = self.tabs.consoles[index].editor.dialect();
        let editor = self.new_editor(text, dialect);
        let console = &mut self.tabs.consoles[index];
        let was_dirty = console
            .file
            .as_ref()
            .is_some_and(|file| file.is_dirty(console.editor.revision()))
            || console
                .external
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
        if let Some(external) = &mut console.external {
            external.rebase(revision, was_dirty);
        }
    }

    /// Applies the connections of the files. A connection with a session stays as it is
    /// until it disconnects, because a load never changes an active connection.
    fn apply_connections(&mut self, items: Vec<Stored>) {
        let mut keys = HashSet::new();
        let order: HashMap<ConnectionKey, usize> = items
            .iter()
            .enumerate()
            .map(|(place, item)| {
                let key = ConnectionKey {
                    scope: item.scope,
                    id: item.id.clone(),
                };
                (key, place)
            })
            .collect();
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
        // The list follows the files, also after a change by another program. A
        // connection without a file stays first, and one that left the files goes last.
        self.connections
            .sort_by_key(|connection| match &connection.key {
                None => 0,
                Some(key) => order.get(key).map_or(usize::MAX, |place| place + 1),
            });
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
            current.theme = definition.theme.clone();
            current.icon = definition.icon;
            current.folder.clone_from(&definition.folder);
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

    /// Writes a new order of the connections of one scope, then shows it. A refusal or a
    /// failed write changes nothing and shows the reason. It returns true on success.
    pub(super) fn arrange(&mut self, arrangement: Result<Arrangement, &'static str>) -> bool {
        let arrangement = match arrangement {
            Ok(arrangement) => arrangement,
            Err(reason) => {
                self.status = Some(reason.to_owned());
                return false;
            }
        };
        if arrangement == crate::arrange::current(&self.connections, arrangement.scope) {
            return true;
        }
        if self.config_blocked() {
            return false;
        }
        let change = Change::Arrange(arrangement.order.clone());
        if let Err(message) = self.change_config(arrangement.scope, change) {
            self.status = Some(text::arrange_failed(&message));
            return false;
        }
        crate::arrange::apply(&mut self.connections, &arrangement);
        refresh_labels(&mut self.connections);
        true
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
                let (custom, notes) = load_themes(&persist.store);
                if let Some(note) = notes.first() {
                    self.status = Some(note.clone());
                }
                let used = self
                    .connections
                    .iter()
                    .filter_map(|connection| {
                        let theme = connection.definition.as_ref()?.theme.clone()?;
                        theme.custom_id().map(str::to_owned)
                    })
                    .collect();
                self.modals.settings_form =
                    Some(SettingsForm::with_themes(global, project, custom, used))
            }
            (Err(message), _) | (_, Err(message)) => self.status = Some(message),
        }
    }

    /// Keeps the choices of the export dialog. A write that fails leaves a note, and
    /// the export goes on.
    pub(super) fn save_export_choices(&mut self, choices: ExportChoices) {
        if self.choices.export == choices {
            return;
        }
        self.choices.export = choices.clone();
        self.change_global(|root| config::change_export(root, choices));
    }

    /// Keeps the choices of the import dialog, as `save_export_choices` does.
    pub(super) fn save_import_choices(&mut self, choices: ImportChoices) {
        if self.choices.import == choices {
            return;
        }
        self.choices.import = choices.clone();
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

    /// The step of the main loop. It ends old messages, it writes the texts after a pause
    /// in the edits, and it looks for changes made by other programs. It returns true
    /// when the screen changes.
    pub fn tick_at(&mut self, now: Instant) -> bool {
        let mut changed = self.advance_clock(now);
        changed |= self.expire_status(now);
        changed |= self.auto_refresh_tick(now);
        if self.console_on_screen() {
            let console = &mut self.tabs.consoles[self.tabs.active];
            if let Some(manager) = &self.manager {
                changed |= console
                    .editor
                    .poll_syntax(now, |operation| manager.spawn_blocking(operation));
            }
            let revision = console.editor.revision();
            let dialect = self.parameter_dialect();
            let console = &mut self.tabs.consoles[self.tabs.active];
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
        for index in 0..self.tabs.consoles.len() {
            self.ensure_file(index);
            let console = &mut self.tabs.consoles[index];
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

    /// The rules of the parameters of the console on screen. A connection without
    /// parameters uses the rules of a command line, which find none.
    pub(super) fn parameter_dialect(&self) -> crate::sql::Dialect {
        let console = self.console();
        let supported = console
            .connection
            .and_then(|id| self.connections.iter().find(|item| item.id == id))
            .is_none_or(|item| item.capabilities.parameters);
        if supported {
            console.editor.dialect()
        } else {
            crate::sql::Dialect::Redis
        }
    }

    /// The moments of the file steps of `tick_at`: autosaves, retries, and the check for
    /// changes by other programs. A save in progress does not wake the loop, so it asks
    /// for a short poll.
    pub(super) fn file_moments(&self, now: Instant) -> Vec<Instant> {
        let Some(persist) = &self.persist else {
            return Vec::new();
        };
        let mut moments = vec![persist.next_poll];
        for file in self
            .tabs
            .consoles
            .iter()
            .filter_map(|console| console.file.as_ref())
        {
            if file.is_saving() {
                moments.push(now);
            }
            moments.extend(file.due_at());
        }
        moments
    }

    pub fn tick(&mut self) -> bool {
        self.tick_at(Instant::now())
    }

    fn poll_files(&mut self, now: Instant) -> bool {
        if let Some(persist) = &mut self.persist {
            persist.next_poll = now + POLL_EVERY;
        }
        let mut changed = false;
        for index in 0..self.tabs.consoles.len() {
            let Some(file) = &mut self.tabs.consoles[index].file else {
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
            let manifest = crate::store::themes::manifest(root);
            if let Some((_, known)) = persist
                .theme_hashes
                .iter_mut()
                .find(|(item, _)| *item == scope)
                && *known != manifest
            {
                *known = manifest;
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

    /// Writes the cursor and the scroll of the consoles that have a script.
    pub(super) fn save_editor_states(&mut self, only: Option<usize>) {
        for scope in [Scope::Global, Scope::Project] {
            let Some(root) = self.persist_root(scope) else {
                continue;
            };
            let states: Vec<(String, EditorState)> = self
                .tabs
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
        // The same file as the layout holds the closed folders.
        let saved = project
            .filter(|item| item.layout.is_some())
            .or(global.filter(|item| item.layout.is_some()));
        if let Some(saved) = &saved {
            self.navigator
                .set_closed_folders(saved.closed_folders.iter().cloned());
        }
        let layout = saved.and_then(|item| item.layout);
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
        let closed = self.navigator.closed_folders(&self.connections);
        let _ = workspace::update(&root, |workspace| {
            workspace.layout = Some(layout);
            workspace.closed_folders = closed;
        });
    }

    /// Writes the identity file again after a change of the schema of a console.
    pub(super) fn save_console_meta(&mut self, index: usize) {
        let console = &mut self.tabs.consoles[index];
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
        let Some(file) = &mut self.tabs.consoles[index].file else {
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
                let name = self.tabs.consoles[index].name.clone();
                self.status = Some(text::save_failed(&name, &error.to_string()));
            }
        }
    }

    /// Writes the text of a console over its file. The user chose it.
    pub(super) fn overwrite_script(&mut self, index: usize) {
        let console = &mut self.tabs.consoles[index];
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
        if let Some(persist) = self.persist.as_mut() {
            persist
                .saves
                .entry(self.tabs.consoles[index].id)
                .or_default()
                .overwrite = true;
        }
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
