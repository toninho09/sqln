//! The open of an external `.sql` file as a console. The user chooses a copy, which
//! becomes a normal console, or a link, which writes the file on each explicit save.

use std::path::PathBuf;

use super::{App, DialogAction, Picker, PickerKind};
use crate::exec::{ConnectionId, ConsoleId};
use crate::script_file::{ExternalFile, Trouble};
use crate::ui::text;

/// A file that waits for the answer of the copy-or-linked question.
pub(super) struct PendingSqlFile {
    connection: crate::exec::ConnectionId,
    file: ExternalFile,
    text: String,
}

impl App {
    /// The action that opens a `.sql` file. It needs a connection, because a console
    /// runs its text against one.
    pub(super) fn start_open_sql_file(&mut self) {
        self.open_sql_file(None);
    }

    /// Opens a file that the command line named. It follows the same steps as the
    /// action, without the file browser.
    pub(super) fn open_sql_file_path(&mut self, path: PathBuf) {
        self.open_sql_file(Some(path));
    }

    /// Opens a file, or asks for its connection first. `path` is `None` when the file
    /// browser chooses the file after the connection.
    fn open_sql_file(&mut self, path: Option<PathBuf>) {
        if self.config_blocked() {
            return;
        }
        if self.connections.is_empty() {
            self.status = Some(text::NO_CONNECTION.to_owned());
            return;
        }
        // The connection of the console, then the only connection, then a question.
        let active = self
            .console()
            .connection
            .filter(|id| self.connections.iter().any(|item| item.id == *id));
        let single = (self.connections.len() == 1).then(|| self.connections[0].id);
        match active.or(single) {
            Some(connection) => self.continue_sql_file(connection, path),
            None => self.pick_sql_file_connection(path),
        }
    }

    fn continue_sql_file(&mut self, connection: ConnectionId, path: Option<PathBuf>) {
        match path {
            Some(path) => self.read_sql_file(connection, path),
            None => self.begin_sql_file(connection),
        }
    }

    /// The action knows the connection, so the file browser comes next.
    fn begin_sql_file(&mut self, connection: ConnectionId) {
        self.sql_file_open = Some(connection);
        let home = std::env::home_dir();
        self.browser = Some(crate::browser::FileBrowser::new("", false, home));
    }

    fn pick_sql_file_connection(&mut self, path: Option<PathBuf>) {
        let rows: Vec<(ConnectionId, String)> = self
            .connections
            .iter()
            .map(|item| {
                let row = item.scope().map_or_else(
                    || item.name.clone(),
                    |scope| {
                        let scope = match scope {
                            crate::store::Scope::Global => text::VALUE_SCOPE_GLOBAL,
                            crate::store::Scope::Project => text::VALUE_SCOPE_PROJECT,
                        };
                        format!("{} · {scope}", item.name)
                    },
                );
                (item.id, row)
            })
            .collect();
        self.picker = Some(Picker {
            kind: PickerKind::SqlFile {
                path,
                connections: rows.iter().map(|(id, _)| *id).collect(),
            },
            title: text::TITLE_SQL_FILE.to_owned(),
            items: rows.into_iter().map(|(_, row)| row).collect(),
            cursor: 0,
            loading: false,
        });
    }

    /// The answer of the picker: the connection of the file.
    pub(super) fn picked_sql_file_connection(
        &mut self,
        connection: ConnectionId,
        path: Option<PathBuf>,
    ) {
        self.continue_sql_file(connection, path);
    }

    /// The file browser chose a file.
    pub(super) fn sql_file_chosen(&mut self, path: PathBuf) {
        let Some(connection) = self.sql_file_open.take() else {
            return;
        };
        self.read_sql_file(connection, path);
    }

    fn read_sql_file(&mut self, connection: ConnectionId, path: PathBuf) {
        match ExternalFile::read(&path) {
            Ok(read) => {
                self.sql_file = Some(PendingSqlFile {
                    connection,
                    file: read.file,
                    text: read.text,
                });
                let path = path.display().to_string();
                self.show_dialog(
                    text::TITLE_SQL_FILE,
                    text::sql_file_question(&path),
                    vec![
                        (text::OPTION_CANCEL, DialogAction::SqlFileCancel),
                        (text::OPTION_SQL_FILE_COPY, DialogAction::SqlFileCopy),
                        (text::OPTION_SQL_FILE_LINKED, DialogAction::SqlFileLinked),
                    ],
                );
            }
            Err(error) => {
                let path = path.display().to_string();
                self.status = Some(text::sql_file_failed(&path, &error.to_string()));
            }
        }
    }

    /// The answer of the copy-or-linked question: it opens the console.
    pub(super) fn accept_sql_file(&mut self, linked: bool) {
        let Some(pending) = self.sql_file.take() else {
            return;
        };
        let Some(target) = self.connection(pending.connection) else {
            self.status = Some(text::NO_CONNECTION.to_owned());
            return;
        };
        let dialect = target.dialect;
        let mut console = self.unbound_console(dialect);
        console.editor = self.new_editor(&pending.text, dialect);
        let mut file = pending.file;
        let stem = file
            .path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .map(str::to_owned);
        if linked {
            file.adopt(console.editor.revision());
            console.external = Some(file);
        }
        self.consoles.push(console);
        let index = self.consoles.len() - 1;
        self.select_console(index);
        self.bind_console(index, pending.connection);
        // The name of the file stays over the name that the binding counts.
        if let Some(stem) = stem {
            self.consoles[index].name = stem;
        }
        self.refresh_tree_consoles();
        self.leave_navigator();
    }

    // ---- the saves of a linked console ----

    /// The write of `Ctrl+S` for a console that links to a file.
    pub(super) fn save_external_console(&mut self, index: usize) {
        let console = &mut self.consoles[index];
        let Some(external) = &mut console.external else {
            return;
        };
        let id = console.id;
        let name = console.name.clone();
        if external.trouble.as_ref().is_some_and(Trouble::needs_choice) {
            let problem = external.trouble.clone().unwrap();
            self.ask_external_conflict(id, &problem);
            return;
        }
        let text = console.editor.text();
        let revision = console.editor.revision();
        match external.save(&text, revision) {
            Ok(()) => self.status = Some(text::SAVED.to_owned()),
            Err(Trouble::Failed(message)) => {
                self.status = Some(text::save_failed(&name, &message));
                self.status_shown = None;
            }
            Err(problem) => self.ask_external_conflict(id, &problem),
        }
    }

    /// The file changed or disappeared after the console read it.
    fn ask_external_conflict(&mut self, id: ConsoleId, problem: &Trouble) {
        let Some(console) = self.console_by_id(id) else {
            return;
        };
        let path = console
            .external
            .as_ref()
            .map(|file| file.path.display().to_string())
            .unwrap_or_default();
        let cancel = if console.closing {
            DialogAction::CancelClose(id)
        } else {
            DialogAction::Dismiss
        };
        match problem {
            Trouble::Changed => self.show_dialog(
                text::TITLE_SQL_FILE_CHANGED,
                text::sql_file_changed(&path),
                vec![
                    (text::OPTION_CANCEL, cancel),
                    (
                        text::OPTION_SQL_FILE_RELOAD,
                        DialogAction::SqlFileReload(id),
                    ),
                    (
                        text::OPTION_SQL_FILE_OVERWRITE,
                        DialogAction::SqlFileOverwrite(id),
                    ),
                ],
            ),
            Trouble::Removed => self.show_dialog(
                text::TITLE_SQL_FILE_REMOVED,
                text::sql_file_removed(&path),
                vec![
                    (text::OPTION_CANCEL, cancel),
                    (
                        text::OPTION_SQL_FILE_RECREATE,
                        DialogAction::SqlFileOverwrite(id),
                    ),
                ],
            ),
            Trouble::Failed(_) => {}
        }
    }

    /// Reads the file again, and the editor takes its text. The text that no file
    /// holds is lost, as in the reload of a script of the store.
    pub(super) fn reload_external(&mut self, id: ConsoleId) {
        let Some(index) = self.console_index(id) else {
            return;
        };
        let text = {
            let Some(external) = &mut self.consoles[index].external else {
                return;
            };
            match external.reload() {
                Ok(text) => text,
                Err(error) => {
                    let name = self.consoles[index].name.clone();
                    self.status = Some(text::save_failed(&name, &error.to_string()));
                    return;
                }
            }
        };
        self.replace_editor(index, &text);
        let console = &mut self.consoles[index];
        let revision = console.editor.revision();
        if let Some(external) = &mut console.external {
            external.adopt(revision);
        }
        let closing = console.closing;
        self.status = Some(text::SQL_FILE_RELOADED.to_owned());
        if closing {
            self.close_step(id);
        }
    }

    /// Writes the file over whatever it holds now. It also recreates a removed file.
    pub(super) fn overwrite_external(&mut self, id: ConsoleId) {
        let Some(index) = self.console_index(id) else {
            return;
        };
        let console = &mut self.consoles[index];
        let Some(external) = &mut console.external else {
            return;
        };
        let text = console.editor.text();
        let revision = console.editor.revision();
        let name = console.name.clone();
        let id = console.id;
        match external.overwrite(&text, revision) {
            Ok(()) => {
                self.status = Some(text::SAVED.to_owned());
                if console.closing {
                    self.close_step(id);
                }
            }
            Err(Trouble::Failed(message)) => {
                console.closing = false;
                self.status = Some(text::save_failed(&name, &message));
                self.status_shown = None;
            }
            // `Expect::Any` accepts any content, so the other troubles cannot happen.
            Err(problem) => self.ask_external_conflict(id, &problem),
        }
    }

    /// The close of a linked console with text that the file does not hold.
    pub(super) fn ask_external_close(&mut self, index: usize) {
        let id = self.consoles[index].id;
        let path = self.consoles[index]
            .external
            .as_ref()
            .map(|file| file.path.display().to_string())
            .unwrap_or_default();
        self.consoles[index].closing = true;
        self.show_dialog(
            text::TITLE_UNSAVED,
            text::sql_file_unsaved(&path),
            vec![
                (text::OPTION_CANCEL, DialogAction::CancelClose(id)),
                (
                    text::OPTION_SQL_FILE_WRITE_CLOSE,
                    DialogAction::SqlFileWriteClose(id),
                ),
                (
                    text::OPTION_SQL_FILE_CLOSE_WITHOUT,
                    DialogAction::SqlFileDiscardClose(id),
                ),
            ],
        );
    }

    /// The choice that writes the file and closes the console.
    pub(super) fn write_close_external(&mut self, id: ConsoleId) {
        let Some(index) = self.console_index(id) else {
            return;
        };
        let console = &mut self.consoles[index];
        let Some(external) = &mut console.external else {
            return;
        };
        let text = console.editor.text();
        let revision = console.editor.revision();
        let name = console.name.clone();
        match external.save(&text, revision) {
            Ok(()) => self.close_step(id),
            Err(Trouble::Failed(message)) => {
                console.closing = false;
                self.status = Some(text::save_failed(&name, &message));
                self.status_shown = None;
            }
            Err(problem) => self.ask_external_conflict(id, &problem),
        }
    }

    /// The choice that closes the console without a write of the file.
    pub(super) fn discard_close_external(&mut self, id: ConsoleId) {
        let Some(index) = self.console_index(id) else {
            return;
        };
        let console = &mut self.consoles[index];
        let revision = console.editor.revision();
        if let Some(external) = &mut console.external {
            external.discard(revision);
        }
        self.close_step(id);
    }
}
