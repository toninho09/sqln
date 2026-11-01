//! The saved consoles: the list that opens them again, the scripts without a
//! connection, the removal of a script, and the association with a connection.

use std::time::{Instant, SystemTime};

use edtui::Index2;

use super::{App, DialogAction, Picker, PickerKind};
use crate::connection::Connection;
use crate::editor::SqlEditor;
use crate::exec::{ConnectionId, ConsoleId};
use crate::script_file::{ScriptFile, Trouble};
use crate::store::file::Expect;
use crate::store::scripts::{self, ScriptEntry};
use crate::store::workspace;
use crate::store::{Root, Scope};
use crate::ui::name::{NamePrompt, NameTarget};
use crate::ui::text;

/// Writes the age of a change, so the list needs no clock zone.
pub(super) fn age(modified: Option<SystemTime>) -> String {
    let Some(seconds) = modified
        .and_then(|time| SystemTime::now().duration_since(time).ok())
        .map(|elapsed| elapsed.as_secs())
    else {
        return String::new();
    };
    match seconds {
        0..=59 => text::AGE_NOW.to_owned(),
        60..=3_599 => text::age_minutes(seconds / 60),
        3_600..=86_399 => text::age_hours(seconds / 3_600),
        _ => text::age_days(seconds / 86_400),
    }
}

fn scope_word(scope: Scope) -> &'static str {
    match scope {
        Scope::Global => text::VALUE_SCOPE_GLOBAL,
        Scope::Project => text::VALUE_SCOPE_PROJECT,
    }
}

impl App {
    fn is_script_open(&self, scope: Scope, id: &str) -> bool {
        self.consoles.iter().any(|c| {
            c.file
                .as_ref()
                .is_some_and(|file| file.scope == scope && file.meta.id == id)
        })
    }

    fn script_row(&self, entry: &ScriptEntry, show_scope: bool) -> String {
        let mut row = format!("{:<22} {}", entry.meta.name, age(entry.modified));
        if show_scope {
            row = format!("{row} · {}", scope_word(entry.scope));
        }
        if self.is_script_open(entry.scope, &entry.meta.id) {
            row.push_str(text::MARK_OPEN);
        }
        row
    }

    /// Lists the saved consoles of a connection, the newest first.
    pub(super) fn consoles_of(&self, connection: ConnectionId) -> Vec<ScriptEntry> {
        let Some(key) = self
            .connection(connection)
            .and_then(|item| item.key.clone())
        else {
            return Vec::new();
        };
        let Some(root) = self.persist_root(key.scope) else {
            return Vec::new();
        };
        let mut entries = scripts::list(&root, key.scope);
        entries.retain(|entry| entry.meta.connection_id == key.id);
        entries
    }

    /// The request for a console. The list offers the saved consoles of the
    /// connection, and a row creates a new console.
    pub(super) fn request_console(&mut self, connection: ConnectionId) {
        if self.config_blocked() {
            return;
        }
        let entries = self.consoles_of(connection);
        if entries.is_empty() {
            self.new_console(connection);
            return;
        }
        let item = self.connection(connection);
        let name = item.map(|item| item.name.clone()).unwrap_or_default();
        let scope = item.and_then(Connection::scope).map_or("", scope_word);
        let items: Vec<String> = std::iter::once(text::NEW_CONSOLE_ROW.to_owned())
            .chain(entries.iter().map(|entry| self.script_row(entry, false)))
            .collect();
        self.picker = Some(Picker {
            kind: PickerKind::Consoles {
                connection,
                scripts: entries,
            },
            title: text::consoles_title(&name, scope),
            items,
            cursor: 0,
            loading: false,
        });
    }

    /// Lists the scripts whose connection is not in the files.
    pub(super) fn open_orphans(&mut self) {
        if self.config_blocked() {
            return;
        }
        let mut entries = Vec::new();
        for scope in [Scope::Global, Scope::Project] {
            let Some(root) = self.persist_root(scope) else {
                continue;
            };
            for entry in scripts::list(&root, scope) {
                if self
                    .connection_by_key(scope, &entry.meta.connection_id)
                    .is_none()
                {
                    entries.push(entry);
                }
            }
        }
        if entries.is_empty() {
            self.status = Some(text::NO_ORPHANS.to_owned());
            return;
        }
        let items = entries
            .iter()
            .map(|entry| self.script_row(entry, true))
            .collect();
        self.picker = Some(Picker {
            kind: PickerKind::Orphans { scripts: entries },
            title: text::TITLE_ORPHANS.to_owned(),
            items,
            cursor: 0,
            loading: false,
        });
    }

    /// Opens a saved console in a tab, or it selects the tab that already shows it.
    pub(super) fn open_script(&mut self, entry: &ScriptEntry) {
        let open = self.consoles.iter().position(|c| {
            c.file
                .as_ref()
                .is_some_and(|file| file.scope == entry.scope && file.meta.id == entry.meta.id)
        });
        let index = match open.map_or_else(|| self.load_script(entry), Ok) {
            Ok(index) => index,
            Err(message) => {
                self.status = Some(message);
                return;
            }
        };
        self.select_console(index);
        self.leave_navigator();
    }

    /// Opens a saved console in a new tab, without a change of the tab on screen. It
    /// returns the index of the console, or the reason that the console did not open.
    pub(super) fn load_script(&mut self, entry: &ScriptEntry) -> Result<usize, String> {
        let failed =
            |error: std::io::Error| text::script_failed(&entry.meta.name, &error.to_string());
        let Some(root) = self.persist_root(entry.scope) else {
            return Err(text::NO_STORE.to_owned());
        };
        let id = &entry.meta.id;
        let lock = scripts::lock(&root, id)
            .map_err(failed)?
            .ok_or_else(|| text::script_in_use(&entry.meta.name))?;
        let (text, loaded) = match scripts::read_text(&root, id) {
            Ok((raw, hash)) => {
                let (text, ending) = scripts::decode(&raw);
                (text, Some((hash, ending)))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (String::new(), None),
            Err(error) => return Err(failed(error)),
        };
        let connection = self
            .connection_by_key(entry.scope, &entry.meta.connection_id)
            .map(|item| (item.id, item.dialect));
        let dialect = connection.map_or(self.fallback_dialect, |(_, dialect)| dialect);
        let mut console = self.unbound_console(dialect);
        console.editor = SqlEditor::new(&text, self.keyboard, dialect, self.clipboard.clone());
        console.name.clone_from(&entry.meta.name);
        console.schema.clone_from(&entry.meta.schema);
        let revision = console.editor.revision();
        console.file = Some(ScriptFile::open(
            entry.scope,
            entry.meta.clone(),
            Some(lock),
            revision,
            loaded,
            &root,
        ));
        restore_editor_state(&mut console.editor, &root, id, &text);
        self.consoles.push(console);
        let index = self.consoles.len() - 1;
        if let Some((connection, _)) = connection {
            self.bind_console(index, connection);
        }
        Ok(index)
    }

    /// The row of the list under the cursor, when it stands for a saved script.
    pub(super) fn picker_script(&self) -> Option<ScriptEntry> {
        let picker = self.picker.as_ref()?;
        match &picker.kind {
            PickerKind::Consoles { scripts, .. } => scripts.get(picker.cursor.checked_sub(1)?),
            PickerKind::Orphans { scripts } => scripts.get(picker.cursor),
            _ => None,
        }
        .cloned()
    }

    /// The key that removes the script under the cursor of the list.
    pub(super) fn picker_delete(&mut self) {
        let Some(entry) = self.picker_script() else {
            return;
        };
        if self.is_script_open(entry.scope, &entry.meta.id) {
            self.status = Some(text::SCRIPT_OPEN.to_owned());
            return;
        }
        let path = self
            .persist_root(entry.scope)
            .map(|root| root.consoles_dir().display().to_string());
        self.picker = None;
        self.show_dialog(
            text::TITLE_REMOVE_CONSOLE,
            text::remove_script(&entry.meta.name, &path.unwrap_or_default()),
            vec![
                (text::OPTION_CANCEL, DialogAction::Dismiss),
                (
                    text::OPTION_REMOVE,
                    DialogAction::DeleteScript(entry.scope, entry.meta.id),
                ),
            ],
        );
    }

    /// Removes the two files of a script. The files of the connection stay.
    pub(super) fn delete_script(&mut self, scope: Scope, id: &str) {
        let Some(root) = self.persist_root(scope) else {
            return;
        };
        // The lock proves that no other instance has the script open.
        match scripts::lock(&root, id) {
            Ok(Some(_lock)) => {}
            Ok(None) => {
                self.status = Some(text::SCRIPT_BUSY.to_owned());
                return;
            }
            Err(error) => {
                self.status = Some(text::script_failed(id, &error.to_string()));
                return;
            }
        }
        if let Err(message) = scripts::remove(&root, id) {
            self.status = Some(text::script_failed(id, &message));
            return;
        }
        let _ = std::fs::remove_file(root.locks_dir().join(format!("{id}.lock")));
        let _ = workspace::update(&root, |workspace| {
            workspace.consoles.remove(id);
        });
        self.status = Some(text::SCRIPT_REMOVED.to_owned());
    }

    // ---- rename ----

    /// Opens the request for a new name of the active console.
    pub(super) fn start_rename(&mut self) {
        let console = self.console();
        self.rename = Some(NamePrompt::new(
            NameTarget::Console(console.id),
            &console.name,
        ));
    }

    /// The key of the list. A saved console that a tab shows renames that tab.
    pub(super) fn picker_rename(&mut self) {
        let Some(entry) = self.picker_script() else {
            return;
        };
        let open = self.consoles.iter().find(|console| {
            console
                .file
                .as_ref()
                .is_some_and(|file| file.scope == entry.scope && file.meta.id == entry.meta.id)
        });
        let target = open.map_or_else(
            || NameTarget::Script(entry.clone()),
            |console| NameTarget::Console(console.id),
        );
        self.picker = None;
        self.rename = Some(NamePrompt::new(target, &entry.meta.name));
    }

    /// Returns the names of the other consoles of a connection, open and saved.
    fn names_in_use(
        &self,
        connection: Option<ConnectionId>,
        skip: Option<ConsoleId>,
        id: Option<&str>,
    ) -> Vec<String> {
        let Some(connection) = connection else {
            return Vec::new();
        };
        let open = self
            .consoles
            .iter()
            .filter(|console| console.connection == Some(connection) && Some(console.id) != skip)
            .map(|console| console.name.clone());
        let saved = self
            .consoles_of(connection)
            .into_iter()
            .filter(|entry| Some(entry.meta.id.as_str()) != id)
            .map(|entry| entry.meta.name);
        open.chain(saved).collect()
    }

    fn name_note(&mut self, note: &str) {
        if let Some(prompt) = &mut self.rename {
            prompt.note = Some(note.to_owned());
        }
    }

    /// Applies a new name. The identity, the files, and the connection stay.
    pub(super) fn accept_name(&mut self, name: String) {
        if name.is_empty() {
            self.name_note(text::NAME_EMPTY);
            return;
        }
        let Some(prompt) = &self.rename else { return };
        let (connection, skip, id) = match &prompt.target {
            NameTarget::Console(console) => {
                let index = self.console_index(*console);
                let console_ref = index.map(|index| &self.consoles[index]);
                let id = console_ref
                    .and_then(|item| item.file.as_ref())
                    .map(|file| file.meta.id.clone());
                (
                    console_ref.and_then(|item| item.connection),
                    Some(*console),
                    id,
                )
            }
            NameTarget::Script(entry) => {
                let connection = self
                    .connection_by_key(entry.scope, &entry.meta.connection_id)
                    .map(|item| item.id);
                (connection, None, Some(entry.meta.id.clone()))
            }
        };
        let taken = self.names_in_use(connection, skip, id.as_deref());
        if taken
            .iter()
            .any(|item| item.to_lowercase() == name.to_lowercase())
        {
            self.name_note(text::NAME_TAKEN);
            return;
        }
        let Some(prompt) = self.rename.take() else {
            return;
        };
        let result = match prompt.target {
            NameTarget::Console(console) => self.rename_console(console, &name),
            NameTarget::Script(entry) => self.rename_script(&entry, &name),
        };
        self.status = Some(match result {
            Ok(()) => text::CONSOLE_RENAMED.to_owned(),
            Err(message) => text::script_failed(&name, &message),
        });
    }

    fn rename_console(&mut self, id: ConsoleId, name: &str) -> Result<(), String> {
        let Some(index) = self.console_index(id) else {
            return Ok(());
        };
        let root = self.consoles[index]
            .file
            .as_ref()
            .and_then(|file| self.persist_root(file.scope));
        let console = &mut self.consoles[index];
        if let (Some(file), Some(root)) = (&mut console.file, &root) {
            let old = std::mem::replace(&mut file.meta.name, name.to_owned());
            if let Err(message) = file.write_meta(root) {
                file.meta.name = old;
                return Err(message);
            }
        }
        console.name = name.to_owned();
        Ok(())
    }

    fn rename_script(&mut self, entry: &ScriptEntry, name: &str) -> Result<(), String> {
        let root = self.persist_root(entry.scope).ok_or(text::NO_STORE)?;
        // The lock proves that no other instance has the script open.
        let _lock = match scripts::lock(&root, &entry.meta.id) {
            Ok(Some(lock)) => lock,
            Ok(None) => return Err(text::SCRIPT_BUSY.to_owned()),
            Err(error) => return Err(error.to_string()),
        };
        let mut meta = entry.meta.clone();
        meta.name = name.to_owned();
        scripts::write_meta(&root, &meta, Expect::Any)
    }

    // ---- change of connection ----

    /// `Ctrl+W a`. A console without a connection uses the same list.
    pub(super) fn start_connection_change(&mut self) {
        if self.config_blocked() {
            return;
        }
        let console = self.console();
        if console.is_busy() || console.in_transaction() {
            self.status = Some(text::CONNECTION_CHANGE_BUSY.to_owned());
            return;
        }
        let current = console.connection;
        let list: Vec<(ConnectionId, String)> = self
            .connections
            .iter()
            .filter(|item| Some(item.id) != current)
            .map(|item| {
                let row = item.scope().map_or_else(
                    || item.name.clone(),
                    |scope| format!("{} · {}", item.name, scope_word(scope)),
                );
                (item.id, row)
            })
            .collect();
        if list.is_empty() {
            self.status = Some(text::NO_OTHER_CONNECTION.to_owned());
            return;
        }
        let console = console.id;
        self.picker = Some(Picker {
            kind: PickerKind::Associate {
                console,
                connections: list.iter().map(|(id, _)| *id).collect(),
            },
            title: text::TITLE_CHANGE_CONNECTION.to_owned(),
            items: list.into_iter().map(|(_, row)| row).collect(),
            cursor: 0,
            loading: false,
        });
    }

    /// Links a console without a file to a connection. Only memory changes.
    fn change_memory_console(&mut self, index: usize, connection: ConnectionId) {
        let Some(target) = self.connection(connection) else {
            return;
        };
        let (dialect, name) = (target.dialect, target.name.clone());
        self.bind_keeping_name(index, connection, dialect);
        self.status = Some(text::associated(&name));
    }

    /// Moves a console to a connection. The name stays, and the results of the last
    /// connection end. It sends no SQL.
    fn bind_keeping_name(
        &mut self,
        index: usize,
        connection: ConnectionId,
        dialect: crate::sql::Dialect,
    ) {
        let budget = self.budget.clone();
        let name = self.consoles[index].name.clone();
        if self.consoles[index].connection.is_some() {
            self.unbind_console(index);
        }
        let console = &mut self.consoles[index];
        console.clear_results(&budget);
        console.schema = None;
        console.editor.set_dialect(dialect);
        self.bind_console(index, connection);
        self.consoles[index].name = name;
    }

    // ---- association ----

    /// The run of a console that has a script and no connection. The user picks a
    /// connection first. The run starts only after the association.
    pub(super) fn start_association(&mut self) {
        if self.config_blocked() {
            return;
        }
        let list: Vec<(ConnectionId, String)> = self
            .connections
            .iter()
            .filter_map(|item| {
                let scope = item.scope()?;
                Some((item.id, format!("{} · {}", item.name, scope_word(scope))))
            })
            .collect();
        if list.is_empty() {
            self.status = Some(text::NO_CONNECTION.to_owned());
            return;
        }
        let console = self.console().id;
        self.picker = Some(Picker {
            kind: PickerKind::Associate {
                console,
                connections: list.iter().map(|(id, _)| *id).collect(),
            },
            title: text::TITLE_ASSOCIATE.to_owned(),
            items: list.into_iter().map(|(_, row)| row).collect(),
            cursor: 0,
            loading: false,
        });
    }

    /// Asks for the confirmation. It tells where the files will be.
    pub(super) fn confirm_association(&mut self, console: ConsoleId, connection: ConnectionId) {
        let Some(index) = self.console_index(console) else {
            return;
        };
        let Some(target) = self.connection(connection) else {
            return;
        };
        let item = &self.consoles[index];
        let root = target
            .key
            .as_ref()
            .and_then(|key| self.persist_root(key.scope));
        let place = root.map_or_else(
            || text::NO_STORE.to_owned(),
            |root| root.consoles_dir().display().to_string(),
        );
        let moves = match (&item.file, &target.key) {
            (Some(file), Some(key)) => file.scope != key.scope,
            _ => false,
        };
        if let Some(current) = item.connection.and_then(|id| self.connection(id)) {
            let body =
                text::change_connection(&item.name, &current.name, &target.name, &place, moves);
            self.show_dialog(
                text::TITLE_CHANGE_CONNECTION,
                body,
                vec![
                    (text::OPTION_CANCEL, DialogAction::Dismiss),
                    (
                        text::OPTION_CHANGE,
                        DialogAction::Associate(console, connection),
                    ),
                ],
            );
            return;
        }
        let body = match (&item.file, &target.key) {
            (Some(_), Some(key)) if moves => {
                text::associate_move(&item.name, &target.name, scope_word(key.scope), &place)
            }
            _ => text::associate_same(&item.name, &target.name, &place),
        };
        self.show_dialog(
            text::TITLE_ASSOCIATE_CONFIRM,
            body,
            vec![
                (text::OPTION_CANCEL, DialogAction::Dismiss),
                (
                    text::OPTION_ASSOCIATE,
                    DialogAction::Associate(console, connection),
                ),
            ],
        );
    }

    /// Links the script to the connection. It moves the files when the scope changes.
    /// It sends no SQL.
    pub(super) fn associate(&mut self, console: ConsoleId, connection: ConnectionId) {
        let Some(index) = self.console_index(console) else {
            return;
        };
        // The console can start to work while the question stays open.
        if self.consoles[index].is_busy() || self.consoles[index].in_transaction() {
            self.status = Some(text::CONNECTION_CHANGE_BUSY.to_owned());
            return;
        }
        if self.consoles[index].file.is_none() {
            self.change_memory_console(index, connection);
            return;
        }
        let Some(target) = self.connection(connection) else {
            return;
        };
        let Some(key) = target.key.clone() else {
            return;
        };
        let dialect = target.dialect;
        let name = target.name.clone();
        let now = Instant::now();
        // The move copies the saved text, so the pending text goes to the file first.
        if self.consoles[index].file.is_some() && self.save_console_for_move(index, now).is_err() {
            self.status = Some(text::ASSOCIATE_SAVE_FIRST.to_owned());
            return;
        }
        let Some(to_root) = self.persist_root(key.scope) else {
            return;
        };
        let Some(file) = &mut self.consoles[index].file else {
            return;
        };
        let Some(from_root) = self
            .persist
            .as_ref()
            .and_then(|p| p.store.root(file.scope))
            .cloned()
        else {
            return;
        };
        let mut meta = file.meta.clone();
        meta.connection_id = key.id;
        meta.connection_scope = key.scope;
        meta.schema = None;
        if file.scope == key.scope {
            file.meta = meta;
            if let Err(message) = file.write_meta(&to_root) {
                self.status = Some(text::script_failed(&name, &message));
                return;
            }
        } else {
            let lock = match scripts::lock(&to_root, &meta.id) {
                Ok(Some(lock)) => lock,
                _ => {
                    self.status = Some(text::SCRIPT_BUSY.to_owned());
                    return;
                }
            };
            if let Err(message) = scripts::move_to(&from_root, &to_root, &meta) {
                self.status = Some(text::script_failed(&name, &message));
                return;
            }
            file.relocate(key.scope, meta, Some(lock), &to_root);
        }
        self.bind_keeping_name(index, connection, dialect);
        self.status = Some(text::associated(&name));
    }

    fn save_console_for_move(&mut self, index: usize, now: Instant) -> Result<(), Trouble> {
        let console = &self.consoles[index];
        let dirty = console
            .file
            .as_ref()
            .is_some_and(|file| file.is_dirty(console.editor.revision()));
        if let Some(trouble) = console.file.as_ref().and_then(|file| file.trouble.clone()) {
            return Err(trouble);
        }
        if dirty {
            self.save_console(index, now)
        } else {
            Ok(())
        }
    }
}

/// Puts the cursor and the scroll of the last session back, inside the text.
fn restore_editor_state(editor: &mut SqlEditor, root: &Root, id: &str, text: &str) {
    let Some(state) = workspace::load(root).consoles.get(id).copied() else {
        return;
    };
    let lines: Vec<&str> = text.split('\n').collect();
    let row = state.cursor.row.min(lines.len() - 1);
    let col = state.cursor.col.min(lines[row].chars().count());
    editor.set_cursor(Index2::new(row, col));
    editor.set_scroll(state.scroll.col, state.scroll.row.min(lines.len() - 1));
}
