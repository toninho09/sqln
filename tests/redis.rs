// Tests of Redis and Valkey. The tests of text and replies need no server. The other
// tests need `scripts/test-env.sh`, and they print "skipped" without its variables.
//
// Each test uses its own keys, so the tests can run at the same time.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::ctrl;
use crossterm::event::{Event, KeyCode};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use redis::Value as Reply;
use sqln::app::{App, Options};
use sqln::catalog::RelationRef;
use sqln::clipboard::MemoryClipboard;
use sqln::complete::suggest;
use sqln::connection::{Definition, Source};
use sqln::connector::redis::{MAX_KEYS, keyspace_databases, reply_grid, split_words, value_page};
use sqln::connector::target::{Engine, parse_uri};
use sqln::connector::{ConnectError, Namespace, Session, network_connector};
use sqln::editor::KeyboardMode;
use sqln::exec::{
    ConsoleId, ExecEvent, ExecutionEnd, Manager, RunOptions, Settings, StatementStatus,
};
use sqln::form::{ConnectionForm, Field, Kind};
use sqln::grid::{Budget, Row, Value};
use sqln::sql::{Dialect, split};
use sqln::store::config::{KindName, to_definition, to_entry};
use sqln::table::TableFocus;
use sqln::ui::theme::Theme;
use tokio::sync::mpsc::Receiver;

fn words(line: &str) -> Vec<String> {
    let found = split_words(line).unwrap();
    let text = |word: Vec<u8>| String::from_utf8(word).unwrap();
    found.into_iter().map(text).collect()
}

#[test]
fn continuous_commands_are_known_by_their_words() {
    use sqln::sql::is_continuous;
    for command in [
        "SUBSCRIBE a",
        "psubscribe a*",
        "SSUBSCRIBE a",
        "MONITOR",
        "SYNC",
        "PSYNC ? -1",
        "XREAD BLOCK 0 STREAMS k $",
        "xread count 5 block 100 streams k 0",
        "XREADGROUP GROUP g c BLOCK 0 STREAMS k >",
        "# comment\nMONITOR",
    ] {
        assert!(is_continuous(command, Dialect::Redis), "{command}");
    }
    for command in [
        "XREAD STREAMS BLOCK 0",
        "XREAD COUNT 1 STREAMS k 0",
        "XREADGROUP GROUP BLOCK BLOCK STREAMS k >",
        "PUBLISH c m",
        "GET SUBSCRIBE",
        "\"MONITOR\"",
    ] {
        assert!(!is_continuous(command, Dialect::Redis), "{command}");
    }
    assert!(!is_continuous("SUBSCRIBE", Dialect::Postgres));
}

#[test]
fn the_live_reader_reads_resp2_replies() {
    use sqln::connector::redis::live::{Frame, parse};
    let bulk = |text: &str| Frame::Bulk(Some(text.as_bytes().to_vec()));
    let message = b"*3\r\n$7\r\nmessage\r\n$1\r\nc\r\n$2\r\nhi\r\n+OK\r\n";
    let (frame, size) = parse(message).unwrap().unwrap();
    assert_eq!(
        frame,
        Frame::Array(Some(vec![bulk("message"), bulk("c"), bulk("hi")]))
    );
    assert_eq!(&message[size..], b"+OK\r\n");
    for end in 0..size {
        assert_eq!(
            parse(&message[..end]).unwrap(),
            None,
            "a part waits for more"
        );
    }
    assert_eq!(parse(b":-3\r\n").unwrap().unwrap().0, Frame::Int(-3));
    assert_eq!(parse(b"$-1\r\n").unwrap().unwrap().0, Frame::Bulk(None));
    assert_eq!(
        parse(b"-ERR no\r\n").unwrap().unwrap().0,
        Frame::Error("ERR no".to_owned())
    );
    assert!(parse(b"?bad\r\n").is_err());
    assert!(parse(b"$2\r\nabc\r\n").is_err());
}

#[test]
fn the_live_reader_drops_the_rdb_file_in_small_parts() {
    use sqln::connector::redis::live::{Frame, Reader};
    use tokio::io::AsyncWriteExt;
    runtime().block_on(async {
        let (mut write, read) = tokio::io::duplex(3);
        tokio::spawn(async move {
            let data = b"\n\n$10\r\n0123456789*1\r\n$4\r\nPING\r\n";
            write.write_all(data).await.unwrap();
        });
        let mut reader = Reader::new(read);
        assert_eq!(reader.skip_rdb().await.unwrap(), 10);
        let (frame, size) = reader.frame().await.unwrap();
        assert_eq!(
            frame,
            Frame::Array(Some(vec![Frame::Bulk(Some(b"PING".to_vec()))]))
        );
        assert_eq!(size, 14);
        let error = reader.frame().await.unwrap_err();
        assert!(error.message.contains("closed"), "{error:?}");
    });
}

#[test]
fn a_monitor_line_fills_its_columns() {
    use sqln::connector::redis::live::monitor_row;
    let row = texts(&monitor_row(
        r#"1700000000.123456 [0 127.0.0.1:5000] "SET" "k" "v""#,
    ));
    assert_eq!(
        row,
        [
            "1700000000.123456",
            "0",
            "127.0.0.1:5000",
            r#""SET" "k" "v""#
        ]
    );
    let row = texts(&monitor_row(r#"1700000000.1 [3 lua] "GET" "k""#));
    assert_eq!(row, ["1700000000.1", "3", "lua", r#""GET" "k""#]);
    let row = texts(&monitor_row("unknown text"));
    assert_eq!(row, ["NULL", "NULL", "NULL", "unknown text"]);
}

#[test]
fn a_line_splits_as_in_redis_cli() {
    assert_eq!(words("  GET   a "), ["GET", "a"]);
    assert_eq!(words(r#"SET a "b c""#), ["SET", "a", "b c"]);
    assert_eq!(words("SET a 'b c'"), ["SET", "a", "b c"]);
    assert_eq!(words(r#"SET a "x\ny\t\"z\"""#), ["SET", "a", "x\ny\t\"z\""]);
    assert_eq!(words(r"SET a 'it\'s'"), ["SET", "a", "it's"]);
    assert_eq!(words(r"SET a 'x\ny'"), ["SET", "a", r"x\ny"]);
    assert_eq!(words(r#"SET user#1 pre"b c""#), ["SET", "user#1", "preb c"]);
    assert_eq!(words(r#"SET a """#), ["SET", "a", ""]);
    assert_eq!(words("SET a é"), ["SET", "a", "é"]);
}

#[test]
fn a_hex_escape_gives_raw_bytes() {
    let found = split_words(r#"SET a "\xff\x00z""#).unwrap();
    assert_eq!(found[2], [0xff, 0, b'z']);
}

#[test]
fn a_quote_that_does_not_close_has_a_position() {
    let error = split_words(r#"SET a "b"#).unwrap_err();
    assert_eq!(error.offset, Some(6));
    let error = split_words(r#"SET a "b"c"#).unwrap_err();
    assert_eq!(error.offset, Some(6));
}

#[test]
fn a_key_page_keeps_any_name() {
    for name in [
        "plain",
        "with space",
        r#"quo"te"#,
        r"back\slash",
        "line\nbreak",
        "é",
    ] {
        let relation = RelationRef {
            schema: "3".to_owned(),
            relation: name.to_owned(),
        };
        let line = value_page(&relation, 20, 11);
        assert_eq!(
            words(&line),
            ["SQLN.VALUE", "3", name, "20", "11"],
            "{line}"
        );
    }
}

fn texts(row: &Row) -> Vec<String> {
    row.cells()
        .map(|value| match value {
            Value::Null => "NULL".to_owned(),
            Value::Text(text) | Value::Number(text) => text.to_string(),
            Value::Binary(_) => "BINARY".to_owned(),
        })
        .collect()
}

fn bulk(text: &str) -> Reply {
    Reply::BulkString(text.as_bytes().to_vec())
}

#[test]
fn each_reply_shape_has_its_grid() {
    assert!(reply_grid(&Reply::Okay).is_none());

    let (columns, rows) = reply_grid(&bulk("v")).unwrap();
    assert_eq!(columns, ["value"]);
    assert_eq!(texts(&rows[0]), ["v"]);

    let (_, rows) = reply_grid(&Reply::Nil).unwrap();
    assert_eq!(rows[0].get(0).unwrap(), Value::Null);

    let (_, rows) = reply_grid(&Reply::Int(7)).unwrap();
    assert_eq!(rows[0].get(0).unwrap(), Value::Number("7"));

    let (_, rows) = reply_grid(&Reply::BulkString(vec![0xff])).unwrap();
    assert_eq!(rows[0].get(0).unwrap(), Value::binary(&[0xff]));

    let (columns, rows) = reply_grid(&Reply::Array(vec![bulk("a"), bulk("b")])).unwrap();
    assert_eq!(columns, ["value"]);
    assert_eq!(rows.len(), 2);

    let (columns, rows) = reply_grid(&Reply::Map(vec![(bulk("f"), bulk("v"))])).unwrap();
    assert_eq!(columns, ["field", "value"]);
    assert_eq!(texts(&rows[0]), ["f", "v"]);
}

#[test]
fn a_nested_value_is_text() {
    let nested = Reply::Array(vec![Reply::Array(vec![bulk("a"), Reply::Int(1)])]);
    let (_, rows) = reply_grid(&nested).unwrap();
    assert_eq!(texts(&rows[0]), ["[a, 1]"]);

    let nested = Reply::Array(vec![Reply::Map(vec![(bulk("k"), Reply::Nil)])]);
    let (_, rows) = reply_grid(&nested).unwrap();
    assert_eq!(texts(&rows[0]), ["{k: nil}"]);
}

#[test]
fn the_keyspace_text_gives_the_databases() {
    let info =
        "# Keyspace\r\ndb0:keys=3,expires=0,avg_ttl=0\r\ndb12:keys=1,expires=0,avg_ttl=0\r\n";
    assert_eq!(keyspace_databases(info), [0, 12]);
    assert!(keyspace_databases("# Keyspace\r\n").is_empty());
}

#[test]
fn each_line_is_one_command() {
    let text = "GET a\n\n# a note\n  SET b \"c d\"\nDEL x;y\nSET a --b\n";
    let found: Vec<&str> = split(text, Dialect::Redis)
        .iter()
        .map(|statement| &text[statement.start..statement.end])
        .collect();
    assert_eq!(found, ["GET a", "SET b \"c d\"", "DEL x;y", "SET a --b"]);
}

#[test]
fn an_open_quote_ends_with_its_line() {
    let text = "SET a \"b\nGET c\n";
    assert_eq!(split(text, Dialect::Redis).len(), 2);
}

#[test]
fn a_hash_starts_a_comment_only_at_the_start_of_a_line() {
    let text = "SET user#1 #2\n";
    let found = split(text, Dialect::Redis);
    assert_eq!(found[0].text(text), "SET user#1 #2");
}

fn labels(text: &str) -> Vec<String> {
    suggest(text, text.len(), Dialect::Redis, None)
        .items
        .into_iter()
        .map(|item| item.label)
        .collect()
}

#[test]
fn the_list_holds_commands_at_the_start_of_a_line() {
    assert!(labels("GE").contains(&"GET".to_owned()));
    assert!(labels("ge").contains(&"get".to_owned()));
    assert!(labels("GET a\nHGETA").contains(&"HGETALL".to_owned()));
    assert!(labels("WHE").is_empty(), "SQL words stay out");
}

#[test]
fn the_arguments_have_no_list() {
    assert!(labels("GET ").is_empty());
    assert!(labels("GET ke").is_empty());
    assert!(labels("# GE").is_empty());
}

fn form() -> ConnectionForm {
    let mut form = ConnectionForm::new();
    form.kind = Kind::Redis;
    form.focus_on(Field::Name);
    form.on_paste("cache");
    form.focus_on(Field::Host);
    form.on_paste("127.0.0.1");
    form
}

#[test]
fn the_form_needs_no_user() {
    let definition = form().definition().unwrap();
    let Source::Network(target) = &definition.source else {
        panic!("not a network source");
    };
    assert_eq!(target.engine, Engine::Redis);
    assert_eq!(target.user, "");
    assert_eq!(definition.summary(), "Redis/Valkey 127.0.0.1:6379 · no TLS");
}

#[test]
fn the_form_refuses_a_database_name() {
    let mut form = form();
    form.focus_on(Field::Database);
    form.on_paste("sales");
    let error = form.definition().unwrap_err();
    assert!(error.message.contains("must be a number"), "{error:?}");
    assert_eq!(error.field, Field::Database);
}

#[test]
fn the_form_reads_a_saved_definition() {
    let definition = form().definition().unwrap();
    let form = ConnectionForm::editing(1, &definition, false);
    assert_eq!(form.kind, Kind::Redis);
    assert_eq!(form.definition().unwrap(), definition);
}

#[test]
fn a_saved_connection_keeps_its_kind_without_a_user() {
    let definition = form().definition().unwrap();
    let entry = to_entry("one", &definition);
    assert_eq!(entry.kind, KindName::Redis);
    assert_eq!(entry.username, None);
    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains(r#""kind":"redis""#), "{json}");

    let mut read = to_definition(&entry, "/base".into());
    read.base = None;
    assert_eq!(read, definition);
}

#[cfg(unix)]
#[tokio::test]
async fn a_socket_refuses_tls() {
    let target = parse_uri("rediss:///0?socket=/tmp/sqln-none.sock").unwrap();
    let error = network_connector(target).connect().await.err().unwrap();
    assert_eq!(error.message, sqln::ui::text::REDIS_SOCKET_TLS);
}

#[cfg(unix)]
#[tokio::test]
async fn a_missing_socket_reports_an_error() {
    let path = common::temp_dir("redis-no-socket").join("none.sock");
    let target = parse_uri(&format!("redis:///0?socket={}", path.display())).unwrap();
    let error = network_connector(target).connect().await.err().unwrap();
    assert!(!error.message.is_empty());
    assert!(!error.password_refused);
}

// The tests below need a server.

fn uris() -> Vec<String> {
    let mut found = Vec::new();
    for name in ["SQLN_TEST_REDIS", "SQLN_TEST_VALKEY"] {
        match std::env::var(name) {
            Ok(uri) => found.push(uri),
            Err(_) => println!("skipped: {name} is not set"),
        }
    }
    found
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn try_connect(uri: &str) -> Result<Box<dyn Session>, ConnectError> {
    let connector = network_connector(parse_uri(uri).unwrap());
    runtime().block_on(connector.connect())
}

#[derive(Debug, Default)]
struct Run {
    columns: Vec<Vec<String>>,
    rows: Vec<Row>,
    statuses: Vec<StatementStatus>,
    end: Option<ExecutionEnd>,
    message: Option<String>,
    commands: Vec<String>,
}

impl Run {
    fn grid(&self) -> Vec<Vec<String>> {
        self.rows.iter().map(texts).collect()
    }

    fn error(&self) -> &str {
        self.statuses
            .iter()
            .find_map(|status| match status {
                StatementStatus::Error(message) => Some(message.as_str()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no error: {self:?}"))
    }
}

struct Harness {
    manager: Manager,
    events: Receiver<ExecEvent>,
    console: ConsoleId,
}

impl Harness {
    fn open(uri: &str, settings: Settings) -> Self {
        let budget = Arc::new(Budget::new(settings.budget_bytes));
        let (mut manager, events) = Manager::new(budget).unwrap();
        let connector = network_connector(parse_uri(uri).unwrap());
        let connection = manager.add_connection(connector, settings);
        let console = manager.open_console(connection).unwrap();
        Self {
            manager,
            events,
            console,
        }
    }

    fn next(&mut self) -> ExecEvent {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Ok(event) = self.events.try_recv() {
                return event;
            }
            assert!(Instant::now() < deadline, "no event arrived");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn start(&mut self, statements: &[&str]) -> u64 {
        let statements = statements.iter().map(|text| (*text).to_owned()).collect();
        self.manager
            .run(self.console, statements, RunOptions::default())
    }

    fn collect(&mut self, execution: u64) -> Run {
        let mut run = Run::default();
        loop {
            match self.next() {
                ExecEvent::CommandLog {
                    event: sqln::command_log::LogEvent::Started { text, .. },
                    ..
                } => run.commands.push(text),
                ExecEvent::Columns { columns, .. } => run.columns.push(columns),
                ExecEvent::Rows { rows, .. } => run.rows.extend(rows),
                ExecEvent::StatementFinished { status, .. } => run.statuses.push(status),
                ExecEvent::ExecutionFinished {
                    execution: finished,
                    end,
                    message,
                    ..
                } if finished == execution => {
                    run.end = Some(end);
                    run.message = message;
                    return run;
                }
                _ => {}
            }
        }
    }

    fn run(&mut self, statements: &[&str]) -> Run {
        let execution = self.start(statements);
        self.collect(execution)
    }

    fn run_ok(&mut self, statements: &[&str]) -> Run {
        let run = self.run(statements);
        assert_eq!(
            run.end,
            Some(ExecutionEnd::Completed),
            "{statements:?}: {run:?}"
        );
        run
    }

    fn wait_started(&mut self) {
        while !matches!(self.next(), ExecEvent::StatementStarted { .. }) {}
    }

    /// Starts a run as a console does, so a continuous command is accepted.
    fn start_live(&mut self, statements: &[&str]) -> u64 {
        let statements = statements.iter().map(|text| (*text).to_owned()).collect();
        let options = RunOptions {
            live: true,
            ..RunOptions::default()
        };
        self.manager.run(self.console, statements, options)
    }

    /// Waits for the columns of a live result. They arrive after the server accepted the
    /// command.
    fn wait_columns(&mut self) -> (Vec<String>, Option<usize>) {
        loop {
            match self.next() {
                ExecEvent::Columns { columns, live, .. } => return (columns, live),
                ExecEvent::ExecutionFinished { end, message, .. } => {
                    panic!("the run ended: {end:?} {message:?}")
                }
                _ => {}
            }
        }
    }

    /// Waits for rows until one of them matches.
    fn wait_row(&mut self, found: impl Fn(&[String]) -> bool) -> Vec<String> {
        loop {
            match self.next() {
                ExecEvent::Rows { rows, .. } => {
                    if let Some(row) = rows.iter().map(texts).find(|row| found(row)) {
                        return row;
                    }
                }
                ExecEvent::ExecutionFinished { end, message, .. } => {
                    panic!("the run ended: {end:?} {message:?}")
                }
                _ => {}
            }
        }
    }

    /// Stops the live run and returns its end.
    fn stop_live(&mut self, execution: u64) -> Run {
        self.manager.stop(self.console);
        self.collect(execution)
    }
}

#[test]
fn redis_command_history_keeps_actual_arguments_and_internal_cleanup() {
    for uri in uris() {
        let mut h = Harness::open(&uri, Settings::default());
        h.manager
            .set_console_history(h.console, 1, "cache".to_owned());
        let run = h.run_ok(&[r#"ECHO "\xff\x00quoted\"value""#]);
        let commands = run.commands.join("\n");
        assert!(commands.contains("CLIENT ID"), "{commands}");
        assert!(
            commands.contains(r#""ECHO" "\xff\x00quoted\"value""#),
            "{commands}"
        );
        assert!(commands.contains("UNWATCH"));
        let commands = h.run_ok(&["PING"]).commands.join("\n");
        assert!(commands.matches("\"PING\"").count() >= 2, "{commands}");
    }
}

fn open(uri: &str) -> Harness {
    Harness::open(uri, Settings::default())
}

/// Reads a page of a key, as a key tab does.
fn page(harness: &mut Harness, key: &str, offset: usize, count: usize) -> Run {
    let relation = RelationRef {
        schema: "0".to_owned(),
        relation: key.to_owned(),
    };
    harness.run_ok(&[&value_page(&relation, offset, count)])
}

#[test]
fn each_type_reaches_the_grid_by_its_shape() {
    for uri in uris() {
        let mut h = open(&uri);
        h.run_ok(&[
            "DEL shape:s shape:l shape:h shape:z",
            r#"SET shape:s "a b""#,
            "RPUSH shape:l x y",
            "HSET shape:h f1 v1",
            "ZADD shape:z 1.5 m",
        ]);

        let run = h.run_ok(&["SET shape:s2 v"]);
        assert!(run.columns.is_empty(), "OK has no grid: {run:?}");

        let run = h.run_ok(&["GET shape:s"]);
        assert_eq!(run.columns, [["value"]]);
        assert_eq!(run.grid(), [["a b"]]);

        let run = h.run_ok(&["GET shape:none"]);
        assert_eq!(run.rows[0].get(0).unwrap(), Value::Null);

        let run = h.run_ok(&["LRANGE shape:l 0 -1"]);
        assert_eq!(run.grid(), [["x"], ["y"]]);

        let run = h.run_ok(&["HGETALL shape:h"]);
        assert_eq!(run.columns, [["field", "value"]]);
        assert_eq!(run.grid(), [["f1", "v1"]]);

        let run = h.run_ok(&["ZSCORE shape:z m"]);
        assert_eq!(run.rows[0].get(0).unwrap(), Value::Number("1.5"));

        let run = h.run_ok(&["LLEN shape:l"]);
        assert_eq!(run.rows[0].get(0).unwrap(), Value::Number("2"));
    }
}

#[test]
fn a_server_error_has_its_code() {
    for uri in uris() {
        let run = open(&uri).run(&["NOSUCHCOMMAND a"]);
        assert_eq!(run.end, Some(ExecutionEnd::Failed));
        assert!(run.error().starts_with("ERR unknown command"), "{run:?}");
    }
}

#[test]
fn a_wrong_or_missing_password_asks_for_a_password() {
    for uri in uris() {
        let wrong = uri.replace(":spike@", ":wrong@");
        let error = try_connect(&wrong).err().unwrap();
        assert!(error.password_refused, "{error:?}");
        assert!(!error.message.contains("wrong"), "{error:?}");

        let missing = uri.replace(":spike@", "");
        let error = try_connect(&missing).err().unwrap();
        assert!(error.password_refused, "{error:?}");
    }
}

#[test]
fn a_server_without_a_password_connects() {
    let Ok(uri) = std::env::var("SQLN_TEST_REDIS_OPEN") else {
        println!("skipped: SQLN_TEST_REDIS_OPEN is not set");
        return;
    };
    let run = open(&uri).run_ok(&["PING"]);
    assert_eq!(run.grid(), [["PONG"]]);
}

#[test]
fn a_network_failure_is_not_a_password_refusal() {
    let error = try_connect("redis://127.0.0.1:1/0").err().unwrap();
    assert!(!error.password_refused, "{error:?}");
}

#[test]
fn read_only_runs_only_what_the_server_marks_as_a_read() {
    for uri in uris() {
        open(&uri).run_ok(&["SET ro:k v"]);
        let settings = Settings {
            read_only: true,
            ..Settings::default()
        };
        let mut h = Harness::open(&uri, settings);
        assert_eq!(h.run_ok(&["GET ro:k"]).grid(), [["v"]]);
        for command in [
            "SET ro:k w",
            "DEL ro:k",
            "FLUSHDB",
            "MULTI",
            "EVAL \"return 1\" 0",
            "NOSUCHCOMMAND",
        ] {
            let run = h.run(&[command]);
            assert_eq!(run.end, Some(ExecutionEnd::Failed), "{command}");
            assert!(run.error().contains("read only"), "{command}: {run:?}");
        }
        assert_eq!(page(&mut h, "ro:k", 0, 2).grid(), [["v"]]);
    }
}

#[test]
fn pub_sub_shows_each_message_until_stop() {
    for uri in uris() {
        for (subscribe, publish, kind, pattern) in [
            (
                "SUBSCRIBE live:a",
                "PUBLISH live:a hello",
                "message",
                "NULL",
            ),
            (
                "PSUBSCRIBE live:p*",
                "PUBLISH live:pq hello",
                "pmessage",
                "live:p*",
            ),
            (
                "SSUBSCRIBE live:s",
                "SPUBLISH live:s hello",
                "smessage",
                "NULL",
            ),
        ] {
            let mut h = open(&uri);
            let execution = h.start_live(&[subscribe]);
            let (columns, live) = h.wait_columns();
            assert_eq!(
                columns,
                ["received_at", "kind", "pattern", "channel", "message"]
            );
            assert_eq!(live, Some(Settings::default().row_cap));
            open(&uri).run_ok(&[publish]);
            let row = h.wait_row(|row| row[4] == "hello");
            assert_eq!(row[1], kind, "{subscribe}");
            assert_eq!(row[2], pattern, "{subscribe}");
            let run = h.stop_live(execution);
            assert_eq!(run.end, Some(ExecutionEnd::Stopped), "{subscribe}: {run:?}");
            assert_eq!(run.statuses, [StatementStatus::Success], "{subscribe}");
            assert_eq!(h.run_ok(&["PING"]).grid(), [["PONG"]], "the session stays");
        }
    }
}

#[test]
fn a_live_command_has_no_time_limit() {
    for uri in uris() {
        let settings = Settings {
            sql_timeout: Some(Duration::from_millis(200)),
            ..Settings::default()
        };
        let mut h = Harness::open(&uri, settings);
        let execution = h.start_live(&["SUBSCRIBE live:time"]);
        h.wait_columns();
        std::thread::sleep(Duration::from_millis(600));
        open(&uri).run_ok(&["PUBLISH live:time late"]);
        h.wait_row(|row| row[4] == "late");
        let run = h.stop_live(execution);
        assert_eq!(run.end, Some(ExecutionEnd::Stopped), "{run:?}");
    }
}

#[test]
fn monitor_shows_the_commands_of_other_clients() {
    for uri in uris() {
        let mut h = open(&uri);
        let execution = h.start_live(&["MONITOR"]);
        let (columns, _) = h.wait_columns();
        assert_eq!(columns, ["time", "database", "client", "command"]);
        open(&uri).run_ok(&["SET live:monitor v"]);
        let row = h.wait_row(|row| row[3].contains("live:monitor"));
        assert_eq!(row[3], r#""SET" "live:monitor" "v""#);
        assert_eq!(row[1], "0");
        let run = h.stop_live(execution);
        assert_eq!(run.end, Some(ExecutionEnd::Stopped), "{run:?}");
    }
}

#[test]
fn xread_block_reads_each_new_entry_until_stop() {
    for uri in uris() {
        let mut h = open(&uri);
        h.run_ok(&["DEL live:x", "XADD live:x * old 1"]);
        let execution = h.start_live(&["XREAD BLOCK 200 STREAMS live:x $"]);
        let (columns, _) = h.wait_columns();
        assert_eq!(columns, ["stream", "id", "fields"]);
        // A wait longer than BLOCK makes the read start again.
        std::thread::sleep(Duration::from_millis(500));
        open(&uri).run_ok(&["XADD live:x * f 1"]);
        let first = h.wait_row(|_| true);
        open(&uri).run_ok(&["XADD live:x * f 2"]);
        let second = h.wait_row(|_| true);
        assert_eq!(first[0], "live:x");
        assert!(
            first[2].contains('1') && second[2].contains('2'),
            "{first:?} {second:?}"
        );
        assert_ne!(first[1], second[1]);
        let run = h.stop_live(execution);
        assert_eq!(run.end, Some(ExecutionEnd::Stopped), "{run:?}");
        assert_eq!(h.run_ok(&["PING"]).grid(), [["PONG"]], "the session stays");
    }
}

#[test]
fn xreadgroup_block_keeps_reading_new_entries() {
    for uri in uris() {
        let mut h = open(&uri);
        h.run_ok(&["DEL live:g", "XGROUP CREATE live:g g $ MKSTREAM"]);
        let execution = h.start_live(&["XREADGROUP GROUP g c BLOCK 0 STREAMS live:g >"]);
        h.wait_columns();
        open(&uri).run_ok(&["XADD live:g * f 1"]);
        h.wait_row(|row| row[2].contains('1'));
        open(&uri).run_ok(&["XADD live:g * f 2"]);
        h.wait_row(|row| row[2].contains('2'));
        let run = h.stop_live(execution);
        assert_eq!(run.end, Some(ExecutionEnd::Stopped), "{run:?}");
    }
}

#[test]
fn sync_shows_the_rdb_size_and_then_each_command() {
    for uri in uris() {
        for command in ["SYNC", "PSYNC ? -1"] {
            let mut h = open(&uri);
            let execution = h.start_live(&[command]);
            let (columns, _) = h.wait_columns();
            assert_eq!(columns, ["received_at", "kind", "value"]);
            let rdb = h.wait_row(|row| row[1] == "rdb");
            assert!(rdb[2].parse::<usize>().unwrap() > 0, "{rdb:?}");
            open(&uri).run_ok(&["SET live:sync v"]);
            let row = h.wait_row(|row| row[2].contains("live:sync"));
            assert_eq!(row[1], "command");
            let run = h.stop_live(execution);
            assert_eq!(run.end, Some(ExecutionEnd::Stopped), "{command}: {run:?}");
        }
    }
}

#[test]
fn read_only_accepts_pub_sub_and_refuses_other_live_commands() {
    for uri in uris() {
        let settings = Settings {
            read_only: true,
            ..Settings::default()
        };
        let mut h = Harness::open(&uri, settings);
        let execution = h.start_live(&["SUBSCRIBE live:ro"]);
        h.wait_columns();
        assert_eq!(h.stop_live(execution).end, Some(ExecutionEnd::Stopped));
        for command in [
            "MONITOR",
            "SYNC",
            "XREADGROUP GROUP g c BLOCK 0 STREAMS live:ro >",
        ] {
            let execution = h.start_live(&[command]);
            let run = h.collect(execution);
            assert!(run.error().contains("read only"), "{command}: {run:?}");
        }
    }
}

#[test]
fn a_live_command_must_be_the_last_command_of_a_console_run() {
    // The runner refuses before it asks for a session, so no server is needed.
    let mut h = open("redis://127.0.0.1:1/0");
    let execution = h.start_live(&["SUBSCRIBE c", "PING"]);
    let run = h.collect(execution);
    assert_eq!(run.end, Some(ExecutionEnd::Refused), "{run:?}");
    assert!(run.message.as_deref().unwrap().contains("last"), "{run:?}");

    for command in ["SUBSCRIBE c", "MONITOR", "XREAD BLOCK 0 STREAMS k $"] {
        let run = h.run(&[command]);
        assert_eq!(run.end, Some(ExecutionEnd::Refused), "{command}: {run:?}");
        assert!(
            run.message.as_deref().unwrap().contains("console"),
            "{run:?}"
        );
    }
}

#[test]
fn a_shared_session_keeps_its_login() {
    for uri in uris() {
        let mut h = open(&uri);
        for command in ["AUTH x", "HELLO 2", "RESET"] {
            let run = h.run(&[command]);
            assert!(run.error().contains("login"), "{command}: {run:?}");
        }
    }
}

#[test]
fn a_command_without_an_answer_is_refused() {
    for uri in uris() {
        let mut h = open(&uri);
        for command in ["CLIENT REPLY OFF", "client reply skip"] {
            let run = h.run(&[command]);
            assert!(run.error().contains("answers"), "{command}: {run:?}");
        }
        assert_eq!(h.run_ok(&["PING"]).grid(), [["PONG"]]);
    }
}

#[test]
fn the_cleanup_removes_the_client_state_between_executions() {
    for uri in uris() {
        let settings = Settings {
            max_connections: 1,
            ..Settings::default()
        };
        let mut h = Harness::open(&uri, settings);
        let first = h.run_ok(&[
            "CLIENT ID",
            "CLIENT SETNAME dirty",
            "CLIENT TRACKING ON",
            "CLIENT NO-EVICT ON",
            "CLIENT NO-TOUCH ON",
        ]);
        let second = h.run_ok(&["CLIENT ID", "CLIENT INFO"]);
        let grid = second.grid();
        assert_eq!(first.grid()[0], grid[0], "the same session: {uri}");
        let info = &grid[1][0];
        let field = |name: &str| {
            info.split_whitespace()
                .find_map(|pair| pair.strip_prefix(name))
                .unwrap_or_else(|| panic!("no {name}: {info}"))
                .to_owned()
        };
        assert_eq!(field("name="), "", "{info}");
        assert!(!field("flags=").contains(['t', 'e', 'T']), "{info}");
    }
}

#[test]
fn stop_ends_a_blocking_command_and_the_session_stays() {
    for uri in uris() {
        let mut h = open(&uri);
        let execution = h.start(&["BLPOP stop:none 0"]);
        h.wait_started();
        std::thread::sleep(Duration::from_millis(100));
        h.manager.stop(h.console);
        let run = h.collect(execution);
        assert_eq!(run.end, Some(ExecutionEnd::Stopped), "{run:?}");
        assert_eq!(h.run_ok(&["PING"]).grid(), [["PONG"]]);
    }
}

#[test]
fn the_time_limit_ends_a_blocking_command() {
    for uri in uris() {
        let settings = Settings {
            sql_timeout: Some(Duration::from_millis(200)),
            ..Settings::default()
        };
        let mut h = Harness::open(&uri, settings);
        let run = h.run(&["BLPOP limit:none 0"]);
        assert_eq!(run.end, Some(ExecutionEnd::TimedOut), "{run:?}");
    }
}

#[test]
fn a_script_runs_multi_and_exec_on_one_session() {
    for uri in uris() {
        let mut h = open(&uri);
        h.run_ok(&["DEL multi:a"]);
        let run = h.run_ok(&["MULTI", "INCR multi:a", "INCR multi:a", "EXEC"]);
        assert_eq!(run.grid().last().unwrap(), &["2"]);
    }
}

#[test]
fn a_multi_that_stays_open_ends_with_the_execution() {
    for uri in uris() {
        let mut h = open(&uri);
        h.run_ok(&["DEL open:a"]);
        h.run_ok(&["MULTI", "SET open:a v"]);
        let run = h.run_ok(&["GET open:a"]);
        assert_eq!(run.rows[0].get(0).unwrap(), Value::Null, "{run:?}");
    }
}

#[test]
fn the_session_returns_to_its_database() {
    for uri in uris() {
        let mut h = open(&uri);
        h.run_ok(&["DEL back:a"]);
        h.run_ok(&["SELECT 5", "SET back:a v", "EXPIRE back:a 60"]);
        let run = h.run_ok(&["GET back:a"]);
        assert_eq!(run.rows[0].get(0).unwrap(), Value::Null, "{run:?}");
    }
}

#[test]
fn a_console_can_choose_a_database() {
    for uri in uris() {
        let mut h = open(&uri);
        let options = |schema: &str| RunOptions {
            schema: Some(schema.to_owned()),
            ..RunOptions::default()
        };
        let set = vec!["SET chosen:a v".to_owned(), "EXPIRE chosen:a 60".to_owned()];
        let execution = h.manager.run(h.console, set, options("6"));
        h.collect(execution);
        let get = vec!["GET chosen:a".to_owned()];
        let execution = h.manager.run(h.console, get, options("6"));
        assert_eq!(h.collect(execution).grid(), [["v"]]);
    }
}

#[test]
fn a_key_page_reads_each_type() {
    for uri in uris() {
        let mut h = open(&uri);
        h.run_ok(&[
            "DEL page:s page:l page:h page:t page:z page:x",
            "SET page:s text",
            "RPUSH page:l a b c d e",
            "HSET page:h f1 v1 f2 v2 f3 v3",
            "SADD page:t m1 m2 m3",
            "ZADD page:z 1 one 2 two 3 three",
            "XADD page:x 1-1 a 1",
            "XADD page:x 2-1 b 2",
        ]);

        assert_eq!(page(&mut h, "page:s", 0, 3).grid(), [["text"]]);
        assert!(page(&mut h, "page:s", 3, 3).rows.is_empty());

        assert_eq!(page(&mut h, "page:l", 0, 2).grid(), [["a"], ["b"]]);
        assert_eq!(page(&mut h, "page:l", 2, 2).grid(), [["c"], ["d"]]);
        assert_eq!(page(&mut h, "page:l", 4, 2).grid(), [["e"]]);

        let first = page(&mut h, "page:h", 0, 2);
        let second = page(&mut h, "page:h", 2, 2);
        assert_eq!(first.columns, [["field", "value"]]);
        assert_eq!(first.rows.len(), 2);
        assert_eq!(second.rows.len(), 1);
        let mut all: Vec<_> = first.grid().into_iter().chain(second.grid()).collect();
        all.sort();
        assert_eq!(all, [["f1", "v1"], ["f2", "v2"], ["f3", "v3"]]);

        let run = page(&mut h, "page:t", 0, 10);
        assert_eq!(run.columns, [["value"]]);
        assert_eq!(run.rows.len(), 3);

        let run = page(&mut h, "page:z", 1, 2);
        assert_eq!(run.columns, [["member", "score"]]);
        assert_eq!(run.grid(), [["two", "2"], ["three", "3"]]);

        let run = page(&mut h, "page:x", 1, 2);
        assert_eq!(run.columns, [["id", "fields"]]);
        assert_eq!(run.grid(), [["2-1", "[b, 2]"]]);
    }
}

#[test]
fn a_key_page_reports_a_missing_key() {
    for uri in uris() {
        let mut h = open(&uri);
        h.run_ok(&["DEL page:gone"]);
        let relation = RelationRef {
            schema: "0".to_owned(),
            relation: "page:gone".to_owned(),
        };
        let run = h.run(&[&value_page(&relation, 0, 2)]);
        assert!(run.error().contains("does not exist"), "{run:?}");
    }
}

#[test]
fn the_catalog_lists_the_databases_and_their_keys() {
    for uri in uris() {
        let mut h = open(&uri);
        h.run_ok(&[
            "SELECT 8",
            "FLUSHDB",
            "SET catalog:a 1",
            "SET catalog:b 2",
            "SET \"\\xff\" 3",
        ]);
        let connector = network_connector(parse_uri(&uri).unwrap());
        assert_eq!(connector.capabilities().namespace, Namespace::Database);
        assert!(!connector.capabilities().transactions);
        let catalog = runtime().block_on(async {
            let mut session = connector.connect().await.unwrap();
            session.read_catalog().await.unwrap()
        });
        assert_eq!(catalog.current_schema.as_deref(), Some("0"));
        assert!(catalog.schema("0").is_some());
        let schema = catalog.schema("8").unwrap();
        let names: Vec<&str> = schema
            .relations
            .iter()
            .map(|key| key.name.as_str())
            .collect();
        assert_eq!(names, ["catalog:a", "catalog:b"]);
        assert!(
            schema.partial,
            "a key that is not UTF-8 stays out of the list"
        );
    }
}

#[test]
fn a_list_reads_its_databases_and_the_current_one_and_the_names_need_no_keys() {
    for uri in uris() {
        let mut h = open(&uri);
        h.run_ok(&[
            "SELECT 12",
            "FLUSHDB",
            "SELECT 11",
            "FLUSHDB",
            "SET listed:a 1",
        ]);
        let connector = network_connector(parse_uri(&uri).unwrap());
        let (catalog, names) = runtime().block_on(async {
            let mut session = connector.connect().await.unwrap();
            let catalog = session.read_catalog_of(&["12".to_owned()]).await.unwrap();
            let names = session.read_schema_names().await.unwrap();
            (catalog, names)
        });
        let found: Vec<&str> = catalog
            .schemas
            .iter()
            .map(|schema| schema.name.as_str())
            .collect();
        // A database of the list appears also when it is empty.
        assert_eq!(found, ["0", "12"]);
        assert!(names.contains(&"0".to_owned()), "{names:?}");
        assert!(names.contains(&"11".to_owned()), "{names:?}");
        assert!(!names.contains(&"12".to_owned()), "{names:?}");
        h.run_ok(&["SELECT 11", "FLUSHDB"]);
    }
}

#[test]
fn a_large_database_gives_a_partial_list() {
    for uri in uris() {
        let mut h = open(&uri);
        let keys: Vec<String> = (0..MAX_KEYS + 50).map(|key| format!("k{key} v")).collect();
        let script = format!("MSET {}", keys.join(" "));
        h.run_ok(&["SELECT 9", "FLUSHDB", &script]);
        let connector = network_connector(parse_uri(&uri).unwrap());
        let catalog = runtime().block_on(async {
            let mut session = connector.connect().await.unwrap();
            session.read_catalog().await.unwrap()
        });
        let schema = catalog.schema("9").unwrap();
        assert_eq!(schema.relations.len(), MAX_KEYS);
        assert!(schema.partial);
        h.run_ok(&["SELECT 9", "FLUSHDB"]);
    }
}

#[test]
fn the_session_stays_in_its_database_after_the_catalog() {
    for uri in uris() {
        let connector = network_connector(parse_uri(&uri).unwrap());
        let keys = runtime().block_on(async {
            let mut session = connector.connect().await.unwrap();
            session.read_catalog().await.unwrap();
            session.read_columns("0", "any").await.unwrap()
        });
        assert!(keys.is_empty());
    }
}

struct Screen {
    app: App,
}

impl Screen {
    fn open(uri: &str) -> Self {
        let target = parse_uri(uri).unwrap();
        let definition = Definition::new("cache", Source::Network(target));
        let options = Options {
            keyboard: KeyboardMode::Conventional,
            connections: vec![definition],
            ..Options::default()
        };
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        Self {
            app: App::new(options, clipboard, Theme::plain()),
        }
    }

    fn screen(&mut self) -> String {
        self.screen_at(100, 30)
    }

    fn screen_at(&mut self, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for row in buffer.content.chunks(usize::from(buffer.area.width)) {
            text.extend(row.iter().map(|cell| cell.symbol()));
            text.push('\n');
        }
        text
    }

    fn wait_for(&mut self, wanted: &str) {
        self.wait_for_at(100, 30, wanted);
    }

    fn wait_for_at(&mut self, width: u16, height: u16, wanted: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.app.drain_events();
            let screen = self.screen_at(width, height);
            if screen.contains(wanted) {
                return;
            }
            assert!(Instant::now() < deadline, "{wanted} is not here:\n{screen}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// Runs a command in the console of the screen, and waits for its end.
fn run_on_screen(screen: &mut Screen, command: &str) {
    let app = &mut screen.app;
    app.on_event(Event::Key(ctrl('a')));
    app.on_event(Event::Paste(command.to_owned()));
    app.on_event(Event::Key(common::key(KeyCode::F(5))));
    let deadline = Instant::now() + Duration::from_secs(20);
    app.drain_events();
    while app.console().is_busy() {
        app.drain_events();
        assert!(Instant::now() < deadline, "the run did not end");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn catalog_names(app: &App) -> Vec<String> {
    let connection = &app.connections()[0];
    let catalog = connection.catalog.as_ref().expect("a catalog");
    catalog
        .schemas
        .iter()
        .map(|schema| schema.name.clone())
        .collect()
}

fn wait_for_catalog(app: &mut App) {
    use sqln::connection::CatalogState;
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.connections()[0].catalog_state != CatalogState::Loaded {
        app.drain_events();
        assert!(Instant::now() < deadline, "the catalog did not arrive");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn the_form_of_an_open_connection_marks_a_database_and_the_tree_follows() {
    use sqln::navigator::{RowKind, RowNote};
    for uri in uris() {
        let mut h = open(&uri);
        h.run_ok(&[
            "SELECT 13",
            "FLUSHDB",
            "SET marked:a 1",
            "EXPIRE marked:a 120",
        ]);
        let mut screen = Screen::open(&uri);
        run_on_screen(&mut screen, "PING");
        let app = &mut screen.app;
        common::select_row(app, 0, &[RowKind::Connection], "cache");
        app.on_event(Event::Key(common::key(KeyCode::Char('e'))));
        assert!(app.form().unwrap().display_only);
        common::focus_form_field(app, Field::Schemas);
        app.on_event(Event::Key(ctrl('o')));
        let deadline = Instant::now() + Duration::from_secs(20);
        while app.form().unwrap().test.is_some() {
            app.drain_events();
            assert!(Instant::now() < deadline, "the names did not arrive");
            std::thread::sleep(Duration::from_millis(1));
        }
        let open = app.form().unwrap().schema_list.as_ref().expect("the list");
        assert!(open.list.list().items().contains(&"13".to_owned()));
        for code in [KeyCode::Char('1'), KeyCode::Char('3'), KeyCode::Char(' ')] {
            app.on_event(Event::Key(common::key(code)));
        }
        app.on_event(Event::Key(common::key(KeyCode::Enter)));
        assert_eq!(app.form().unwrap().text(Field::Schemas), "13");
        common::focus_form_field(app, Field::Save);
        app.on_event(Event::Key(common::key(KeyCode::Enter)));
        assert!(app.form().is_none());
        let definition = app.connections()[0].definition.as_ref().unwrap();
        assert_eq!(definition.schemas, ["13"]);
        wait_for_catalog(app);
        assert_eq!(catalog_names(app), ["0", "13"]);
        let rows = app.navigator().rows(app.connections());
        let row = rows
            .iter()
            .find(|row| row.kind == RowKind::Connection)
            .unwrap();
        assert_eq!(
            app.navigator().suffix(row, app.connections()),
            Some(RowNote::Filtered)
        );
        h.run_ok(&["SELECT 13", "FLUSHDB"]);
    }
}

#[test]
fn a_live_result_shows_its_state_and_rows_on_a_small_screen() {
    for uri in uris() {
        let mut screen = Screen::open(&uri);
        let app = &mut screen.app;
        app.on_event(Event::Key(ctrl('a')));
        app.on_event(Event::Paste("SUBSCRIBE live:ui".to_owned()));
        app.on_event(Event::Key(common::key(KeyCode::F(5))));
        screen.wait_for_at(80, 24, "0 rows received. Live.");
        let shown = screen.screen_at(80, 24);
        let status = shown.lines().rev().nth(1).unwrap();
        assert!(status.contains("Live"), "{shown}");

        open(&uri).run_ok(&["PUBLISH live:ui shown-row"]);
        screen.wait_for_at(80, 24, "1 row received. Live.");
        let shown = screen.screen_at(80, 24);
        assert!(shown.contains("│ message │ NULL"), "{shown}");

        let app = &mut screen.app;
        app.on_event(Event::Key(ctrl('w')));
        app.on_event(Event::Key(common::key(KeyCode::Char('x'))));
        let deadline = Instant::now() + Duration::from_secs(20);
        while app.console().is_busy() {
            app.drain_events();
            assert!(Instant::now() < deadline, "the stream did not stop");
            std::thread::sleep(Duration::from_millis(1));
        }
        let shown = screen.screen_at(80, 24);
        assert!(shown.contains("1 row received."), "{shown}");
        assert!(!shown.contains("Live"), "{shown}");
        assert!(shown.contains("│ message │ NULL"), "the rows stay: {shown}");
    }
}

#[test]
fn a_key_opens_in_a_tab_without_sql_fields() {
    for uri in uris() {
        let mut h = open(&uri);
        h.run_ok(&[
            "SELECT 1",
            "FLUSHDB",
            "RPUSH tab:list first second",
            "SET \"\\xff\" hidden",
        ]);

        let mut screen = Screen::open(&uri.replace("/0", "/1"));
        common::open_table(&mut screen.app, 0, "tab:list");
        screen.wait_for("second");
        let shown = screen.screen();
        assert!(shown.contains("tab:list (key)"), "{shown}");
        assert!(shown.contains("Reload"), "{shown}");
        assert!(shown.contains("(partial list)"), "{shown}");
        for absent in ["WHERE", "ORDER BY", "Clear"] {
            assert!(!shown.contains(absent), "{absent}:\n{shown}");
        }

        // A click or a key cannot move the focus to a field that is not on the screen.
        screen.app.on_event(Event::Key(common::key(KeyCode::Up)));
        assert_eq!(screen.app.table().unwrap().focus, TableFocus::Grid);
        h.run_ok(&["SELECT 1", "FLUSHDB"]);
    }
}

#[test]
fn a_console_runs_commands_and_ignores_the_manual_mode() {
    for uri in uris() {
        let mut screen = Screen::open(&uri);
        assert_eq!(screen.app.console().editor.dialect(), Dialect::Redis);

        screen.app.on_event(Event::Key(ctrl('w')));
        screen
            .app
            .on_event(Event::Key(common::key(KeyCode::Char('m'))));
        assert!(screen.app.console().manual);

        for c in "ECHO hello-console".chars() {
            let event = common::key(KeyCode::Char(c));
            screen.app.on_event(Event::Key(event));
        }
        screen.app.on_event(Event::Key(ctrl('w')));
        screen
            .app
            .on_event(Event::Key(common::key(KeyCode::Char('e'))));
        screen.wait_for("hello-console");
        screen.wait_for("1 row");
    }
}

#[test]
fn tls_verifies_the_server() {
    let (Ok(port), Ok(folder)) = (
        std::env::var("SQLN_REDIS_TLS_PORT"),
        std::env::var("SQLN_CERTS"),
    ) else {
        println!("skipped: SQLN_REDIS_TLS_PORT is not set");
        return;
    };
    if std::env::var("SQLN_TEST_TLS").is_err() {
        println!("skipped: SQLN_TEST_TLS is not set");
        return;
    }
    let good = format!("{folder}/ca.crt");
    let other = format!("{folder}/other-ca.crt");
    let text = |host: &str, ca: &str| format!("rediss://:spike@{host}:{port}/0?sslrootcert={ca}");

    assert!(try_connect(&text("localhost", &good)).is_ok());

    let error = try_connect(&text("localhost", &other)).err().unwrap();
    assert!(!error.password_refused, "{error:?}");
    let error = try_connect(&text("127.0.0.1", &good)).err().unwrap();
    assert!(error.message.contains("certificate"), "{error:?}");

    let client = format!(
        "{}&sslcert={folder}/client.crt&sslkey={folder}/client.pk8.key",
        text("localhost", &good)
    );
    assert!(try_connect(&client).is_ok());

    let plain = format!("redis://:spike@localhost:{port}/0");
    assert!(try_connect(&plain).is_err());
}
