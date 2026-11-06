use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sqln::command_log::{HISTORY_BYTES, History, LogEvent, TraceContext, limited};
use sqln::connector::sqlite::{SqliteConnector, SqliteTarget};
use sqln::connector::{Connector, TransactionCommand};
use sqln::console::MessageLevel;
use sqln::exec::{ConsoleId, ExecEvent, Manager, RunOptions, Settings};
use sqln::grid::Budget;
use tokio::sync::mpsc::{self, Receiver};

fn started(id: u64, text: &str) -> LogEvent {
    LogEvent::Started {
        id,
        key: None,
        user: true,
        system: false,
        header_len: 0,
        dialect: sqln::sql::Dialect::Postgres,
        text: text.to_owned(),
    }
}

fn finished(id: u64) -> LogEvent {
    LogEvent::Finished {
        id,
        level: MessageLevel::Info,
        text: "Completed.".to_owned(),
        elapsed: Duration::from_micros(12345),
    }
}

fn texts(history: &History) -> String {
    history
        .iter()
        .map(|message| message.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_history_updates_one_command_without_adding_a_second_record() {
    let mut history = History::default();
    history.on_event(started(1, "SELECT 1"));
    history.on_event(finished(1));
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].text, "SELECT 1\nTime: 12.345 ms. Completed.");
}

#[test]
fn the_budget_removes_whole_records_and_ignores_their_late_answers() {
    let mut history = History::with_limit(2048);
    for id in 1..100 {
        history.on_event(started(id, &format!("{id}: {}", "x".repeat(100))));
        history.on_event(finished(id));
        assert!(history.bytes() <= 2048);
    }
    let before = texts(&history);
    history.on_event(finished(1));
    assert_eq!(texts(&history), before);
    assert!(
        history
            .iter()
            .all(|message| !message.text.starts_with("1: "))
    );
    assert!(before.contains("99: "));
    let mut scroll = 0;
    history.viewport(&mut scroll, 80, 100);
    scroll = 0;
    assert!(
        history.viewport(&mut scroll, 80, 100)[0]
            .text
            .contains("Older records removed")
    );
}

#[test]
fn a_large_command_has_a_utf8_safe_truncation_marker_and_fits_the_budget() {
    let mut history = History::with_limit(2048);
    history.on_event(started(1, &"é🙂".repeat(10_000)));
    assert_eq!(history.len(), 1);
    assert!(history[0].text.ends_with("[Text truncated.]"));
    history.on_event(finished(1));
    assert!(history.bytes() <= 2048);
    assert!(history[0].text.contains("[Text truncated.]"));
}

#[test]
fn the_producer_limits_text_before_it_enters_the_event_queue() {
    let original = "é".repeat(HISTORY_BYTES);
    let text = limited(&original);
    assert!(text.len() < HISTORY_BYTES);
    assert!(text.ends_with("[Text truncated.]"));
    assert_eq!(original.len(), HISTORY_BYTES * 2);
}

#[test]
fn the_view_follows_only_when_the_reader_is_at_the_end() {
    let mut history = History::default();
    for index in 0..20 {
        history.push(MessageLevel::Info, &format!("row {index}"), None);
    }
    let mut scroll = 0;
    assert_eq!(history.viewport(&mut scroll, 80, 3)[2].text, "row 19");
    scroll -= 5;
    let before = history.viewport(&mut scroll, 80, 3);
    history.push(MessageLevel::Info, "new row", None);
    assert_eq!(history.viewport(&mut scroll, 80, 3), before);
    scroll = usize::MAX;
    assert_eq!(history.viewport(&mut scroll, 80, 3)[2].text, "new row");
    history.push(MessageLevel::Info, "next row", None);
    assert_eq!(history.viewport(&mut scroll, 80, 3)[2].text, "next row");
}

#[test]
fn the_view_reaches_more_than_65535_rows_and_neutralizes_control_characters() {
    let mut history = History::default();
    history.push(
        MessageLevel::Info,
        &format!("{}last\x1b\t", "row\n".repeat(70_000)),
        None,
    );
    let mut scroll = 0;
    let rows = history.viewport(&mut scroll, 80, 2);
    assert!(scroll > 65_535);
    assert_eq!(rows[1].text, "last�→");
    let rows = history.viewport(&mut scroll, 3, 2);
    assert!(!rows.iter().any(|message| message.text.contains('\x1b')));
}

#[test]
fn a_running_command_does_not_claim_success() {
    let mut history = History::default();
    history.on_event(started(1, "SELECT slow()"));
    let rows = history.viewport(&mut 0, 80, 10);
    assert!(rows.last().unwrap().text.contains("Result unknown"));
    history.on_event(finished(1));
    let rows = history.viewport(&mut 0, 80, 10);
    assert_eq!(rows.last().unwrap().text, "Time: 12.345 ms. Completed.");
}

#[test]
fn a_dropped_operation_reports_an_unknown_result() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (events, mut inbox) = mpsc::channel(8);
    let trace = TraceContext::new(1, 1, "db".to_owned(), false, events);
    runtime.block_on(async {
        let span = trace.start("SELECT slow()").await;
        tokio::time::sleep(Duration::from_millis(2)).await;
        drop(span);
    });
    assert!(matches!(
        inbox.try_recv().unwrap(),
        ExecEvent::CommandLog {
            event: LogEvent::Started { .. },
            ..
        }
    ));
    let ExecEvent::CommandLog {
        event: LogEvent::Finished { text, elapsed, .. },
        ..
    } = inbox.try_recv().unwrap()
    else {
        panic!("no result")
    };
    assert!(text.contains("Result unknown"));
    assert!(elapsed >= Duration::from_millis(2));
}

#[test]
fn statement_notes_stay_with_the_user_command_instead_of_a_later_internal_call() {
    let mut history = History::default();
    let key = sqln::exec::StatementKey {
        console: 1,
        execution: 1,
        index: 0,
    };
    history.note(MessageLevel::Info, "Added LIMIT.", Some(key));
    history.on_event(LogEvent::Started {
        id: 1,
        key: Some(key),
        user: true,
        system: false,
        header_len: 0,
        dialect: sqln::sql::Dialect::Postgres,
        text: "SELECT 1 LIMIT 2".to_owned(),
    });
    history.on_event(LogEvent::Started {
        id: 2,
        key: Some(key),
        user: false,
        system: true,
        header_len: 0,
        dialect: sqln::sql::Dialect::Postgres,
        text: "internal probe".to_owned(),
    });
    history.note(MessageLevel::Warning, "database notice", Some(key));
    history.on_event(finished(1));
    assert_eq!(history.len(), 2);
    assert!(history[0].text.contains("Added LIMIT"));
    assert!(history[0].text.contains("database notice"));
    assert_eq!(history[0].level, MessageLevel::Warning);
    assert!(!history[1].text.contains("database notice"));
}

#[test]
fn an_oversized_record_keeps_its_result_after_text_truncation() {
    let mut history = History::with_limit(2048);
    history.on_event(started(1, &"x".repeat(100_000)));
    history.on_event(finished(1));
    assert!(history[0].text.ends_with("Time: 12.345 ms. Completed."));
    assert!(history[0].text.contains("Text truncated"));
    assert!(history.bytes() <= 2048);
}

#[test]
fn system_commands_start_hidden_but_keep_their_records_and_results() {
    let mut history = History::default();
    history.on_event(started(1, "user command"));
    for (id, user, text) in [(2, false, "internal command"), (3, true, "shared command")] {
        history.on_event(LogEvent::Started {
            id,
            key: None,
            user,
            system: true,
            header_len: 0,
            dialect: sqln::sql::Dialect::Postgres,
            text: text.to_owned(),
        });
        history.on_event(finished(id));
    }
    history.push(
        MessageLevel::Info,
        "Internal is also a word in user messages.",
        None,
    );
    let visible = |history: &mut History| {
        history
            .viewport(&mut 0, 100, 100)
            .into_iter()
            .map(|message| message.text)
            .collect::<Vec<_>>()
            .join("\n")
    };
    let hidden = visible(&mut history);
    assert!(hidden.contains("user command"));
    assert!(hidden.contains("Internal is also a word"));
    assert!(!hidden.contains("internal command"));
    assert!(!hidden.contains("shared command"));
    let count = history.len();
    let bytes = history.bytes();
    history.set_show_system_commands(true);
    let shown = visible(&mut history);
    assert!(shown.contains("internal command"));
    assert!(shown.contains("shared command"));
    assert!(shown.contains("Time: 12.345 ms."));
    history.set_show_system_commands(false);
    assert_eq!(visible(&mut history), hidden);
    assert_eq!(history.len(), count);
    assert_eq!(history.bytes(), bytes);
}

#[test]
fn hidden_commands_take_no_scroll_rows_and_filter_changes_keep_the_user_anchor() {
    let mut history = History::default();
    history.push(MessageLevel::Info, "first user row", None);
    history.on_event(LogEvent::Started {
        id: 1,
        key: None,
        user: false,
        system: true,
        header_len: 0,
        dialect: sqln::sql::Dialect::Postgres,
        text: "hidden row\n".repeat(1000),
    });
    history.push(MessageLevel::Info, "last user row", None);
    let mut scroll = 0;
    assert_eq!(
        history.viewport(&mut scroll, 100, 1)[0].text,
        "last user row"
    );
    assert_eq!(scroll, 1);
    scroll = 0;
    assert_eq!(
        history.viewport(&mut scroll, 100, 1)[0].text,
        "first user row"
    );
    history.set_show_system_commands(true);
    assert_eq!(
        history.viewport(&mut scroll, 100, 1)[0].text,
        "first user row"
    );
    history.set_show_system_commands(false);
    assert_eq!(
        history.viewport(&mut scroll, 100, 1)[0].text,
        "first user row"
    );
}

#[test]
fn a_history_with_only_hidden_commands_has_no_visible_rows() {
    let mut history = History::default();
    history.on_event(LogEvent::Started {
        id: 1,
        key: None,
        user: false,
        system: true,
        header_len: 0,
        dialect: sqln::sql::Dialect::Postgres,
        text: "internal command".to_owned(),
    });
    history.on_event(finished(1));
    assert!(!history.is_empty());
    assert!(history.viewport(&mut 0, 80, 10).is_empty());
    history.set_show_system_commands(true);
    assert!(!history.viewport(&mut 0, 80, 10).is_empty());
}

#[test]
fn a_user_error_stays_visible_when_only_internal_preparation_reaches_the_database() {
    let mut history = History::default();
    let key = sqln::exec::StatementKey {
        console: 1,
        execution: 1,
        index: 0,
    };
    history.on_event(LogEvent::Started {
        id: 1,
        key: Some(key),
        user: false,
        system: true,
        header_len: 0,
        dialect: sqln::sql::Dialect::Postgres,
        text: "internal preparation".to_owned(),
    });
    history.note(MessageLevel::Error, "Statement 1: syntax error", Some(key));
    let rows = history.viewport(&mut 0, 80, 10);
    assert!(
        rows.iter()
            .any(|message| message.text == "Statement 1: syntax error"
                && message.level == MessageLevel::Error)
    );
    assert!(
        !rows
            .iter()
            .any(|message| message.text.contains("internal preparation"))
    );
}

#[test]
fn elapsed_time_excludes_the_wait_to_deliver_the_completion_event() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (events, mut inbox) = mpsc::channel(1);
    let trace = TraceContext::new(1, 1, "db".to_owned(), false, events);
    runtime.block_on(async {
        let before = Instant::now();
        let span = trace.start("SELECT 1").await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        let finish = span.finish(&Ok::<_, String>(()));
        let maximum = before.elapsed();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(matches!(
            inbox.try_recv().unwrap(),
            ExecEvent::CommandLog {
                event: LogEvent::Started { .. },
                ..
            }
        ));
        finish.await;
        let ExecEvent::CommandLog {
            event: LogEvent::Finished { elapsed, .. },
            ..
        } = inbox.try_recv().unwrap()
        else {
            panic!("no result")
        };
        assert!(elapsed >= Duration::from_millis(5));
        assert!(elapsed <= maximum);
        assert!(before.elapsed() >= maximum + Duration::from_millis(20));
    });
}

#[test]
fn an_oversized_error_keeps_the_command_time_visible() {
    let mut history = History::with_limit(2048);
    history.on_event(started(1, "bad command"));
    history.on_event(LogEvent::Finished {
        id: 1,
        level: MessageLevel::Error,
        text: "error detail ".repeat(10_000),
        elapsed: Duration::from_millis(25),
    });
    assert!(history[0].text.contains("Time: 25.000 ms."));
    assert!(history[0].text.contains("Text truncated"));
    assert!(history.bytes() <= 2048);
}

#[test]
fn blocking_commands_keep_elapsed_time_on_errors() {
    let (events, mut inbox) = mpsc::channel(8);
    let trace = TraceContext::new(1, 1, "db".to_owned(), false, events);
    let result = sqln::command_log::observe_blocking(&Some(trace), "bad command", || {
        std::thread::sleep(Duration::from_millis(5));
        Err::<(), _>("database error")
    });
    assert_eq!(result, Err("database error"));
    inbox.try_recv().unwrap();
    let ExecEvent::CommandLog {
        event:
            LogEvent::Finished {
                elapsed,
                text,
                level,
                ..
            },
        ..
    } = inbox.try_recv().unwrap()
    else {
        panic!("no result")
    };
    assert!(elapsed >= Duration::from_millis(5));
    assert_eq!(level, MessageLevel::Error);
    assert!(text.contains("database error"));
}

struct Harness {
    manager: Manager,
    events: Receiver<ExecEvent>,
    histories: HashMap<u64, History>,
    connection: u64,
    first: ConsoleId,
    second: ConsoleId,
    first_history: u64,
    second_history: u64,
}

impl Harness {
    fn new(name: &str) -> Self {
        let path = crate::common::temp_db(name);
        rusqlite::Connection::open(&path).unwrap();
        let connector = Arc::new(SqliteConnector::new(SqliteTarget::new(path)));
        Self::connect(connector)
    }

    fn connect(connector: Arc<dyn Connector>) -> Self {
        let (mut manager, events) = Manager::new(Arc::new(Budget::new(1024 * 1024))).unwrap();
        let connection = manager.add_connection(connector, Settings::default());
        let first = manager.open_console(connection).unwrap();
        let second = manager.open_console(connection).unwrap();
        let a = History::default();
        let b = History::default();
        let first_history = a.id;
        let second_history = b.id;
        manager.set_console_history(first, a.id, "database".to_owned());
        manager.set_console_history(second, b.id, "database".to_owned());
        Self {
            manager,
            events,
            histories: HashMap::from([(a.id, a), (b.id, b)]),
            connection,
            first,
            second,
            first_history,
            second_history,
        }
    }

    fn until(&mut self, done: impl Fn(&ExecEvent) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(event) = self.events.try_recv() {
                let finished = done(&event);
                if let ExecEvent::CommandLog { target, event } = event
                    && let Some(history) = self.histories.get_mut(&target)
                {
                    history.on_event(event);
                }
                if finished {
                    return;
                }
            } else {
                assert!(Instant::now() < deadline, "no event arrived");
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }

    fn run(&mut self, console: ConsoleId, sql: &str, manual: bool) {
        let execution = self.manager.run(
            console,
            vec![sql.to_owned()],
            RunOptions {
                manual,
                ..RunOptions::default()
            },
        );
        self.until(|event| matches!(event, ExecEvent::ExecutionFinished { execution: id, .. } if *id == execution));
    }
}

#[test]
fn sqlite_records_the_sent_sql_and_keeps_each_console_separate() {
    let mut h = Harness::new("command-log-isolation");
    h.run(h.first, "SELECT 123 AS first_value", false);
    h.run(h.second, "SELECT 456 AS second_value", false);
    h.run(h.first, "SELECT 789 AS next_value", false);
    let first = texts(&h.histories[&h.first_history]);
    let second = texts(&h.histories[&h.second_history]);
    assert!(first.contains("PRAGMA query_only = OFF"));
    assert!(
        first.contains("SELECT 123 AS first_value\nLIMIT 1001"),
        "{first}"
    );
    assert!(first.contains("SELECT 789 AS next_value\nLIMIT 1001"));
    assert!(!first.contains("second_value"));
    assert!(second.contains("second_value"));
    assert!(!second.contains("first_value"));
}

#[test]
fn the_manager_records_the_connector_dialect_for_history_highlighting() {
    let mut h = Harness::new("command-log-dialect");
    h.run(h.first, "SELECT 1 AS [select]", false);
    let theme = sqln::ui::theme::Theme::ansi();
    let history = h.histories.get_mut(&h.first_history).unwrap();
    let rows = sqln::ui::messages::viewport(history, &mut 0, 90, 30, &theme);
    assert!(
        rows.iter()
            .flat_map(|row| &row.spans)
            .any(|span| span.content == "[select]" && span.style == theme.identifier)
    );
}

#[test]
fn shared_commands_keep_the_active_destination_from_the_start() {
    let mut h = Harness::new("command-log-shared");
    h.manager.set_active_history(Some(h.first));
    h.manager.load_catalog(h.connection, 1, Vec::new());
    h.manager.set_active_history(Some(h.second));
    h.until(|event| matches!(event, ExecEvent::CatalogLoaded { read: 1, .. }));
    assert!(texts(&h.histories[&h.first_history]).contains("Shared"));
    assert!(texts(&h.histories[&h.first_history]).contains("sqlite_schema"));
    assert!(h.histories[&h.second_history].is_empty());
}

#[test]
fn a_shared_command_without_an_active_console_has_no_history() {
    let mut h = Harness::new("command-log-no-active");
    h.manager.load_catalog(h.connection, 1, Vec::new());
    h.until(|event| matches!(event, ExecEvent::CatalogLoaded { read: 1, .. }));
    assert!(h.histories.values().all(History::is_empty));
}

#[test]
fn begin_commit_and_rollback_keep_the_console_destination() {
    let mut h = Harness::new("command-log-transactions");
    h.run(h.first, "SELECT 1", true);
    h.manager.set_active_history(Some(h.second));
    let first = h.first;
    h.manager.transaction(h.first, TransactionCommand::Commit);
    h.until(|event| matches!(event, ExecEvent::TransactionChanged { console, state: sqln::connector::TransactionState::None } if *console == first));
    h.run(h.first, "SELECT 2", true);
    h.manager.transaction(h.first, TransactionCommand::Rollback);
    h.until(|event| matches!(event, ExecEvent::TransactionChanged { console, state: sqln::connector::TransactionState::None } if *console == first));
    let history = texts(&h.histories[&h.first_history]);
    for command in ["BEGIN", "COMMIT", "ROLLBACK"] {
        assert!(history.contains(command), "{history}");
    }
    assert!(h.histories[&h.second_history].is_empty());
}

#[test]
fn metadata_keeps_the_parameterized_sql_and_separate_values() {
    let mut h = Harness::new("command-log-parameters");
    h.manager.set_active_history(Some(h.first));
    h.manager.load_columns(
        h.connection,
        sqln::catalog::RelationRef {
            schema: "main".to_owned(),
            relation: "missing ' table".to_owned(),
        },
    );
    h.until(|event| matches!(event, ExecEvent::ColumnsLoaded { .. }));
    let history = texts(&h.histories[&h.first_history]);
    assert!(history.contains("pragma_table_info(?1, ?2)"));
    assert!(history.contains("?1=\"missing ' table\", ?2=\"main\""));
}

#[test]
fn a_closed_history_does_not_redirect_shared_events_to_another_console() {
    let mut h = Harness::new("command-log-closed");
    h.manager.set_active_history(Some(h.first));
    h.manager.load_catalog(h.connection, 1, Vec::new());
    h.manager.close_console(h.first);
    h.histories.remove(&h.first_history);
    h.manager.set_active_history(Some(h.second));
    h.until(|event| matches!(event, ExecEvent::CatalogLoaded { .. }));
    assert!(h.histories[&h.second_history].is_empty());
}

#[test]
fn imports_leave_no_commands_in_any_console_on_success_failure_or_stop() {
    use sqln::import::statement::{Target, TargetColumn};
    use sqln::import::{ImportJob, ImportMode, ImportProgress, ReadOptions};
    for outcome in ["success", "failure", "stop"] {
        let mut h = Harness::new(&format!("command-log-import-{outcome}"));
        h.run(h.first, "CREATE TABLE t(a TEXT)", false);
        let before = texts(&h.histories[&h.first_history]);
        let path =
            crate::common::temp_db(&format!("command-log-input-{outcome}")).with_extension("csv");
        if outcome != "failure" {
            std::fs::write(&path, "a\nx\ny\n").unwrap();
        }
        h.manager.set_active_history(Some(h.first));
        let job = ImportJob {
            path: path.clone(),
            read: ReadOptions::default(),
            columns: vec!["a".to_owned()],
            target: Target {
                table: "t".to_owned(),
                columns: vec![TargetColumn {
                    name: "a".to_owned(),
                    binary: false,
                    source: 0,
                }],
                dialect: sqln::sql::Dialect::Sqlite,
                base64: false,
            },
            mode: ImportMode::AllOrNothing,
            batch: 1,
            error_limit: 1,
            log: path.with_extension("log"),
            progress: Arc::new(ImportProgress::default()),
        };
        let execution = h.manager.import(h.first, job);
        if outcome == "stop" {
            h.manager.stop(h.first);
        }
        h.until(|event| matches!(event, ExecEvent::ExecutionFinished { execution: id, .. } if *id == execution));
        assert_eq!(texts(&h.histories[&h.first_history]), before, "{outcome}");
        assert!(h.histories[&h.second_history].is_empty());
        h.run(h.first, "SELECT count(*) FROM t", false);
        assert!(texts(&h.histories[&h.first_history]).contains("SELECT count(*) FROM t"));
    }
}

#[test]
fn shared_commands_do_not_enter_an_active_console_of_another_connection() {
    let mut h = Harness::new("command-log-other-connection");
    let path = crate::common::temp_db("command-log-other-db");
    rusqlite::Connection::open(&path).unwrap();
    let connection = h.manager.add_connection(
        Arc::new(SqliteConnector::new(SqliteTarget::new(path))),
        Settings::default(),
    );
    let console = h.manager.open_console(connection).unwrap();
    h.manager
        .set_console_history(console, h.second_history, "other".to_owned());
    h.manager.set_active_history(Some(console));
    h.manager.load_catalog(h.connection, 1, Vec::new());
    h.until(|event| matches!(event, ExecEvent::CatalogLoaded { .. }));
    assert!(h.histories.values().all(History::is_empty));
}

struct ReusableConnector {
    inner: SqliteConnector,
    opened: std::sync::atomic::AtomicUsize,
}

struct ReusableSession {
    inner: Box<dyn sqln::connector::Session>,
    trace: Option<TraceContext>,
}

#[async_trait::async_trait]
impl Connector for ReusableConnector {
    fn dialect(&self) -> sqln::sql::Dialect {
        sqln::sql::Dialect::Sqlite
    }
    async fn connect(
        &self,
    ) -> Result<Box<dyn sqln::connector::Session>, sqln::connector::ConnectError> {
        self.opened
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(Box::new(ReusableSession {
            inner: self.inner.connect().await?,
            trace: None,
        }))
    }
}

#[async_trait::async_trait]
impl sqln::connector::Session for ReusableSession {
    fn set_trace_context(&mut self, trace: Option<TraceContext>) {
        self.inner.set_trace_context(trace.clone());
        self.trace = trace;
    }
    fn canceller(&self) -> Arc<dyn sqln::connector::Canceller> {
        self.inner.canceller()
    }
    async fn apply_context(
        &mut self,
        context: &sqln::connector::SessionContext,
    ) -> Result<(), sqln::connector::ExecError> {
        self.inner.apply_context(context).await
    }
    async fn run(
        &mut self,
        sql: &str,
        sink: sqln::exec::ResultSink,
    ) -> Result<sqln::connector::StatementEnd, sqln::connector::ExecError> {
        self.inner.run(sql, sink).await
    }
    async fn transaction_state(&mut self) -> sqln::connector::TransactionState {
        self.inner.transaction_state().await
    }
    async fn control(
        &mut self,
        command: TransactionCommand,
    ) -> Result<(), sqln::connector::ExecError> {
        self.inner.control(command).await
    }
    async fn reset(&mut self) -> sqln::connector::ResetOutcome {
        let result: Result<_, String> =
            sqln::command_log::observe(self.trace.clone(), "TestReset", async {
                Ok(sqln::connector::ResetOutcome::Clean)
            })
            .await;
        result.unwrap()
    }
    async fn is_alive(&mut self) -> bool {
        let result: Result<_, String> =
            sqln::command_log::observe(self.trace.clone(), "TestPing", async { Ok(true) }).await;
        result.unwrap()
    }
    async fn read_catalog(&mut self) -> Result<sqln::catalog::Catalog, sqln::connector::ExecError> {
        self.inner.read_catalog().await
    }
    async fn read_columns(
        &mut self,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<String>, sqln::connector::ExecError> {
        self.inner.read_columns(schema, relation).await
    }
}

#[test]
fn a_reused_session_changes_its_destination_before_the_health_check() {
    let path = crate::common::temp_db("command-log-reuse");
    rusqlite::Connection::open(&path).unwrap();
    let connector = Arc::new(ReusableConnector {
        inner: SqliteConnector::new(SqliteTarget::new(path)),
        opened: std::sync::atomic::AtomicUsize::new(0),
    });
    let mut h = Harness::connect(connector.clone());
    h.run(h.first, "SELECT 111", false);
    let first = texts(&h.histories[&h.first_history]);
    h.run(h.second, "SELECT 222", false);
    assert_eq!(
        connector.opened.load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    assert_eq!(texts(&h.histories[&h.first_history]), first);
    let second = texts(&h.histories[&h.second_history]);
    assert!(second.contains("TestPing"));
    assert!(second.contains("TestReset"));
    assert!(second.contains("SELECT 222"));
    assert!(!second.contains("SELECT 111"));
}

#[test]
fn a_disconnected_history_consumer_does_not_stop_the_database_command() {
    let path = crate::common::temp_db("command-log-no-consumer");
    rusqlite::Connection::open(&path).unwrap();
    let connector = SqliteConnector::new(SqliteTarget::new(path));
    let (events, inbox) = mpsc::channel(8);
    drop(inbox);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let trace = TraceContext::new(1, 1, "db".to_owned(), false, events);
        let mut session = connector.connect_traced(Some(trace)).await.unwrap();
        session.control(TransactionCommand::Begin).await.unwrap();
        session.control(TransactionCommand::Rollback).await.unwrap();
    });
}
