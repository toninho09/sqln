//! The flow of an export: the dialog, its questions, the rows, and the end of the file.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::{App, DialogAction};
use crate::console::{Console, Phase, RunClock, Signal};
use crate::exec::{
    ConsoleId, ExecEvent, ExecutionEnd, ExecutionId, ReadLimit, RunOptions, StatementStatus,
};
use crate::export::form::{AllRows, ExportForm, ExportRequest, ExportSource, RowChoice};
use crate::export::{ExportOptions, ExportWriter, resolve_path, time_stamp};
use crate::grid::printable;
use crate::input::Action;
use crate::result::ResultTab;
use crate::sql::{Dialect, is_plain_read};
use crate::table::{TableTab, from_clause};
use crate::ui::text;

/// What an export waits for.
enum Work {
    /// The database sends the rows of this execution to the file.
    Reading(ExecutionId),
    /// The interface writes the rows of the grid, one slice at each step of the main loop.
    Writing {
        result: usize,
        rows: Range<usize>,
        columns: Range<usize>,
    },
    /// A dialog asks what to do with an incomplete file.
    Asking,
}

pub(super) struct ExportRun {
    console: ConsoleId,
    destination: PathBuf,
    writer: ExportWriter,
    work: Work,
    /// The reason of a failed read.
    failure: Option<String>,
}

/// A read of all rows that waits for credentials.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ExportRead {
    pub sql: String,
    pub schema: Option<String>,
    pub destination: PathBuf,
    pub options: ExportOptions,
}

/// Returns the full path of the text of the dialog, or the text of the problem.
fn destination_of(path: &str) -> Result<PathBuf, String> {
    if path.trim().is_empty() {
        return Err(text::EXPORT_NO_FILE.to_owned());
    }
    let path = resolve_path(path, std::env::home_dir())
        .map_err(|error| text::export_failed(&error.to_string()))?;
    if path.is_dir() {
        return Err(text::EXPORT_IS_FOLDER.to_owned());
    }
    Ok(path)
}

fn shown_path(path: &Path) -> String {
    printable(&path.display().to_string())
}

/// Decides whether a console can read a result again. The session of an execution
/// ends with it, so a statement of a script can depend on the statements before it.
fn console_all_rows(console: &Console, result: &ResultTab, dialect: Dialect) -> AllRows {
    let plain = console
        .statement_of(result)
        .is_some_and(|sql| is_plain_read(sql, dialect));
    if !plain {
        AllRows::Refused(text::EXPORT_ALL_READ)
    } else if console.ran_alone() || console.in_transaction() {
        AllRows::Available
    } else {
        AllRows::Refused(text::EXPORT_ALL_SCRIPT)
    }
}

fn result_in<'a>(
    tables: &'a [TableTab],
    consoles: &'a [Console],
    id: ConsoleId,
    result: usize,
) -> Option<&'a ResultTab> {
    if let Some(tab) = tables.iter().find(|tab| tab.id == id) {
        return Some(&tab.result);
    }
    let console = consoles.iter().find(|console| console.id == id)?;
    console.results.get(result)
}

impl App {
    fn active_tab_id(&self) -> ConsoleId {
        self.table().map_or(self.console().id, |tab| tab.id)
    }

    pub(super) fn active_result(&self) -> Option<&ResultTab> {
        let result = match self.table() {
            Some(tab) => Some(&tab.result),
            None => {
                let console = self.console();
                console.results.get(console.active)
            }
        };
        result.filter(|result| !result.table.columns.is_empty())
    }

    /// Returns true when the tab on screen can open the export dialog.
    pub(super) fn can_export(&self) -> bool {
        let id = self.active_tab_id();
        let busy = self
            .table()
            .map_or(self.console().is_busy(), TableTab::is_busy);
        !busy && self.active_result().is_some() && !self.has_export(id)
    }

    /// Returns true when an export uses the results of this tab.
    pub(super) fn has_export(&self, id: ConsoleId) -> bool {
        self.exports.iter().any(|run| run.console == id)
    }

    pub(super) fn tab_run(&mut self, id: ConsoleId) -> Option<(&mut Phase, &mut RunClock)> {
        if let Some(index) = self.tables.iter().position(|tab| tab.id == id) {
            let tab = &mut self.tables[index];
            return Some((&mut tab.phase, &mut tab.run));
        }
        let console = self.console_by_id(id)?;
        Some((&mut console.phase, &mut console.run))
    }

    fn all_rows(&self, result: &ResultTab, dialect: Dialect) -> AllRows {
        let table = self.table();
        let complete = match table {
            Some(tab) => !tab.has_more && tab.limit == ReadLimit::None && result.is_complete(),
            None => result.is_complete(),
        };
        if complete {
            AllRows::Complete
        } else if dialect == Dialect::Redis {
            AllRows::Refused(text::EXPORT_ALL_SOURCE)
        } else if table.is_some() {
            AllRows::Available
        } else {
            console_all_rows(self.console(), result, dialect)
        }
    }

    fn export_source(&self) -> Option<ExportSource> {
        let result = self.active_result()?;
        let dialect = self.tab_connection()?.dialect;
        let (name, table) = match self.table() {
            Some(tab) => match &tab.structure {
                Some(structure) => (structure.name.clone(), String::new()),
                None => (
                    tab.relation.relation.clone(),
                    from_clause(&tab.relation, dialect),
                ),
            },
            None => (self.console().name.clone(), String::new()),
        };
        let selection = result
            .view
            .selection()
            .map(|range| (range.bottom - range.top + 1, range.right - range.left + 1));
        Some(ExportSource {
            name,
            table,
            dialect,
            loaded: result.table.rows.len(),
            selection,
            all_rows: self.all_rows(result, dialect),
        })
    }

    pub(super) fn open_export(&mut self) {
        if !self.can_export() || self.config_blocked() {
            return;
        }
        if let Some(source) = self.export_source() {
            let form = ExportForm::new(source, &self.export_choices, &time_stamp());
            self.export_form = Some(form);
        }
    }

    pub(super) fn on_export_request(&mut self, request: ExportRequest) {
        match request {
            ExportRequest::None => {}
            ExportRequest::Cancel => self.export_form = None,
            ExportRequest::Export => self.confirm_export(),
            ExportRequest::Browse => self.open_browser(),
        }
    }

    fn confirm_export(&mut self) {
        let Some(form) = &self.export_form else {
            return;
        };
        let checked = form
            .options()
            .and_then(|_| destination_of(form.path_text()))
            .and_then(|path| {
                let used = self.exports.iter().any(|run| run.destination == path);
                if used {
                    Err(text::EXPORT_IN_USE.to_owned())
                } else {
                    Ok(path)
                }
            });
        match checked {
            Err(note) => {
                if let Some(form) = &mut self.export_form {
                    form.note = Some(note);
                }
            }
            Ok(path) if path.exists() => self.show_dialog(
                text::TITLE_EXPORT_EXISTS,
                text::export_exists(&shown_path(&path)),
                vec![
                    (text::OPTION_CANCEL, DialogAction::Dismiss),
                    (text::OPTION_EXPORT_REPLACE, DialogAction::ExportReplace),
                ],
            ),
            Ok(_) => self.start_export(),
        }
    }

    pub(super) fn start_export(&mut self) {
        let Some(form) = self.export_form.take() else {
            return;
        };
        let (Ok(options), Ok(destination)) = (form.options(), destination_of(form.path_text()))
        else {
            return;
        };
        self.save_export_choices(form.choices(&destination));
        if form.rows == RowChoice::AllRows {
            self.request_read(destination, options);
        } else {
            self.write_loaded(form.rows, destination, options);
        }
    }

    /// Asks for the credentials of the connection, as a page or a run does.
    fn request_read(&mut self, destination: PathBuf, options: ExportOptions) {
        let id = self.active_tab_id();
        let source = match self.table() {
            Some(tab) => tab
                .read_sql(options.dialect)
                .map(|sql| (sql, None, Some(tab.connection))),
            None => {
                let console = self.console();
                let sql = console
                    .results
                    .get(console.active)
                    .and_then(|result| console.statement_of(result));
                sql.map(|sql| {
                    (
                        sql.to_owned(),
                        console.ran_schema().map(str::to_owned),
                        console.connection,
                    )
                })
            }
        };
        let Some((sql, schema, Some(connection))) = source else {
            return;
        };
        let read = ExportRead {
            sql,
            schema,
            destination,
            options,
        };
        self.authorize(connection, super::auth::Use::Export { console: id, read });
    }

    /// Creates the file and starts the read. The file waits for the credentials, so a
    /// refused password leaves nothing on the disk.
    pub(super) fn start_read(&mut self, id: ConsoleId, read: ExportRead) {
        let ExportRead {
            sql,
            schema,
            destination,
            options,
        } = read;
        let writer = match ExportWriter::create(&destination, options) {
            Ok(writer) => writer,
            Err(error) => {
                if let Some((phase, _)) = self.tab_run(id) {
                    *phase = Phase::Idle;
                }
                self.status = Some(text::export_failed(&error.to_string()));
                return;
            }
        };
        let (Some(manager), true) = (&self.manager, self.tab_exists(id)) else {
            writer.discard();
            return;
        };
        let run = RunOptions {
            manual: false,
            schema,
            row_cap: None,
            export: Some(writer.clone()),
        };
        let execution = manager.run(id, vec![sql], run);
        if let Some((phase, clock)) = self.tab_run(id) {
            *phase = Phase::Waiting;
            clock.start();
        }
        self.exports.push(ExportRun {
            console: id,
            destination,
            writer,
            work: Work::Reading(execution),
            failure: None,
        });
    }

    fn tab_exists(&self, id: ConsoleId) -> bool {
        self.tables.iter().any(|tab| tab.id == id)
            || self.consoles.iter().any(|console| console.id == id)
    }

    fn write_loaded(&mut self, choice: RowChoice, destination: PathBuf, options: ExportOptions) {
        let id = self.active_tab_id();
        let console = self.console();
        let index = console.active;
        let Some(result) = self.active_result() else {
            return;
        };
        let table = &result.table;
        let selection = result
            .view
            .selection()
            .filter(|_| choice == RowChoice::Selection);
        let (rows, columns) = selection
            .map_or((0..table.rows.len(), 0..table.columns.len()), |range| {
                (range.top..range.bottom + 1, range.left..range.right + 1)
            });
        let names = table.columns[columns.clone()].to_vec();
        let writer = match ExportWriter::create(&destination, options) {
            Ok(writer) => writer,
            Err(error) => {
                self.status = Some(text::export_failed(&error.to_string()));
                return;
            }
        };
        writer.start(&names);
        if let Some((phase, clock)) = self.tab_run(id) {
            *phase = Phase::Running;
            clock.start();
        }
        self.exports.push(ExportRun {
            console: id,
            destination,
            writer,
            work: Work::Writing {
                result: index,
                rows,
                columns,
            },
            failure: None,
        });
    }

    /// Returns true when an export has rows of the grid to write.
    pub(super) fn exports_have_work(&self) -> bool {
        self.exports
            .iter()
            .any(|run| matches!(run.work, Work::Writing { .. }))
    }

    /// Writes rows of the grid until the deadline.
    pub(super) fn continue_exports(&mut self, deadline: Instant) {
        for index in (0..self.exports.len()).rev() {
            if self.write_slice(index, deadline) {
                let run = self.exports.remove(index);
                self.end_export(run, ExecutionEnd::Completed, None);
            }
        }
    }

    /// Returns true at the end of the rows of one export.
    fn write_slice(&mut self, index: usize, deadline: Instant) -> bool {
        let run = &mut self.exports[index];
        let Work::Writing {
            result,
            rows,
            columns,
        } = &mut run.work
        else {
            return false;
        };
        let Some(source) = result_in(&self.tables, &self.consoles, run.console, *result) else {
            run.failure = Some(text::EXPORT_RESULT_CLOSED.to_owned());
            return true;
        };
        for row in rows.by_ref() {
            let cells = source.table.rows.get(row);
            let Some(cells) = cells.filter(|cells| cells.columns() >= columns.end) else {
                return true;
            };
            let cells = cells.cells().skip(columns.start).take(columns.len());
            if !run.writer.write_row(cells) {
                return true;
            }
            if Instant::now() >= deadline {
                break;
            }
        }
        rows.start >= rows.end
    }

    /// Takes an event of an export. It returns false for any other event, and the
    /// tab never sees these events, so its results stay on the grid.
    pub(super) fn on_export_event(&mut self, event: &ExecEvent) -> bool {
        let Some(execution) = event.execution() else {
            return false;
        };
        let reading = |run: &ExportRun| matches!(run.work, Work::Reading(id) if id == execution);
        let Some(index) = self.exports.iter().position(reading) else {
            return false;
        };
        match event {
            ExecEvent::Started { console, .. } => {
                if let Some((phase, _)) = self.tab_run(*console)
                    && *phase == Phase::Waiting
                {
                    *phase = Phase::Running;
                }
            }
            ExecEvent::StatementFinished {
                status: StatementStatus::Error(message),
                ..
            } => self.exports[index].failure = Some(message.clone()),
            ExecEvent::ConfirmClose { console, .. } => {
                self.on_signal(*console, Signal::ConfirmClose);
            }
            ExecEvent::ExecutionFinished { end, message, .. } => {
                let run = self.exports.remove(index);
                self.end_export(run, *end, message.clone());
            }
            _ => {}
        }
        true
    }

    /// Stops a write of grid rows. It returns false for a tab without one.
    pub(super) fn stop_export(&mut self, id: ConsoleId) -> bool {
        let writing =
            |run: &ExportRun| run.console == id && matches!(run.work, Work::Writing { .. });
        let Some(index) = self.exports.iter().position(writing) else {
            return false;
        };
        let run = self.exports.remove(index);
        self.end_export(run, ExecutionEnd::Stopped, None);
        true
    }

    /// Removes the file of a tab that closes.
    pub(super) fn discard_export(&mut self, id: ConsoleId) {
        self.exports.retain(|run| {
            if run.console == id {
                run.writer.discard();
            }
            run.console != id
        });
    }

    /// Removes the files of the exports that the exit of the application ends.
    pub(super) fn discard_exports(&mut self) {
        for run in self.exports.drain(..) {
            run.writer.discard();
        }
    }

    fn end_export(&mut self, mut run: ExportRun, end: ExecutionEnd, message: Option<String>) {
        let id = run.console;
        if let Some((phase, clock)) = self.tab_run(id) {
            *phase = Phase::Idle;
            clock.finish(end);
        }
        self.note_long_run(id);
        run.writer.seal();
        let failure = run
            .failure
            .take()
            .or(message)
            .or_else(|| run.writer.error());
        let closing = self
            .console_by_id(id)
            .is_some_and(|console| console.closing);
        if closing {
            run.writer.discard();
        } else if end == ExecutionEnd::Completed && failure.is_none() {
            self.promote_export(&run);
        } else if run.writer.rows() == 0 {
            run.writer.discard();
            self.status = Some(failure.map_or_else(
                || text::EXPORT_STOPPED.to_owned(),
                |reason| text::export_failed(&reason),
            ));
        } else {
            self.ask_about_part(run, failure.as_deref());
        }
        self.on_signal(
            id,
            Signal::ExecutionFinished {
                schema_changed: false,
            },
        );
    }

    fn promote_export(&mut self, run: &ExportRun) {
        self.status = Some(match run.writer.promote(&run.destination) {
            Ok(()) => text::export_done(run.writer.rows(), &shown_path(&run.destination)),
            Err(error) => {
                run.writer.discard();
                text::export_failed(&error.to_string())
            }
        });
    }

    /// The first option keeps the disk as it was before the export.
    fn ask_about_part(&mut self, mut run: ExportRun, failure: Option<&str>) {
        let id = run.console;
        let body = text::export_part(run.writer.rows(), failure, run.destination.exists());
        run.work = Work::Asking;
        self.exports.push(run);
        self.show_dialog(
            text::TITLE_EXPORT_PART,
            body,
            vec![
                (text::OPTION_EXPORT_DELETE, DialogAction::ExportDelete(id)),
                (text::OPTION_EXPORT_KEEP, DialogAction::ExportKeep(id)),
            ],
        );
    }

    fn take_asking(&mut self, id: ConsoleId) -> Option<ExportRun> {
        let asking = |run: &ExportRun| run.console == id && matches!(run.work, Work::Asking);
        let index = self.exports.iter().position(asking)?;
        Some(self.exports.remove(index))
    }

    pub(super) fn keep_export(&mut self, id: ConsoleId) {
        if let Some(run) = self.take_asking(id) {
            self.promote_export(&run);
        }
    }

    pub(super) fn delete_export(&mut self, id: ConsoleId) {
        if let Some(run) = self.take_asking(id) {
            run.writer.discard();
            self.status = Some(text::EXPORT_DELETED.to_owned());
        }
    }

    /// Returns the progress of the tab on screen for the hint row.
    pub(super) fn export_progress(&self) -> Option<String> {
        let id = self.active_tab_id();
        let run = self
            .exports
            .iter()
            .find(|run| run.console == id && !matches!(run.work, Work::Asking))?;
        Some(text::export_progress(
            run.writer.rows(),
            &shown_path(&run.destination),
            self.shown_key(Action::Stop).as_deref(),
        ))
    }
}
