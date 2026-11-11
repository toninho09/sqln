//! One command without the terminal interface.

mod auth;
#[cfg(unix)]
mod output;

use std::io::{self, BufWriter, IsTerminal, Read, Write};
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
use crate::grid::{Budget, Row, printable};
use crate::sql::{self, Dialect};
use crate::store::{Scope, Store, config};

/// The largest script that `--execute -` reads from standard input.
const STDIN_LIMIT: usize = 10 * 1024 * 1024;

pub struct Options {
    pub store: Store,
    pub target: Option<String>,
    pub connection: Option<String>,
    pub scope: Option<Scope>,
    pub sql: String,
    /// `None` keeps standard output empty; row counts go to the error stream.
    pub output: Option<ExportFormat>,
    /// True runs the whole script in one transaction that rolls back on failure.
    pub single_transaction: bool,
    /// True keeps running after a failed instruction.
    pub continue_on_error: bool,
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

/// Reads the SQL of `--execute -` from standard input.
/// The read happens before SQLN resolves a connection, so a password question
/// never comes before an input failure.
pub fn read_standard_input() -> Result<String, Error> {
    if io::stdin().is_terminal() {
        return Err(Error::usage(
            "--execute - needs SQL on standard input, not an interactive terminal.",
        ));
    }
    let mut bytes = Vec::new();
    let mut limited = io::stdin().take(STDIN_LIMIT as u64 + 1);
    if let Err(error) = limited.read_to_end(&mut bytes) {
        return Err(Error::failed(format!(
            "Standard input cannot be read: {error}"
        )));
    }
    if bytes.len() > STDIN_LIMIT {
        return Err(Error::usage(format!(
            "--execute - input exceeds {} MiB.",
            STDIN_LIMIT / (1024 * 1024)
        )));
    }
    String::from_utf8(bytes).map_err(|_| Error::failed("Standard input is not valid UTF-8."))
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
            crate::connector::target::Engine::Kafka => Dialect::Kafka,
        },
    };
    let statements: Vec<String> = sql::split(&options.sql, dialect)
        .iter()
        .map(|statement| statement.text(&options.sql).to_owned())
        .collect();
    if statements.is_empty() {
        return Err(Error::usage(
            "--execute needs SQL: the text has no instruction.",
        ));
    }
    if options.single_transaction && dialect.is_command_line() {
        return Err(Error::usage(
            "--single-transaction does not support Redis or Kafka.",
        ));
    }
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
    let (mut manager, mut events) = Manager::new_streaming(budget, stream_stopped.clone())?;
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
    let mut sinks = Sinks::new(options.output, &signals.stopped, &stream_stopped)?;
    let mut context = Drain {
        manager: &manager,
        events: &mut events,
        console,
        stopped: &signals.stopped,
    };
    crate::log::event("batch started");
    let result = execute(&mut context, &statements, &options, &mut sinks);
    manager.close_console(console);
    manager.remove_connection(connection);
    manager.shutdown();
    result
}

/// The shared context of the drained executions of one invocation.
struct Drain<'a> {
    manager: &'a Manager,
    events: &'a mut tokio::sync::mpsc::Receiver<ExecEvent>,
    console: u64,
    stopped: &'a AtomicBool,
}

/// Runs the script and, with `--single-transaction`, the statement that ends the
/// transaction.
fn execute(
    context: &mut Drain<'_>,
    statements: &[String],
    options: &Options,
    sinks: &mut Sinks<'_>,
) -> Result<(), Error> {
    let execution = context.manager.run(
        context.console,
        statements.to_vec(),
        RunOptions {
            stream: true,
            manual: options.single_transaction,
            hold_transaction: options.single_transaction,
            continue_on_error: options.continue_on_error,
            ..RunOptions::default()
        },
    );
    let mut summary = drain(context, execution, true, options.continue_on_error, sinks)?;
    if summary.signal {
        return Err(Error::failed(
            "The execution stops after a signal. Output can be incomplete.",
        ));
    }
    if !options.single_transaction {
        return finish(&summary, statements.len(), options.continue_on_error, sinks);
    }
    // Flush the result before COMMIT so an output failure can still roll back.
    if !summary.failed()
        && let Err(error) = sinks.finish()
    {
        summary.fail_output(error);
    }
    // The transaction stays open on the held session. End it with one statement.
    let (close_sql, kind) = if summary.failed() {
        ("ROLLBACK", "rollback")
    } else {
        ("COMMIT", "commit")
    };
    let closing_execution = context.manager.run(
        context.console,
        vec![close_sql.to_owned()],
        RunOptions {
            stream: true,
            ..RunOptions::default()
        },
    );
    let closing = drain(context, closing_execution, false, false, sinks)?;
    if summary.failed() {
        return finish(&summary, statements.len(), options.continue_on_error, sinks);
    }
    if closing.failed() {
        return Err(Error::failed(format!(
            "The {kind} failed: {}",
            describe(&closing, 1, false)
        )));
    }
    Ok(())
}

/// Writes the final bytes and turns a failed summary into the report error.
fn finish(
    summary: &Summary,
    total: usize,
    continue_on_error: bool,
    sinks: &mut Sinks<'_>,
) -> Result<(), Error> {
    if summary.failed() {
        return Err(Error::failed(describe(summary, total, continue_on_error)));
    }
    sinks.finish()?;
    Ok(())
}

/// One text line that explains a failed run.
fn describe(summary: &Summary, total: usize, continue_on_error: bool) -> String {
    if let Some(output) = &summary.output_failed {
        return output.clone();
    }
    if let Some(message) = &summary.run_message {
        return message.clone();
    }
    if !summary.failures.is_empty() {
        if continue_on_error {
            return format!(
                "{} of {} instructions failed.",
                summary.failures.len(),
                total
            );
        }
        let (index, message) = &summary.failures[0];
        return format!("[{index}] {message}");
    }
    format!("The execution did not complete: {:?}.", summary.end)
}

/// What one execution reported.
#[derive(Default)]
struct Summary {
    end: Option<ExecutionEnd>,
    run_message: Option<String>,
    /// Instruction failures: the one-based number and the message.
    failures: Vec<(usize, String)>,
    output_failed: Option<String>,
    signal: bool,
}

impl Summary {
    /// Stores the first output failure and returns true for that failure.
    fn fail_output(&mut self, error: io::Error) -> bool {
        let first = self.output_failed.is_none();
        self.output_failed
            .get_or_insert_with(|| format!("Output failed: {error}. Output can be incomplete."));
        first
    }

    fn failed(&self) -> bool {
        self.end != Some(ExecutionEnd::Completed)
            || self.run_message.is_some()
            || !self.failures.is_empty()
            || self.output_failed.is_some()
    }
}

/// Reads the events of one execution until it ends.
///
/// `numbered` marks the run of the user script: its reports carry instruction
/// numbers. The closing statement of a transaction runs without numbers.
fn drain(
    context: &mut Drain<'_>,
    execution: u64,
    numbered: bool,
    continue_on_error: bool,
    sinks: &mut Sinks<'_>,
) -> Result<Summary, Error> {
    let manager = context.manager;
    let console = context.console;
    let mut summary = Summary::default();
    let mut stopping = false;
    loop {
        if context.stopped.load(Ordering::Relaxed) && !stopping {
            manager.stop(console);
            stopping = true;
            summary.signal = true;
        }
        let event = match context.events.blocking_recv() {
            Some(event) => event,
            None => {
                return Err(Error::failed("The executor closed before completion."));
            }
        };
        if event.console() != Some(console) || event.execution().is_some_and(|id| id != execution) {
            continue;
        }
        let written = match event {
            ExecEvent::Columns { columns, .. } => sinks.start_set(&columns),
            ExecEvent::Rows { rows, .. } => sinks.rows(&rows),
            ExecEvent::ResultDone { key, rows, .. } => {
                sinks.returned(usize::from(numbered) * (key.index + 1), rows)
            }
            ExecEvent::Notice { text, .. } => sinks.notice(&text),
            ExecEvent::StatementFinished {
                key, status, end, ..
            } => {
                let index = usize::from(numbered) * (key.index + 1);
                match status {
                    StatementStatus::Success => {
                        if numbered
                            && !end.is_some_and(|end| end.has_columns)
                            && let Some(changed) = end.and_then(|end| end.changed)
                        {
                            sinks.affected(index, changed)
                        } else {
                            Ok(())
                        }
                    }
                    StatementStatus::Error(message) => {
                        summary.failures.push((index, message.clone()));
                        if continue_on_error {
                            sinks.failure_line(index, &message)
                        } else {
                            Ok(())
                        }
                    }
                    other => {
                        summary
                            .failures
                            .push((index, format!("The statement did not complete: {other:?}.")));
                        Ok(())
                    }
                }
            }
            ExecEvent::ConfirmClose { .. } => {
                // Batch execution has no transaction that can continue after the
                // process ends. A dropped session rolls back on the server.
                manager.confirm_close(console, true);
                Ok(())
            }
            ExecEvent::ExecutionFinished { end, message, .. } => {
                summary.end = Some(end);
                summary.run_message = message;
                return Ok(summary);
            }
            _ => Ok(()),
        };
        if let Err(error) = written
            && summary.fail_output(error)
            && !stopping
        {
            manager.stop(console);
            stopping = true;
        }
    }
}

/// The writers of one invocation: the error stream and the optional result encoder.
struct Sinks<'a> {
    errors: ErrorsSink<'a>,
    encoder: Option<Encoder<StdoutSink<'a>>>,
    /// True when at least one result set arrived. Without a set, standard output
    /// stays empty: no format writes its empty form.
    started: bool,
}

#[cfg(unix)]
type ErrorsSink<'a> = output::Output<'a>;
#[cfg(not(unix))]
type ErrorsSink<'a> = io::Stderr;

#[cfg(unix)]
type StdoutSink<'a> = BufWriter<output::Output<'a>>;
#[cfg(not(unix))]
type StdoutSink<'a> = BufWriter<io::Stdout>;

impl<'a> Sinks<'a> {
    fn new(
        format: Option<ExportFormat>,
        stopped: &'a AtomicBool,
        stream_stopped: &'a StreamControl,
    ) -> io::Result<Self> {
        #[cfg(unix)]
        let errors = output::Output::new(io::stderr(), stopped, stream_stopped)?;
        #[cfg(not(unix))]
        {
            let _ = (stopped, stream_stopped);
            let errors = io::stderr();
            let encoder = match format {
                Some(format) => Some(Encoder::new(
                    BufWriter::new(io::stdout()),
                    ExportOptions {
                        format,
                        ..ExportOptions::default()
                    },
                )),
                None => None,
            };
            return Ok(Self {
                errors,
                encoder,
                started: false,
            });
        }
        #[cfg(unix)]
        let encoder = match format {
            Some(format) => Some(Encoder::new(
                BufWriter::new(output::Output::new(io::stdout(), stopped, stream_stopped)?),
                ExportOptions {
                    format,
                    ..ExportOptions::default()
                },
            )),
            None => None,
        };
        #[cfg(unix)]
        Ok(Self {
            errors,
            encoder,
            started: false,
        })
    }

    /// Starts one result set. Without `--output`, the set only counts at its end.
    fn start_set(&mut self, columns: &[String]) -> io::Result<()> {
        self.started = true;
        match &mut self.encoder {
            Some(encoder) => encoder.start_set(columns),
            None => Ok(()),
        }
    }

    fn rows(&mut self, rows: &[Row]) -> io::Result<()> {
        match &mut self.encoder {
            Some(encoder) => rows.iter().try_for_each(|row| encoder.row(row.cells())),
            None => Ok(()),
        }
    }

    /// Reports the rows of one set when standard output stays empty.
    /// A zero index marks the closing statement of a transaction, which reports nothing.
    fn returned(&mut self, index: usize, rows: usize) -> io::Result<()> {
        if self.encoder.is_none() && index > 0 {
            writeln!(self.errors, "sqln: [{index}] {rows} rows returned.")
        } else {
            Ok(())
        }
    }

    fn notice(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.errors, "sqln: {}", printable(text))
    }

    fn affected(&mut self, index: usize, changed: u64) -> io::Result<()> {
        writeln!(self.errors, "sqln: [{index}] {changed} rows affected.")
    }

    fn failure_line(&mut self, index: usize, message: &str) -> io::Result<()> {
        writeln!(self.errors, "sqln: [{index}] {}", printable(message))
    }

    /// Ends the last set and sends the waiting bytes. Without a set, the writer
    /// sends nothing, so a script of writes leaves standard output empty.
    fn finish(&mut self) -> io::Result<()> {
        match (&mut self.encoder, self.started) {
            (Some(encoder), true) => encoder.finish(),
            _ => Ok(()),
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
