//! One command without the terminal interface.

mod auth;
#[cfg(unix)]
mod output;

use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::connection::{Definition, Source};
use crate::connector::duckdb::{DuckdbTarget, file_source as duckdb_source};
use crate::connector::sqlite::{SqliteTarget, file_source as sqlite_source};
use crate::connector::target::{OpenTarget, open_target};
use crate::exec::{
    ExecEvent, ExecutionEnd, Manager, RunOptions, Settings, StatementStatus, StreamControl,
};
use crate::export::{Encoder, ExportFormat, ExportOptions};
use crate::grid::{Budget, printable};
use crate::sql::{self, Dialect};
use crate::store::{Scope, Store, config};

pub struct Options {
    pub store: Store,
    pub target: Option<String>,
    pub connection: Option<String>,
    pub scope: Option<Scope>,
    pub sql: String,
    pub format: ExportFormat,
    pub yes: bool,
    pub password_file: Option<PathBuf>,
}

#[derive(Debug)]
pub struct Error {
    pub message: String,
    pub usage: bool,
}

impl Error {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            usage: true,
        }
    }

    fn failed(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            usage: false,
        }
    }
}

/// Reports a final failure without waiting for a full error stream on Unix.
pub fn report_error(message: &str) {
    let message = format!("sqln: {}\n", printable(message));
    #[cfg(unix)]
    output::report_error(message.as_bytes());
    #[cfg(not(unix))]
    let _ = io::stderr().write_all(message.as_bytes());
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::failed(format!("I/O failed: {error}"))
    }
}

/// Resolves a target without opening a database or asking for credentials.
fn resolve(options: &Options) -> Result<(Definition, Option<config::Stored>), Error> {
    let config = config::load(&options.store)
        .config
        .map_err(|error| Error::failed(error.to_string()))?;
    if let Some(name) = &options.connection {
        let name = name.to_lowercase();
        let mut matches = config.connections.into_iter().filter(|stored| {
            stored.definition.name.to_lowercase() == name
                && options.scope.is_none_or(|scope| scope == stored.scope)
        });
        let stored = matches
            .next()
            .ok_or_else(|| Error::usage("Connection not found. Check --connection and --scope."))?;
        if matches.next().is_some() {
            return Err(Error::usage(
                "The connection name is ambiguous. Set --scope global or --scope project.",
            ));
        }
        return Ok((stored.definition.clone(), Some(stored)));
    }
    let target = options
        .target
        .as_deref()
        .ok_or_else(|| Error::usage("--execute needs a target or --connection <name>."))?;
    let definition = match open_target(target).map_err(Error::usage)? {
        OpenTarget::File(path) => Definition::sqlite(SqliteTarget {
            source: sqlite_source(&path.to_string_lossy()),
            ..SqliteTarget::new(path)
        }),
        OpenTarget::Duckdb(text) => Definition::duckdb(DuckdbTarget {
            source: duckdb_source(&text),
            allow_create: false,
            read_only: false,
        }),
        OpenTarget::Network(target) => Definition::new(target.label(), Source::Network(target)),
    };
    Ok((definition, None))
}

fn allow_create(definition: &mut Definition, yes: bool) -> Result<(), Error> {
    let missing = match definition.resolved_source() {
        Source::Sqlite(target) if !target.is_memory() && !target.source_exists() => {
            Some(target.path().to_owned())
        }
        Source::Duckdb(target) if !target.is_memory() && !target.source_exists() => {
            Some(target.path().to_owned())
        }
        _ => None,
    };
    if let Some(path) = missing {
        if definition.read_only {
            return Err(Error::failed(
                "The database file does not exist. A read-only connection cannot create it.",
            ));
        }
        if !yes {
            auth::confirm(
                &format!(
                    "Create database {}?",
                    printable(&path.display().to_string())
                ),
                "Use --yes to authorize database creation.",
            )?;
        }
        match &mut definition.source {
            Source::Sqlite(target) => target.allow_create = true,
            Source::Duckdb(target) => target.allow_create = true,
            _ => {}
        }
    }
    Ok(())
}

/// Runs without restoring or changing consoles, workspace, or export preferences.
pub fn run(options: Options) -> Result<(), Error> {
    let (mut definition, stored) = resolve(&options)?;
    let dialect = match &definition.source {
        Source::Sqlite(_) => Dialect::Sqlite,
        Source::Duckdb(_) => Dialect::DuckDb,
        Source::Network(target) => match target.engine {
            crate::connector::target::Engine::Postgres => Dialect::Postgres,
            crate::connector::target::Engine::MySql => Dialect::MySql,
            crate::connector::target::Engine::Redis => Dialect::Redis,
        },
    };
    let statements = sql::split(&options.sql, dialect);
    if statements.len() != 1 {
        return Err(Error::usage(
            "--execute needs exactly one instruction. Scripts are not supported.",
        ));
    }
    let statement = statements[0].text(&options.sql).to_owned();
    let statements = vec![statement];
    if !options.yes
        && let Some(risk) = sql::run_risk(
            &statements,
            dialect,
            definition.read_only,
            definition.production,
        )
    {
        let first: String = printable(&statements[risk.first])
            .chars()
            .take(200)
            .collect();
        auth::confirm(
            &format!(
                "Connection: {}\nProduction changes: {}. Statements without WHERE: {}.\n{}\nExecute?",
                printable(&definition.name),
                risk.changes,
                risk.no_where,
                first
            ),
            "Use --yes to authorize this execution.",
        )?;
    }
    allow_create(&mut definition, options.yes)?;
    auth::authenticate(&mut definition, stored.as_ref(), &options)?;

    let signals = Signals::new()?;
    let budget = Arc::new(Budget::new(Settings::default().budget_bytes));
    let stream_stopped = Arc::new(StreamControl::default());
    let (mut manager, events) = Manager::new_streaming(budget, stream_stopped.clone())?;
    let settings = definition.settings(&Settings {
        row_cap: usize::MAX,
        cap_in_sql: false,
        ..Settings::default()
    });
    let connection = manager.add_connection(definition.connector(), settings);
    let console = manager
        .open_console(connection)
        .expect("The connection exists.");
    manager.stop_when(console, signals.stopped.clone());
    let execution = manager.run(
        console,
        statements,
        RunOptions {
            stream: true,
            ..RunOptions::default()
        },
    );
    crate::log::event("batch started");
    let result = deliver(
        &manager,
        events,
        console,
        execution,
        options.format,
        &signals.stopped,
        &stream_stopped,
    );
    manager.close_console(console);
    manager.remove_connection(connection);
    manager.shutdown();
    result
}

fn deliver(
    manager: &Manager,
    mut events: tokio::sync::mpsc::Receiver<ExecEvent>,
    console: u64,
    execution: u64,
    format: ExportFormat,
    stopped: &AtomicBool,
    stream_stopped: &StreamControl,
) -> Result<(), Error> {
    #[cfg(unix)]
    let mut errors = output::Output::new(io::stderr(), stopped, stream_stopped)?;
    #[cfg(not(unix))]
    let mut errors = io::stderr();
    #[cfg(unix)]
    let output = output::Output::new(io::stdout(), stopped, stream_stopped)?;
    #[cfg(not(unix))]
    let output = {
        let _ = stream_stopped;
        io::stdout()
    };
    let mut encoder = Encoder::new(
        BufWriter::new(output),
        ExportOptions {
            format,
            ..ExportOptions::default()
        },
    );
    let mut columns = false;
    let mut failure = None;
    let mut stopping = false;
    loop {
        if stopped.load(Ordering::Relaxed) && !stopping {
            manager.stop(console);
            stopping = true;
            failure = Some(Error::failed(
                "The execution stops after a signal. Output can be incomplete.",
            ));
        }
        let event = match events.blocking_recv() {
            Some(event) => event,
            None => {
                return Err(failure
                    .unwrap_or_else(|| Error::failed("The executor closed before completion.")));
            }
        };
        if event.console() != Some(console) || event.execution().is_some_and(|id| id != execution) {
            continue;
        }
        let written =
            match event {
                ExecEvent::Columns { columns: names, .. } if failure.is_none() => {
                    if columns {
                        failure = Some(Error::failed(
                            "Multiple results are not supported. Output can be incomplete.",
                        ));
                        Ok(())
                    } else {
                        columns = true;
                        encoder.start(&names)
                    }
                }
                ExecEvent::Rows { rows, .. } if failure.is_none() => {
                    rows.iter().try_for_each(|row| encoder.row(row.cells()))
                }
                ExecEvent::Notice { text, .. } => writeln!(errors, "sqln: {}", printable(&text)),
                ExecEvent::StatementFinished { status, end, .. } => match status {
                    StatementStatus::Success => {
                        if !columns && let Some(changed) = end.and_then(|end| end.changed) {
                            writeln!(errors, "sqln: {changed} rows affected.")
                        } else {
                            Ok(())
                        }
                    }
                    StatementStatus::Error(message) => {
                        failure.get_or_insert_with(|| Error::failed(message));
                        Ok(())
                    }
                    other => {
                        failure.get_or_insert_with(|| {
                            Error::failed(format!("The statement did not complete: {other:?}."))
                        });
                        Ok(())
                    }
                },
                ExecEvent::ConfirmClose { .. } => {
                    // Batch execution has no transaction that can continue after the process ends.
                    manager.confirm_close(console, true);
                    Ok(())
                }
                ExecEvent::ExecutionFinished { end, message, .. } => {
                    if end != ExecutionEnd::Completed || message.is_some() {
                        failure.get_or_insert_with(|| {
                            Error::failed(message.unwrap_or_else(|| {
                                format!("The execution did not complete: {end:?}.")
                            }))
                        });
                    }
                    if let Some(error) = failure {
                        return Err(error);
                    }
                    if columns {
                        encoder.finish()?;
                    }
                    return Ok(());
                }
                _ => Ok(()),
            };
        if let Err(error) = written {
            failure.get_or_insert_with(|| {
                Error::failed(format!("Output failed: {error}. Output can be incomplete."))
            });
        }
        if failure.is_some() && !stopping {
            manager.stop(console);
            stopping = true;
        }
    }
}

struct Signals {
    stopped: Arc<AtomicBool>,
    #[cfg(unix)]
    registrations: Vec<signal_hook::SigId>,
}

impl Signals {
    fn new() -> io::Result<Self> {
        let stopped = Arc::new(AtomicBool::new(false));
        #[cfg(unix)]
        {
            let mut signals = Self {
                stopped,
                registrations: Vec::new(),
            };
            for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
                signals.registrations.push(signal_hook::flag::register(
                    signal,
                    signals.stopped.clone(),
                )?);
            }
            Ok(signals)
        }
        #[cfg(not(unix))]
        {
            Ok(Self { stopped })
        }
    }
}

#[cfg(unix)]
impl Drop for Signals {
    fn drop(&mut self) {
        for id in self.registrations.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}
