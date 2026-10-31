//! The files of the running application: the configuration, the autosave of the
//! consoles, the changes that other programs make, and the state of the session.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::{App, DialogAction, Options};
use crate::connection::{Connection, ConnectionKey, Definition, refresh_labels};
use crate::editor::{KeyboardMode, SqlEditor};
use crate::exec::ConnectionId;
use crate::script_file::Trouble;
use crate::store::config::{self, Change, Config, ConfigError, CredentialName, Loaded, Stored};
use crate::store::file;
use crate::store::scripts::{self, ScriptMeta};
use crate::store::workspace::{self, EditorState, Position};
use crate::store::{self, Root, Scope, Store};
use crate::ui::text;

/// The time between two checks of the files for a change by another program.
const POLL_EVERY: Duration = Duration::from_secs(1);

pub(super) struct Persist {
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
}

impl Persist {
    /// Reads the configuration for the start. A good configuration sets the preferences
    /// in the options. It returns the saved connections to add.
    pub(super) fn start(options: &mut Options) -> Option<(Self, Vec<Stored>)> {
        let store = options.store.take()?;
        let Loaded { hashes, config } = config::load(&store);
        let mut error = None;
        let mut stored = Vec::new();
        match config {
            Ok(config) => {
                stored = Self::apply_preferences(options, config);
            }
            Err(failure) => error = Some(failure),
        }
        let persist = Self {
            store,
            hashes,
            error,
            presented: HashSet::new(),
            next_poll: Instant::now() + POLL_EVERY,
            config_prompt: false,
            project_offer: options.offer_project.take(),
        };
        Some((persist, stored))
    }

    fn apply_preferences(options: &mut Options, config: Config) -> Vec<Stored> {
        let settings = &mut options.settings;
        settings.page_size = config.page_size.unwrap_or(settings.page_size);
        settings.row_cap = config.row_cap.unwrap_or(settings.row_cap);
        settings.budget_bytes = config.budget_bytes.unwrap_or(settings.budget_bytes);
        options.keyboard = config.keyboard;
        options.keymap = Some(config.keymap);
        config.connections
    }
}

fn editor_state(editor: &SqlEditor) -> EditorState {
    let cursor = editor.cursor();
    let (column, row) = editor.scroll();
    EditorState {
        cursor: Position { row: cursor.row, col: cursor.col },
        scroll: Position { row, col: column },
    }
}

impl App {
    pub(super) fn persist_root(&self, scope: Scope) -> Option<Root> {
        self.persist.as_ref()?.store.root(scope).cloned()
    }

    /// Returns true when a project is open, so a connection can have its scope.
    pub(super) fn has_project(&self) -> bool {
        self.persist.as_ref().is_some_and(|persist| persist.store.project.is_some())
    }

    /// Returns true, with a message, while an invalid configuration blocks the databases.
    pub(super) fn config_blocked(&mut self) -> bool {
        let blocked = self.persist.as_ref().is_some_and(|persist| persist.error.is_some());
        if blocked {
            self.status = Some(text::CONFIG_BLOCKED.to_owned());
        }
        blocked
    }

    /// The last steps of the start: old temporary files, the layout, and the questions.
    pub(super) fn finish_start(&mut self) {
        let Some(persist) = &mut self.persist else { return };
        for (_, root) in persist.store.scopes() {
            file::remove_old_temporary_files(&root.dir);
            file::remove_old_temporary_files(&root.consoles_dir());
        }
        let offer = persist.project_offer.clone();
        let error = persist.error.clone();
        self.restore_layout();
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
        let Some(persist) = &mut self.persist else { return };
        let Some(folder) = persist.project_offer.take() else { return };
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
            connection.key = Some(ConnectionKey { scope: item.scope, id: item.id });
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
        self.settings.page_size = config.page_size.unwrap_or(self.settings.page_size);
        self.settings.row_cap = config.row_cap.unwrap_or(self.settings.row_cap);
        if let Some(bytes) = config.budget_bytes {
            self.settings.budget_bytes = bytes;
            self.budget.set_limit(bytes);
        }
        self.keymap = config.keymap;
        if config.keyboard != self.keyboard {
            self.set_keyboard(config.keyboard);
        }
        self.apply_connections(config.connections);
    }

    fn set_keyboard(&mut self, keyboard: KeyboardMode) {
        self.keyboard = keyboard;
        for index in 0..self.consoles.len() {
            let text = self.consoles[index].editor.text();
            self.replace_editor(index, &text);
        }
    }

    /// Gives a console a new editor with a text. The cursor stays when it fits.
    pub(super) fn replace_editor(&mut self, index: usize, text: &str) {
        let console = &mut self.consoles[index];
        let was_dirty = console.file.as_ref().is_some_and(|file| file.is_dirty(console.editor.revision()));
        let cursor = console.editor.cursor();
        let dialect = console.editor.dialect();
        console.editor = SqlEditor::new(text, self.keyboard, dialect, self.clipboard.clone());
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
            let key = ConnectionKey { scope: item.scope, id: item.id.clone() };
            keys.insert(key.clone());
            match self.connections.iter().position(|c| c.key.as_ref() == Some(&key)) {
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
    }

    fn is_in_use(&self, id: ConnectionId) -> bool {
        let connected = self.manager.as_ref().is_some_and(|manager| manager.is_connected(id));
        connected || self.console_in_use(id).is_some()
    }

    fn update_stored(&mut self, index: usize, item: Stored) {
        let id = self.connections[index].id;
        let mut definition = item.definition;
        let same_credential = self.connections[index].credential == item.credential;
        if let Some(current) = &self.connections[index].definition {
            if same_credential { definition.keep_password_of(current); }
            if *current == definition && same_credential { return; }
        }
        if !self.is_in_use(id) {
            self.replace_definition(id, definition);
            if let Some(connection) = self.connection_mut(id) { connection.credential = item.credential; }
        }
    }

    /// Writes the connection to the file of its scope. It returns the error text.
    pub(super) fn save_connection(&mut self, key: &ConnectionKey, definition: &Definition, credential: CredentialName) -> Result<(), String> {
        let mut entry = config::to_entry(&key.id, definition);
        if matches!(definition.source, crate::connection::Source::Network(_)) { entry.credential = credential; }
        self.change_config(key.scope, Change::Upsert(Box::new(entry)))
    }

    pub(super) fn change_config(&mut self, scope: Scope, change: Change) -> Result<(), String> {
        let Some(persist) = &mut self.persist else { return Ok(()) };
        let Some(root) = persist.store.root(scope) else {
            return Err(text::NO_PROJECT.to_owned());
        };
        let hash = config::change_connections(root, scope, change)?;
        if let Some((_, known)) = persist.hashes.iter_mut().find(|(item, _)| *item == scope) {
            *known = Some(hash);
        }
        Ok(())
    }

    // ---- consoles ----

    /// Returns the script of the console, and it creates the script at the first edit.
    pub(super) fn ensure_file(&mut self, index: usize) {
        if self.persist.is_none() || self.consoles[index].file.is_some() {
            return;
        }
        let console = &self.consoles[index];
        let revision = console.editor.revision();
        let Some(connection) = console.connection.and_then(|id| self.connection(id)) else { return };
        let Some(key) = connection.key.clone() else { return };
        if revision == 0 {
            return;
        }
        let Some(root) = self.persist_root(key.scope) else { return };
        let id = scripts::new_id();
        let lock = scripts::lock(&root, &id).ok().flatten();
        let mut meta = ScriptMeta::new(id, console.name.clone(), key.id, key.scope);
        meta.schema.clone_from(&console.schema);
        let file = crate::script_file::ScriptFile::new(key.scope, meta, lock, 0);
        self.consoles[index].file = Some(file);
    }

    /// Writes the text of a console at once.
    pub(super) fn save_console(&mut self, index: usize, now: Instant) -> Result<(), Trouble> {
        let console = &mut self.consoles[index];
        let Some(file) = &mut console.file else { return Ok(()) };
        let Some(root) = self.persist.as_ref().and_then(|persist| persist.store.root(file.scope)) else {
            return Ok(());
        };
        let text = console.editor.text();
        let revision = console.editor.revision();
        let name = console.name.clone();
        let result = file.save(root, &text, revision, now);
        if let Err(Trouble::Failed(message)) = &result {
            self.status = Some(text::save_failed(&name, message));
        }
        result
    }

    /// The save of the key `Ctrl+S`.
    pub(super) fn save_now(&mut self) {
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
        if self.save_console(index, Instant::now()).is_ok() {
            self.status = Some(text::SAVED.to_owned());
        }
    }

    /// The step of the main loop. It writes the texts after a pause in the edits,
    /// and it looks for changes made by other programs. It returns true when the
    /// screen changes.
    pub fn tick_at(&mut self, now: Instant) -> bool {
        if self.persist.is_none() {
            return false;
        }
        let mut changed = false;
        for index in 0..self.consoles.len() {
            self.ensure_file(index);
            let console = &mut self.consoles[index];
            let revision = console.editor.revision();
            let Some(file) = &mut console.file else { continue };
            let before = file.trouble.clone();
            if file.due(revision, now) {
                let _ = self.save_console(index, now);
            }
            let after = self.consoles[index].file.as_ref().and_then(|file| file.trouble.clone());
            changed |= before != after;
        }
        if self.persist.as_ref().is_some_and(|persist| now >= persist.next_poll) {
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
            let Some(file) = &mut self.consoles[index].file else { continue };
            let Some(root) = self.persist.as_ref().and_then(|persist| persist.store.root(file.scope)) else {
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
        let Some(persist) = &mut self.persist else { return false };
        if persist.config_prompt {
            return false;
        }
        let mut changed = false;
        for (scope, root) in persist.store.scopes() {
            let hash = std::fs::read(root.config_file()).ok().map(|bytes| file::hash(&bytes));
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
        if self.persist.is_none() || self.consoles[index].discard_text {
            return true;
        }
        self.ensure_file(index);
        let console = &self.consoles[index];
        let revision = console.editor.revision();
        let name = console.name.clone();
        let id = console.id;
        let problem = match &console.file {
            Some(file) if file.trouble.as_ref().is_some_and(Trouble::needs_choice) => {
                file.trouble.clone()
            }
            Some(file) if file.is_dirty(revision) => self.save_console(index, Instant::now()).err(),
            Some(_) => None,
            None if console.editor.text().is_empty() => None,
            None => Some(Trouble::Failed(text::NO_PLACE.to_owned())),
        };
        let Some(problem) = problem else { return true };
        let body = match &problem {
            Trouble::Failed(message) => text::unsaved_failed(&name, message),
            Trouble::Changed => text::unsaved_changed(&name),
            Trouble::Removed => text::unsaved_removed(&name),
        };
        self.show_dialog(
            text::TITLE_UNSAVED,
            body,
            vec![
                (text::OPTION_CANCEL, DialogAction::CancelClose(id)),
                (text::OPTION_CLOSE_UNSAVED, DialogAction::DiscardAndClose(id)),
            ],
        );
        false
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
            let Some(root) = self.persist_root(scope) else { continue };
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

    fn layout_root(&self) -> Option<Root> {
        self.persist_root(Scope::Project).or_else(|| self.persist_root(Scope::Global))
    }

    fn restore_layout(&mut self) {
        let project = self.persist_root(Scope::Project).map(|root| workspace::load(&root));
        let global = self.persist_root(Scope::Global).map(|root| workspace::load(&root));
        let layout = project.and_then(|item| item.layout).or_else(|| global.and_then(|item| item.layout));
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
        let Some(root) = self.layout_root() else { return };
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
        let Some(file) = &mut console.file else { return };
        file.meta.schema.clone_from(&console.schema);
        let Some(root) = self.persist.as_ref().and_then(|persist| persist.store.root(file.scope)) else {
            return;
        };
        if let Err(message) = file.write_meta(root) {
            self.status = Some(text::save_failed(&console.name, &message));
        }
    }

    /// Replaces the text of a console with the text of its file. The user chose it.
    pub(super) fn reload_script(&mut self, index: usize) {
        let Some(file) = &mut self.consoles[index].file else { return };
        let Some(root) = self.persist.as_ref().and_then(|persist| persist.store.root(file.scope)) else {
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
        let Some(file) = &mut console.file else { return };
        let Some(root) = self.persist.as_ref().and_then(|persist| persist.store.root(file.scope)) else {
            return;
        };
        let text = console.editor.text();
        if let Err(Trouble::Failed(message)) = file.overwrite(root, &text, console.editor.revision()) {
            self.status = Some(text::save_failed(&console.name, &message));
        }
    }

    pub(super) fn connection_by_key(&self, scope: Scope, id: &str) -> Option<&Connection> {
        self.connections
            .iter()
            .find(|c| c.key.as_ref().is_some_and(|key| key.scope == scope && key.id == id))
    }
}
