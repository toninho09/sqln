//! Named parameters, native bindings, and the panel through application input events.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{ctrl, key, temp_db};
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::connector::sqlite::{SqliteConnector, SqliteTarget};
use sqln::connector::target::parse_uri;
use sqln::connector::{Connector, network_connector};
use sqln::console::Focus;
use sqln::editor::KeyboardMode;
use sqln::exec::{ExecEvent, ExecutionEnd, Manager, RunOptions, Settings, StatementStatus};
use sqln::grid::{Budget, Row, Value};
use sqln::parameters::Panel;
use sqln::sql::Dialect;
use sqln::sql::parameters::{Kind, Parameter, bind, names};
use sqln::ui::theme::Theme;

fn parameter(name: &str, kind: Kind, value: &str) -> Parameter {
    Parameter {
        name: name.into(),
        kind,
        value: value.into(),
    }
}

#[test]
fn detection_ignores_quoted_text_comments_casts_and_native_syntax() {
    let sql = "SELECT :id, :ID, :id::int, ':text', E':escape', \"x:param\", $$:body$$, $tag$:body$tag$, $1, array[1:upper] -- :comment\n/* :comment */ WHERE a=:other";
    assert_eq!(names(sql, Dialect::Postgres), ["id", "ID", "other"]);
    assert_eq!(
        names(
            "SELECT :value, `a:no`, ':no' # :no\n-- :no\n",
            Dialect::MySql
        ),
        ["value"]
    );
    assert_eq!(
        names("SELECT :value, [a:no], ':no'", Dialect::Sqlite),
        ["value"]
    );
    assert!(names("GET :key", Dialect::Redis).is_empty());
    assert_eq!(
        names("SELECT :_a, :a2, :2a, :ação", Dialect::Postgres),
        ["_a", "a2"]
    );
}

#[test]
fn values_stay_outside_sql_and_error_offsets_refer_to_original_text() {
    let sql = "SELECT :customer, :customer::text, missing";
    let value = "'; DROP TABLE orders; -- 雪\\";
    let bound = bind(
        sql,
        Dialect::Postgres,
        &[parameter("customer", Kind::Text, value)],
    )
    .unwrap();
    assert_eq!(bound.sql, "SELECT $1, $2::text, missing");
    assert_eq!(bound.parameters.len(), 2);
    assert!(
        bound
            .parameters
            .iter()
            .all(|parameter| parameter.value == value)
    );
    assert_eq!(
        bound.source_offset(bound.sql.find("missing").unwrap()),
        sql.find("missing").unwrap()
    );
    assert_eq!(
        bound.source_offset(bound.sql.find("$2").unwrap() + 1),
        sql.rfind(":customer").unwrap()
    );
    assert!(!format!("{:?}", bound.parameters).contains(value));
}

#[test]
fn number_binding_keeps_decimal_digits_without_float_conversion() {
    let value = "123456789012345678901234567890.12345678901234567890";
    for dialect in [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite] {
        let bound = bind("SELECT :n", dialect, &[parameter("n", Kind::Number, value)]).unwrap();
        assert_eq!(bound.parameters[0].value, value);
        assert!(!bound.sql.contains(value));
    }
    let bound = bind(
        "SELECT :n",
        Dialect::MySql,
        &[parameter("n", Kind::Number, "1.25e2")],
    )
    .unwrap();
    assert_eq!(bound.parameters[0].value, "125");
    assert!(
        bind(
            "SELECT :n",
            Dialect::MySql,
            &[parameter("n", Kind::Number, "1e100")]
        )
        .is_err()
    );
    for value in [
        "1; DELETE FROM t",
        "NaN",
        "01",
        " 1",
        "1 ",
        "true",
        "null",
        "",
    ] {
        assert!(
            bind(
                "SELECT :n",
                Dialect::Postgres,
                &[parameter("n", Kind::Number, value)]
            )
            .is_err(),
            "{value:?}"
        );
    }
}

#[test]
fn native_markers_cannot_share_named_bindings_but_json_operators_can() {
    let values = [parameter("a", Kind::Text, "x")];
    assert!(bind("SELECT :a, $1", Dialect::Postgres, &values).is_err());
    assert!(bind("SELECT :a, ?", Dialect::MySql, &values).is_err());
    assert!(bind("SELECT :a, @b", Dialect::Sqlite, &values).is_err());
    assert!(bind("SELECT :a, '{}'::jsonb ? 'key'", Dialect::Postgres, &values).is_ok());
}

#[test]
fn panel_preserves_values_by_name_and_validates_only_the_execution_scope() {
    let mut panel = Panel::default();
    panel.update_names(vec!["a".into(), "b".into()]);
    panel.fields[0].input.set("hello");
    panel.fields[0].set = true;
    panel.update_names(vec!["b".into(), "a".into(), "c".into()]);
    assert_eq!(panel.fields[1].input.text(), "hello");
    let values = panel
        .values(&["SELECT :a, :a".into()], Dialect::Sqlite)
        .unwrap();
    assert_eq!(values, [parameter("a", Kind::Text, "hello")]);
    assert!(
        panel
            .values(&["SELECT :b".into()], Dialect::Sqlite)
            .is_err()
    );
    assert!(panel.fields[0].invalid);
    panel.column = 1;
    panel.on_key(key(KeyCode::Enter));
    assert_eq!(
        panel
            .values(&["SELECT :b".into()], Dialect::Sqlite)
            .unwrap()[0]
            .value,
        ""
    );
    panel.update_names(vec!["b".into()]);
    panel.update_names(vec!["a".into()]);
    assert!(!panel.fields[0].set);
}

#[test]
fn asynchronous_detection_rejects_old_revisions() {
    let mut panel = Panel::default();
    let at = Instant::now();
    panel.poll(
        1,
        Dialect::Sqlite,
        at,
        || "SELECT :old".into(),
        |work| {
            std::thread::spawn(work);
        },
    );
    panel.poll(
        1,
        Dialect::Sqlite,
        at + Duration::from_millis(201),
        || "SELECT :old".into(),
        |work| {
            std::thread::spawn(work);
        },
    );
    panel.poll(
        2,
        Dialect::Sqlite,
        at + Duration::from_millis(202),
        || "SELECT :new".into(),
        |work| {
            std::thread::spawn(work);
        },
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while panel.fields.is_empty() {
        panel.poll(
            2,
            Dialect::Sqlite,
            at + Duration::from_secs(1),
            || "SELECT :new".into(),
            |work| {
                std::thread::spawn(work);
            },
        );
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(panel.fields[0].name, "new");
}

struct Screen {
    app: App,
}
impl Screen {
    fn new(name: &str, keyboard: KeyboardMode) -> Self {
        let path = temp_db(name);
        rusqlite::Connection::open(&path).unwrap();
        Self {
            app: App::new(
                Options {
                    database: Some(path),
                    keyboard,
                    ..Options::default()
                },
                Rc::new(RefCell::new(MemoryClipboard::default())),
                Theme::plain(),
            ),
        }
    }
    fn key(&mut self, code: KeyCode) {
        self.app.on_event(Event::Key(key(code)));
    }
    fn prefix(&mut self, c: char) {
        self.app.on_event(Event::Key(ctrl('w')));
        self.key(KeyCode::Char(c));
    }
    fn paste(&mut self, text: &str) {
        self.app.on_event(Event::Paste(text.into()));
    }
    fn wait(&mut self, test: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !test(&self.app) {
            self.app.drain_events();
            assert!(
                Instant::now() < deadline,
                "the expected application state did not appear"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn screen(&mut self, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .chunks(width as usize)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[test]
fn panel_detects_without_running_and_keeps_editor_and_value_input_separate() {
    for keyboard in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut screen = Screen::new(&format!("parameter-ui-{keyboard:?}"), keyboard);
        if keyboard == KeyboardMode::Vim {
            screen.key(KeyCode::Char('i'));
        }
        let sql = "SELECT :customer, :customer;";
        screen.paste(sql);
        screen.wait(|app| app.console().parameters.fields.len() == 1);
        assert!(screen.app.console().results.is_empty());
        assert!(!screen.app.console().is_busy());
        assert!(screen.screen(80, 24).contains("Parameters (1)"));
        screen.prefix('P');
        assert_eq!(screen.app.console().focus, Focus::Parameters);
        screen.key(KeyCode::Tab);
        screen.paste("ação 雪 '; DROP TABLE x; --");
        assert_eq!(screen.app.console().editor.text(), sql);
        screen.key(KeyCode::F(5));
        screen.wait(|app| !app.console().is_busy() && !app.console().results.is_empty());
        assert_eq!(
            screen.app.console().results[0].table.rows[0].get(0),
            Some(Value::Text("ação 雪 '; DROP TABLE x; --"))
        );
        assert_eq!(
            screen.app.console().results[0].table.rows[0].get(1),
            Some(Value::Text("ação 雪 '; DROP TABLE x; --"))
        );
        assert_eq!(screen.app.console().editor.text(), sql);
        screen.key(KeyCode::Esc);
        assert_eq!(screen.app.console().focus, Focus::Editor);
    }
}

#[test]
fn an_unset_value_prevents_execution_and_empty_text_requires_enter() {
    let mut screen = Screen::new("parameters-unset", KeyboardMode::Conventional);
    screen.paste("SELECT :value;");
    screen.key(KeyCode::F(5));
    assert_eq!(screen.app.console().focus, Focus::Parameters);
    assert!(!screen.app.console().is_busy());
    assert!(screen.app.console().results.is_empty());
    assert!(screen.screen(80, 24).contains("Unset"));
    screen.key(KeyCode::Enter);
    screen.key(KeyCode::F(5));
    screen.wait(|app| !app.console().is_busy() && !app.console().results.is_empty());
    assert_eq!(
        screen.app.console().results[0].table.rows[0].get(0),
        Some(Value::Text(""))
    );
}

#[test]
fn panel_scroll_resize_and_mouse_keep_fields_accessible() {
    let mut screen = Screen::new("parameters-scroll", KeyboardMode::Conventional);
    screen.paste("SELECT :a, :b, :c, :d, :e, :f, :g;");
    screen.wait(|app| app.console().parameters.fields.len() == 7);
    screen.prefix('P');
    for _ in 0..6 {
        screen.key(KeyCode::Down);
    }
    let shown = screen.screen(80, 24);
    assert!(shown.contains(":g"), "{shown}");
    screen.screen(40, 12);
    let shown = screen.screen(120, 40);
    let (y, row) = shown
        .lines()
        .enumerate()
        .find(|(_, row)| row.contains(":g") && row.contains("[Text]"))
        .unwrap();
    let x = unicode_width::UnicodeWidthStr::width(&row[..row.find("[Text]").unwrap()]) + 9;
    screen.app.on_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x as u16,
        row: y as u16,
        modifiers: KeyModifiers::NONE,
    }));
    screen.paste("最後");
    assert_eq!(
        screen.app.console().parameters.fields[6].input.text(),
        "最後"
    );
    let (y, row) = shown
        .lines()
        .enumerate()
        .find(|(_, row)| row.contains("Parameters (7)"))
        .unwrap();
    let x = unicode_width::UnicodeWidthStr::width(&row[..row.find("[-]").unwrap()]);
    screen.app.on_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x as u16,
        row: y as u16,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(screen.app.console().parameters.collapsed);
    assert_eq!(screen.app.console().focus, Focus::Editor);
    screen.prefix('P');
    assert!(!screen.app.console().parameters.collapsed);
    assert!(screen.screen(80, 24).contains(":g"));
}

struct Harness {
    manager: Manager,
    events: tokio::sync::mpsc::Receiver<ExecEvent>,
    console: u64,
    budget: Arc<Budget>,
}
#[derive(Default, Debug)]
struct Run {
    rows: Vec<Row>,
    errors: Vec<String>,
    end: Option<ExecutionEnd>,
    offsets: Vec<Option<usize>>,
    commands: Vec<String>,
}
impl Harness {
    fn new(connector: Arc<dyn Connector>, settings: Settings) -> Self {
        let budget = Arc::new(Budget::new(settings.budget_bytes));
        let (mut manager, events) = Manager::new(budget.clone()).unwrap();
        let connection = manager.add_connection(connector, settings);
        let console = manager.open_console(connection).unwrap();
        manager.set_console_history(console, 1, "Parameters test".to_owned());
        Self {
            manager,
            events,
            console,
            budget,
        }
    }
    fn run(&mut self, sql: &[&str], parameters: Vec<Parameter>, manual: bool) -> Run {
        let execution = self.manager.run(
            self.console,
            sql.iter().map(|sql| (*sql).into()).collect(),
            RunOptions {
                parameters,
                manual,
                ..RunOptions::default()
            },
        );
        self.collect(execution)
    }
    fn collect(&mut self, execution: u64) -> Run {
        let mut result = Run::default();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match self.events.try_recv() {
                Ok(ExecEvent::Rows { rows, bytes, .. }) => {
                    self.budget.release(bytes);
                    result.rows.extend(rows);
                }
                Ok(ExecEvent::StatementFinished {
                    status,
                    error_offset,
                    ..
                }) => {
                    if let StatementStatus::Error(error) = status {
                        result.errors.push(error);
                    }
                    result.offsets.push(error_offset);
                }
                Ok(ExecEvent::CommandLog {
                    event: sqln::command_log::LogEvent::Started { text, .. },
                    ..
                }) => result.commands.push(text),
                Ok(ExecEvent::ExecutionFinished {
                    execution: id, end, ..
                }) if id == execution => {
                    result.end = Some(end);
                    return result;
                }
                _ => {
                    assert!(
                        Instant::now() < deadline,
                        "execution did not finish: {result:?}"
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }
}

fn native_sources(name: &str) -> Vec<(Dialect, Arc<dyn Connector>)> {
    let mut target = SqliteTarget::new(temp_db(name));
    target.allow_create = true;
    let mut sources: Vec<(Dialect, Arc<dyn Connector>)> =
        vec![(Dialect::Sqlite, Arc::new(SqliteConnector::new(target)))];
    for (name, dialect) in [
        ("SQLN_TEST_POSTGRES", Dialect::Postgres),
        ("SQLN_TEST_MYSQL", Dialect::MySql),
    ] {
        if let Ok(uri) = std::env::var(name) {
            sources.push((dialect, network_connector(parse_uri(&uri).unwrap())));
        } else {
            println!("skipped: {name} is not set");
        }
    }
    sources
}

#[test]
fn native_bindings_preserve_values_and_block_sql_injection() {
    for (dialect, source) in native_sources("parameters-native") {
        let mut h = Harness::new(source, Settings::default());
        let values = vec![
            parameter("text", Kind::Text, "'; SELECT 999; -- \\ 雪"),
            parameter("number", Kind::Number, "1234567890123456789"),
            parameter("flag", Kind::Boolean, "false"),
            parameter("none", Kind::Null, ""),
            parameter("empty", Kind::Text, ""),
        ];
        let run = h.run(
            &["SELECT :text AS a, :number AS n, :flag AS b, :none AS c, :empty AS d, :text AS e"],
            values,
            false,
        );
        assert_eq!(
            run.end,
            Some(ExecutionEnd::Completed),
            "{dialect:?}: {run:?}"
        );
        assert_eq!(run.rows.len(), 1);
        assert_eq!(
            run.rows[0].get(0),
            Some(Value::Text("'; SELECT 999; -- \\ 雪"))
        );
        assert_eq!(
            run.rows[0].get(1),
            Some(Value::Number("1234567890123456789"))
        );
        assert_eq!(run.rows[0].get(3), Some(Value::Null));
        assert_eq!(run.rows[0].get(4), Some(Value::Text("")));
        assert_eq!(run.rows[0].get(5), run.rows[0].get(0));
        assert!(
            run.commands
                .iter()
                .any(|command| command.contains("Parameters:") && command.contains("(:text)"))
        );
        assert_eq!(
            h.run(&["SELECT 1"], Vec::new(), false).end,
            Some(ExecutionEnd::Completed)
        );
    }
}

#[test]
fn native_bindings_keep_server_decimal_text_and_unknown_postgres_types() {
    for (dialect, source) in native_sources("parameters-decimal") {
        if dialect == Dialect::Sqlite {
            continue;
        }
        let mut h = Harness::new(source, Settings::default());
        let number = "123456789012345678901234567890.12345678901234567890";
        let sql = if dialect == Dialect::Postgres {
            "SELECT :number, '[1,4)'::int4range, '2001-02-03'::date, '\\x00ff'::bytea, 1.5::double precision, NULL::uuid"
        } else {
            "SELECT :number, CAST('2001-02-03' AS DATE), X'00ff', CAST(1.5 AS DOUBLE)"
        };
        let run = h.run(
            &[sql],
            vec![parameter("number", Kind::Number, number)],
            false,
        );
        assert_eq!(
            run.end,
            Some(ExecutionEnd::Completed),
            "{dialect:?}: {run:?}"
        );
        assert_eq!(run.rows[0].get(0), Some(Value::Number(number)));
        if dialect == Dialect::Postgres {
            assert_eq!(run.rows[0].get(1), Some(Value::Text("[1,4)")));
            assert_eq!(run.rows[0].get(2), Some(Value::Text("2001-02-03")));
            assert_eq!(run.rows[0].get(3), Some(Value::binary(&[0, 255])));
            assert_eq!(run.rows[0].get(5), Some(Value::Null));
        }
        if dialect == Dialect::Postgres {
            let uuid = "0f4a2d81-5d3f-4c0b-9d6f-1a2b3c4d5e6f";
            let typed = h.run(
                &["SELECT :id::uuid = :id, :day::date, :stamp::timestamptz, (:doc::jsonb ? 'k')"],
                vec![
                    parameter("id", Kind::Text, uuid),
                    parameter("day", Kind::Text, "2001-02-03"),
                    parameter("stamp", Kind::Text, "2001-02-03 04:05:06+00"),
                    parameter("doc", Kind::Text, "{\"k\": 1}"),
                ],
                false,
            );
            assert_eq!(typed.end, Some(ExecutionEnd::Completed), "{typed:?}");
            assert_eq!(typed.rows[0].get(0), Some(Value::Text("t")));
            assert_eq!(typed.rows[0].get(1), Some(Value::Text("2001-02-03")));
            assert_eq!(
                typed.rows[0].get(2),
                Some(Value::Text("2001-02-03 04:05:06+00"))
            );
            assert_eq!(typed.rows[0].get(3), Some(Value::Text("t")));
        }
    }
}

#[test]
fn parameterized_scripts_keep_transactions_and_stop_after_errors() {
    for (dialect, source) in native_sources("parameters-transactions") {
        let mut h = Harness::new(source, Settings::default());
        let table = format!("parameter_{}", uuid::Uuid::new_v4().simple());
        let create = format!("CREATE TABLE {table}(value INTEGER)");
        assert_eq!(
            h.run(&[&create], vec![], false).end,
            Some(ExecutionEnd::Completed)
        );
        let insert = format!("INSERT INTO {table} VALUES (:value)");
        let read = format!("SELECT value FROM {table} WHERE value=:value");
        let run = h.run(
            &[
                &insert,
                &read,
                "SELECT :value FROM missing_parameter_table",
                &insert,
            ],
            vec![parameter("value", Kind::Number, "42")],
            true,
        );
        assert_eq!(run.end, Some(ExecutionEnd::Failed), "{dialect:?}: {run:?}");
        assert_eq!(run.rows.len(), 1);
        assert_eq!(run.rows[0].get(0), Some(Value::Number("42")));
        let rolled = h.run(
            &["ROLLBACK", &format!("SELECT count(*) FROM {table}")],
            vec![],
            false,
        );
        assert_eq!(
            rolled.end,
            Some(ExecutionEnd::Completed),
            "{dialect:?}: {rolled:?}"
        );
        assert_eq!(rolled.rows[0].get(0), Some(Value::Number("0")));
        h.run(&[&format!("DROP TABLE {table}")], vec![], false);
    }
}
