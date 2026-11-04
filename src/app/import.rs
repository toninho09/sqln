//! The flow of an import: the columns of the table, the dialog, the rows, and the report.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use super::auth::Use;
use super::{App, DialogAction};
use crate::console::Phase;
use crate::exec::{
    ConnectionId, ConsoleId, ExecEvent, ExecutionEnd, ExecutionId, RunOptions, StatementStatus,
};
use crate::export::time_stamp;
use crate::grid::{Row as GridRow, Value};
use crate::import::form::{ImportAction, ImportForm, ImportTable, ProbeRequest, TableColumn};
use crate::import::{ImportJob, ImportProgress, Probe, probe};
use crate::input::Action;
use crate::navigator::{Row, RowKind};
use crate::sql::Dialect;
use crate::structure::{Scope, Section, structure_sql};
use crate::table::{TableTab, from_clause};
use crate::ui::text;

/// What an import waits for.
enum Work {
    /// The database sends the columns of the table.
    Columns {
        execution: ExecutionId,
        rows: Vec<GridRow>,
    },
    /// The rows of the file go to the table.
    Writing {
        execution: ExecutionId,
        progress: Arc<ImportProgress>,
        log: std::path::PathBuf,
    },
}

pub(super) struct ImportRun {
    console: ConsoleId,
    work: Work,
    /// The reason of a failed statement.
    failure: Option<String>,
}

impl ImportRun {
    fn execution(&self) -> ExecutionId {
        match &self.work {
            Work::Columns { execution, .. } | Work::Writing { execution, .. } => *execution,
        }
    }
}

/// What waits for the credentials of the connection.
pub(super) enum ImportStep {
    Columns(String),
    Write(Box<ImportJob>),
}

/// The channel of the answers of the file reads.
pub(super) struct ProbeChannel {
    sender: Sender<(u64, Probe)>,
    receiver: Receiver<(u64, Probe)>,
}

impl Default for ProbeChannel {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Self { sender, receiver }
    }
}

/// Reads the name and the type of each row of a structure query.
fn table_columns(rows: &[GridRow]) -> Vec<TableColumn> {
    let text_of = |value: Option<Value<'_>>| value.map_or_else(String::new, Value::display);
    rows.iter()
        .map(|row| {
            let mut cells = row.cells();
            let name = text_of(cells.next());
            let data_type = text_of(cells.next());
            TableColumn { name, data_type }
        })
        .collect()
}

impl App {
    /// Returns true when the tab on screen can open the import dialog.
    pub(super) fn can_import(&self) -> bool {
        let Some(tab) = self.table() else {
            return false;
        };
        self.connection_can_import(tab.connection)
            && tab.filters
            && !tab.is_view
            && !tab.is_busy()
            && !self.has_import(tab.id)
    }

    fn has_import(&self, id: ConsoleId) -> bool {
        self.imports.iter().any(|run| run.console == id)
    }

    /// Returns true when the row under the cursor of the navigator can receive a file.
    pub(super) fn can_import_row(&self) -> bool {
        let row = self.navigator.current(&self.connections);
        let Some(row) = row.filter(|row| row.kind == RowKind::Table) else {
            return false;
        };
        self.connection_can_import(self.connections[row.connection].id)
    }

    fn connection_can_import(&self, id: ConnectionId) -> bool {
        self.connection(id)
            .is_some_and(|item| item.dialect != Dialect::Redis && !self.is_read_only(id))
    }

    /// Opens the table of a row, and then its dialog. The first page uses the tab, so
    /// the dialog waits for the end of that page.
    pub(super) fn import_row(&mut self, row: &Row) {
        if !self.can_import_row() {
            return;
        }
        self.open_table(row);
        let Some(tab) = self.table().map(|tab| tab.id) else {
            return;
        };
        self.import_wanted = Some(tab);
        self.continue_wanted_import();
    }

    /// Opens the dialog of a table that waited for its first page.
    pub(super) fn continue_wanted_import(&mut self) {
        let Some(id) = self.import_wanted else {
            return;
        };
        let index = self.tables.iter().position(|tab| tab.id == id);
        let Some(tab) = index.map(|index| &self.tables[index]) else {
            self.import_wanted = None;
            return;
        };
        if tab.is_busy() {
            return;
        }
        self.import_wanted = None;
        if self.table().is_some_and(|tab| tab.id == id) {
            self.open_import();
        }
    }

    pub(super) fn clear_wanted_import(&mut self, id: ConsoleId) {
        if self.import_wanted == Some(id) {
            self.import_wanted = None;
        }
    }

    /// Reads the columns of the table. The dialog opens with their types.
    pub(super) fn open_import(&mut self) {
        if self.pending_edit_guard() {
            return;
        }
        if !self.can_import() || self.config_blocked() {
            return;
        }
        let Some(tab) = self.table() else {
            return;
        };
        let (id, connection) = (tab.id, tab.connection);
        let Some(dialect) = self.connection(connection).map(|item| item.dialect) else {
            return;
        };
        let sql = structure_sql(dialect, Scope::Relation, &tab.relation, Section::Columns);
        if let Some(sql) = sql {
            let step = ImportStep::Columns(sql);
            self.authorize(connection, Use::Import { console: id, step });
        }
    }

    /// Runs a step after the credentials.
    pub(super) fn start_import_step(&mut self, id: ConsoleId, step: ImportStep) {
        let Some(manager) = &self.manager else {
            return;
        };
        if !self.tables.iter().any(|tab| tab.id == id) {
            return;
        }
        let work = match step {
            ImportStep::Columns(sql) => Work::Columns {
                execution: manager.run(id, vec![sql], RunOptions::default()),
                rows: Vec::new(),
            },
            ImportStep::Write(job) => {
                let job = *job;
                let progress = job.progress.clone();
                let log = job.log.clone();
                Work::Writing {
                    execution: manager.import(id, job),
                    progress,
                    log,
                }
            }
        };
        if let Some((phase, clock)) = self.tab_run(id) {
            *phase = Phase::Waiting;
            clock.start();
        }
        self.imports.push(ImportRun {
            console: id,
            work,
            failure: None,
        });
    }

    /// Takes an event of an import. It returns false for any other event, and the tab
    /// never sees these events, so its rows stay on the grid.
    pub(super) fn on_import_event(&mut self, event: &mut ExecEvent) -> bool {
        let Some(execution) = event.execution() else {
            return false;
        };
        let found = self
            .imports
            .iter()
            .position(|run| run.execution() == execution);
        let Some(index) = found else {
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
            ExecEvent::Rows { rows, bytes, .. } => {
                self.budget.release(*bytes);
                if let Work::Columns { rows: kept, .. } = &mut self.imports[index].work {
                    kept.append(rows);
                }
            }
            ExecEvent::StatementFinished {
                status: StatementStatus::Error(message),
                ..
            } => self.imports[index].failure = Some(message.clone()),
            ExecEvent::ExecutionFinished { end, message, .. } => {
                let run = self.imports.remove(index);
                self.end_import(run, *end, message.take());
            }
            _ => {}
        }
        true
    }

    fn end_import(&mut self, run: ImportRun, end: ExecutionEnd, message: Option<String>) {
        let id = run.console;
        if let Some((phase, clock)) = self.tab_run(id) {
            *phase = Phase::Idle;
            clock.finish(end);
        }
        self.note_long_run(id);
        let failure = run.failure.or(message);
        match run.work {
            Work::Columns { rows, .. } => self.show_import_form(id, &rows, end, failure),
            Work::Writing { progress, log, .. } => {
                self.show_import_report(id, &progress, &log, end, failure);
            }
        }
    }

    fn show_import_form(
        &mut self,
        id: ConsoleId,
        rows: &[GridRow],
        end: ExecutionEnd,
        failure: Option<String>,
    ) {
        if end != ExecutionEnd::Completed {
            let reason = failure.unwrap_or_else(|| text::IMPORT_STOPPED.to_owned());
            self.status = Some(text::import_failed(&reason));
            return;
        }
        let tab = self.tables.iter().find(|tab| tab.id == id);
        let Some(table) = tab.and_then(|tab| self.import_table_of(tab, rows)) else {
            return;
        };
        // A change of tab during the read leaves the dialog without its table on screen.
        if self.table().is_some_and(|tab| tab.id == id) {
            let stamp = time_stamp();
            self.import_form = Some(ImportForm::new(table, &self.import_choices, &stamp));
            // A read of an older dialog can have the number of a read of this one.
            self.import_probes = ProbeChannel::default();
        }
    }

    fn import_table_of(&self, tab: &TableTab, rows: &[GridRow]) -> Option<ImportTable> {
        let dialect = self.connection(tab.connection)?.dialect;
        Some(ImportTable {
            name: tab.relation.relation.clone(),
            table: from_clause(&tab.relation, dialect),
            dialect,
            columns: table_columns(rows),
        })
    }

    fn show_import_report(
        &mut self,
        id: ConsoleId,
        progress: &ImportProgress,
        log: &Path,
        end: ExecutionEnd,
        failure: Option<String>,
    ) {
        if !self.tables.iter().any(|tab| tab.id == id) {
            self.status = Some(text::import_closed(progress.rows()));
            return;
        }
        let log = (progress.errors() > 0).then(|| log.display().to_string());
        let reason = match end {
            ExecutionEnd::Completed => None,
            ExecutionEnd::Stopped | ExecutionEnd::CanceledBeforeStart => {
                Some(text::IMPORT_STOPPED.to_owned())
            }
            _ => failure,
        };
        let body = text::import_report(&text::ImportReport {
            rows: progress.rows(),
            errors: progress.errors(),
            ignored: progress.ignored_keys(),
            log: log.as_deref(),
            failure: reason.as_deref(),
        });
        let title = if end == ExecutionEnd::Completed {
            text::TITLE_IMPORT_DONE
        } else {
            text::TITLE_IMPORT_FAILED
        };
        self.show_dialog(
            title,
            body,
            vec![(text::OPTION_CLOSE, DialogAction::Dismiss)],
        );
        self.reload_imported(id);
    }

    /// Reads the table again. Only the tab on screen can start a read, so another tab
    /// keeps its rows until the user asks.
    fn reload_imported(&mut self, id: ConsoleId) {
        if self.table().is_some_and(|tab| tab.id == id) {
            self.apply_table();
        }
    }

    pub(super) fn on_import_action(&mut self, action: ImportAction) {
        match action {
            ImportAction::None => {}
            ImportAction::Cancel => self.import_form = None,
            ImportAction::Import => self.confirm_import(),
            ImportAction::Browse => self.open_browser(),
        }
        self.request_probe();
    }

    /// Starts the read that the fields of the dialog ask for.
    pub(super) fn request_probe(&mut self) {
        let Some(form) = &mut self.import_form else {
            return;
        };
        let Some(request) = form.probe_request() else {
            return;
        };
        let Some(manager) = &self.manager else {
            return;
        };
        let ProbeRequest {
            serial,
            path,
            options,
            detect,
        } = request;
        let sender = self.import_probes.sender.clone();
        manager.spawn_blocking(move || {
            let _ = sender.send((serial, probe(&path, options, detect)));
        });
    }

    /// Takes the answers of the reads. It returns true when the dialog changed.
    pub(super) fn poll_probes(&mut self) -> bool {
        let mut changed = false;
        while let Ok((serial, probe)) = self.import_probes.receiver.try_recv() {
            if let Some(form) = &mut self.import_form {
                form.on_probe(serial, probe);
                changed = true;
            }
        }
        if changed {
            self.request_probe();
        }
        changed
    }

    /// A connection in production asks first, as a run of a change does.
    fn confirm_import(&mut self) {
        let Some(form) = &mut self.import_form else {
            return;
        };
        if let Err(note) = form.job() {
            form.note = Some(note);
            return;
        }
        let shown = form.path_text().to_owned();
        let name = form.table().name.clone();
        let Some(connection) = self.table().and_then(|tab| self.connection(tab.connection)) else {
            return;
        };
        if connection.is_production() {
            let body = text::import_production(&shown, &name, connection.plain_name());
            self.show_dialog(
                text::TITLE_IMPORT_PRODUCTION,
                body,
                vec![
                    (text::OPTION_CANCEL, DialogAction::Dismiss),
                    (text::BUTTON_IMPORT, DialogAction::ImportConfirmed),
                ],
            );
        } else {
            self.start_import();
        }
    }

    pub(super) fn start_import(&mut self) {
        let Some(form) = self.import_form.take() else {
            return;
        };
        let Ok(job) = form.job() else {
            return;
        };
        self.save_import_choices(form.choices(&job.path));
        let Some(tab) = self.table() else {
            return;
        };
        let (console, connection) = (tab.id, tab.connection);
        let step = ImportStep::Write(Box::new(job));
        self.authorize(connection, Use::Import { console, step });
    }

    /// Returns the progress of the tab on screen for the hint row.
    pub(super) fn import_progress(&self) -> Option<String> {
        let id = self.table()?.id;
        let run = self.imports.iter().find(|run| run.console == id)?;
        let Work::Writing { progress, .. } = &run.work else {
            return None;
        };
        Some(text::import_progress(
            progress.rows(),
            progress.errors(),
            progress.percent(),
            self.shown_key(Action::Stop).as_deref(),
        ))
    }
}
