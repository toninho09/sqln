//! Console saves: intents, the save worker, metadata, and closing with unsaved text.

use super::*;

impl App {
    /// Returns the script of the console, and it creates the script at the first edit.
    pub(in crate::app) fn ensure_file(&mut self, index: usize) {
        if self.persist.is_none()
            || self.tabs.consoles[index].file.is_some()
            || self.tabs.consoles[index].external.is_some()
            || self.agent.owns(self.tabs.consoles[index].id)
        {
            return;
        }
        let console = &self.tabs.consoles[index];
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
        self.tabs.consoles[index].file = Some(file);
    }

    /// Writes a console during emergency shutdown, after the worker completes.
    pub(in crate::app) fn save_console(
        &mut self,
        index: usize,
        now: Instant,
    ) -> Result<(), Trouble> {
        let console = &mut self.tabs.consoles[index];
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
    pub(in crate::app) fn save_now(&mut self) {
        if self.agent.owns(self.console().id) {
            self.status = Some(text::AGENT_CONSOLE_TEMPORARY.to_owned());
            return;
        }
        let index = self.tabs.active;
        if self.tabs.consoles[index].external.is_some() {
            self.save_external_console(index);
            return;
        }
        if self.persist.is_none() {
            self.status = Some(text::NOTHING_TO_SAVE.to_owned());
            return;
        }
        self.ensure_file(index);
        let console = &self.tabs.consoles[index];
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
        if let Some(persist) = self.persist.as_mut() {
            persist
                .saves
                .entry(self.tabs.consoles[index].id)
                .or_default()
                .manual = true;
        }
        self.start_console_save(index, false);
    }

    /// Returns true while a script write or its continuation waits.
    pub fn has_pending_saves(&self) -> bool {
        self.persist
            .as_ref()
            .is_some_and(|persist| !persist.saves.is_empty())
            || self
                .tabs
                .consoles
                .iter()
                .any(|console| console.file.as_ref().is_some_and(|file| file.is_saving()))
    }

    pub(in crate::app) fn association_pending(&self, id: ConsoleId) -> bool {
        self.persist
            .as_ref()
            .and_then(|persist| persist.saves.get(&id))
            .is_some_and(|intent| intent.associate.is_some())
    }

    pub(super) fn start_console_save(&mut self, index: usize, autosave: bool) {
        let target = self
            .persist
            .as_ref()
            .and_then(|persist| persist.saves.get(&self.tabs.consoles[index].id))
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
        let console = &mut self.tabs.consoles[index];
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
                if let Some(intent) = persist.saves.get_mut(&console.id) {
                    intent.metadata = false;
                }
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
                let associate = intent.associate;
                if let Some(intent) = persist.saves.get_mut(&console.id) {
                    intent.association_running = associate;
                }
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

    pub(super) fn poll_saves(&mut self, now: Instant) -> bool {
        let mut completed = Vec::new();
        for console in &mut self.tabs.consoles {
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
            let console = &self.tabs.consoles[index];
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
                    let console = &mut self.tabs.consoles[index];
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
                let name = self.tabs.consoles[index].name.clone();
                if let Some(file) = self.tabs.consoles[index].file.as_mut() {
                    file.meta.name = name;
                }
                if let Some(target) = self.connection(connection) {
                    let dialect = target.dialect;
                    let name = target.name.clone();
                    self.bind_keeping_name(index, connection, dialect);
                    self.status = Some(text::associated(&name));
                }
            }
            let id = self.tabs.consoles[index].id;
            if intent.metadata
                || intent.overwrite
                || intent.associate.is_some()
                || (dirty
                    && !self.tabs.consoles[index].discard_text
                    && (intent.manual || intent.close))
            {
                if let Some(persist) = self.persist.as_mut() {
                    persist.saves.insert(id, intent);
                }
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

    pub(in crate::app) fn queue_save_for_move(&mut self, index: usize, connection: ConnectionId) {
        let console = &self.tabs.consoles[index];
        if console
            .file
            .as_ref()
            .is_some_and(|file| file.trouble.is_some())
        {
            self.status = Some(text::ASSOCIATE_SAVE_FIRST.to_owned());
            return;
        }
        if let Some(persist) = self.persist.as_mut() {
            persist.saves.entry(console.id).or_default().associate = Some(connection);
        }
        self.start_console_save(index, false);
    }

    pub(in crate::app) fn queue_metadata(
        &mut self,
        index: usize,
        rename: Option<(String, String)>,
    ) {
        let Some(persist) = self.persist.as_mut() else {
            return;
        };
        let intent = persist
            .saves
            .entry(self.tabs.consoles[index].id)
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

    pub(in crate::app) fn rekey_saves(&mut self, previous: ConsoleId, current: ConsoleId) {
        if let Some(persist) = &mut self.persist
            && let Some(intent) = persist.saves.remove(&previous)
        {
            persist.saves.insert(current, intent);
        }
    }

    pub(in crate::app) fn cancel_save_close(&mut self, id: ConsoleId) {
        if let Some(intent) = self
            .persist
            .as_mut()
            .and_then(|persist| persist.saves.get_mut(&id))
        {
            intent.close = false;
        }
    }

    /// Writes the pending text before a close. It returns false after it asked a
    /// question, because the text has no safe place to go.
    pub(in crate::app) fn flush_console(&mut self, index: usize) -> bool {
        if let Some(external) = &self.tabs.consoles[index].external {
            let revision = self.tabs.consoles[index].editor.revision();
            if self.tabs.consoles[index].discard_text || !external.is_dirty(revision) {
                return true;
            }
            self.ask_external_close(index);
            return false;
        }
        if self.persist.is_none() {
            return true;
        }
        if self.tabs.consoles[index].discard_text {
            let console = &self.tabs.consoles[index];
            let saving = console.file.as_ref().is_some_and(|file| file.is_saving());
            let Some(persist) = self.persist.as_mut() else {
                return true;
            };
            persist.saves.remove(&console.id);
            if saving {
                persist.saves.entry(console.id).or_default().close = true;
                return false;
            }
            return true;
        }
        self.ensure_file(index);
        let console = &self.tabs.consoles[index];
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

    pub(super) fn queue_save_close(&mut self, index: usize) {
        if let Some(persist) = self.persist.as_mut() {
            persist
                .saves
                .entry(self.tabs.consoles[index].id)
                .or_default()
                .close = true;
        }
        self.start_console_save(index, false);
    }

    pub(super) fn show_unsaved(&mut self, index: usize, problem: Trouble) {
        let console = &self.tabs.consoles[index];
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
        // A linked console writes its file with the check of a change, because there is
        // no question to ask. A file that another program changed keeps its text.
        for console in &mut self.tabs.consoles {
            if let Some(external) = &mut console.external {
                let revision = console.editor.revision();
                if external.is_dirty(revision) {
                    let text = console.editor.text();
                    let _ = external.save(&text, revision);
                }
            }
        }
        if self.persist.is_none() {
            return;
        }
        let now = Instant::now();
        for console in &mut self.tabs.consoles {
            if let Some(file) = &mut console.file {
                let _ = file.wait_save(now);
            }
        }
        for index in 0..self.tabs.consoles.len() {
            self.ensure_file(index);
            let intent = self
                .persist
                .as_mut()
                .and_then(|persist| persist.saves.remove(&self.tabs.consoles[index].id));
            if let Some(intent) = intent
                && let Some(file) = &mut self.tabs.consoles[index].file
                && let Some(root) = self
                    .persist
                    .as_ref()
                    .and_then(|persist| persist.store.root(file.scope))
            {
                if intent.metadata {
                    let _ = file.write_meta(root);
                }
                if intent.overwrite {
                    let console = &mut self.tabs.consoles[index];
                    let text = console.editor.text();
                    let revision = console.editor.revision();
                    if let Some(file) = console.file.as_mut() {
                        let _ = file.overwrite(root, &text, revision);
                    }
                }
            }
            let console = &self.tabs.consoles[index];
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
    pub(in crate::app) fn flush_all(&mut self) -> bool {
        for index in 0..self.tabs.consoles.len() {
            if !self.flush_console(index) {
                self.select_console(index);
                return false;
            }
        }
        true
    }
}
