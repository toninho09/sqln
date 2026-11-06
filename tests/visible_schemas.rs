// The visible schemas of a connection: the list in the definition, the field and its list
// in the form, and the catalog read. These tests need no server. `tests/servers.rs` and
// `tests/redis.rs` read real servers.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{ctrl, key};
use async_trait::async_trait;
use crossterm::event::{Event, KeyCode};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::catalog::{Catalog, Schema};
use sqln::clipboard::MemoryClipboard;
use sqln::connection::{Definition, Source};
use sqln::connector::sqlite::SqliteTarget;
use sqln::connector::target::{Address, Engine, NetworkTarget, Tls};
use sqln::connector::{
    Canceller, ConnectError, Connector, ExecError, ResetOutcome, Session, SessionContext,
    StatementEnd, TransactionCommand, TransactionState,
};
use sqln::exec::{ExecEvent, Manager, ResultSink, Settings};
use sqln::form::{ConnectionForm, Field, FormNote, Kind, schema_names};
use sqln::grid::Budget;
use sqln::navigator::{RowKind, RowNote};
use sqln::sql::Dialect;
use sqln::ui::theme::Theme;
use tokio::sync::mpsc::Receiver;

fn schema(name: &str) -> Schema {
    Schema {
        name: name.to_owned(),
        relations: Vec::new(),
        partial: false,
        objects: Vec::new(),
    }
}

fn catalog(names: &[&str], current: &str) -> Catalog {
    Catalog {
        schemas: names.iter().map(|name| schema(name)).collect(),
        current_schema: Some(current.to_owned()),
    }
}

fn names_of(catalog: &Catalog) -> Vec<&str> {
    catalog
        .schemas
        .iter()
        .map(|schema| schema.name.as_str())
        .collect()
}

fn list(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn the_catalog_keeps_the_listed_schemas_and_the_current_schema() {
    let mut found = catalog(&["app", "logs", "public", "sales"], "public");
    found.keep_schemas(&list(&["sales", "gone"]));
    assert_eq!(names_of(&found), ["public", "sales"]);
}

#[test]
fn an_empty_list_keeps_all_schemas() {
    let mut found = catalog(&["app", "public"], "public");
    found.keep_schemas(&[]);
    assert_eq!(names_of(&found), ["app", "public"]);
}

#[test]
fn the_field_splits_names_at_commas_without_repeats() {
    assert_eq!(
        schema_names(" public, sales,,public , "),
        ["public", "sales"]
    );
    assert!(schema_names("  ").is_empty());
}

fn network(engine: Engine) -> Definition {
    let target = NetworkTarget {
        engine,
        address: Address::Tcp {
            host: "127.0.0.1".into(),
            port: 1,
        },
        user: "u".into(),
        password: None,
        database: None,
        tls: Tls::Disabled,
    };
    Definition::new("remote", Source::Network(target))
}

#[test]
fn the_field_exists_for_network_kinds_also_when_connected() {
    for kind in [Kind::Postgres, Kind::MySql, Kind::Redis] {
        let mut form = ConnectionForm::new();
        form.kind = kind;
        assert!(form.fields().contains(&Field::Schemas), "{kind:?}");
    }
    let mut form = ConnectionForm::new();
    form.kind = Kind::Sqlite;
    assert!(!form.fields().contains(&Field::Schemas));

    let mut definition = network(Engine::Postgres);
    definition.schemas = list(&["public", "sales"]);
    let connected = ConnectionForm::editing(1, &definition, true);
    assert!(connected.fields().contains(&Field::Schemas));
    assert_eq!(connected.text(Field::Schemas), "public, sales");
    assert_eq!(connected.definition().unwrap().schemas, ["public", "sales"]);
    let sqlite = Definition::sqlite(SqliteTarget::new("a.db"));
    let connected = ConnectionForm::editing(1, &sqlite, true);
    assert!(!connected.fields().contains(&Field::Schemas));
}

#[test]
fn a_redis_list_needs_numbers() {
    let mut definition = network(Engine::Redis);
    definition.schemas = list(&["0", "cache"]);
    let form = ConnectionForm::editing(1, &definition, false);
    let invalid = form.definition().unwrap_err();
    assert_eq!(invalid.field, Field::Schemas);
    definition.schemas = list(&["0", "8"]);
    let form = ConnectionForm::editing(1, &definition, false);
    assert_eq!(form.definition().unwrap().schemas, ["0", "8"]);
}

fn press(form: &mut ConnectionForm, code: KeyCode) {
    form.on_key(key(code), false);
}

fn write(form: &mut ConnectionForm, text: &str) {
    for c in text.chars() {
        press(form, KeyCode::Char(c));
    }
}

fn form_with(names: &[&str]) -> ConnectionForm {
    let mut definition = network(Engine::Postgres);
    definition.schemas = list(names);
    ConnectionForm::editing(1, &definition, false)
}

#[test]
fn the_list_marks_names_and_writes_them_in_the_order_of_the_server() {
    let mut form = form_with(&["sales"]);
    form.open_schema_list();
    // Keys wait for the names of the server.
    press(&mut form, KeyCode::Char(' '));
    press(&mut form, KeyCode::Enter);
    assert!(form.schema_list.as_ref().unwrap().loading);
    form.fill_schema_list(list(&["public", "sales", "logs"]));

    press(&mut form, KeyCode::Char(' '));
    write(&mut form, "lo");
    press(&mut form, KeyCode::Char(' '));
    press(&mut form, KeyCode::Enter);
    assert!(form.schema_list.is_none());
    assert_eq!(form.text(Field::Schemas), "public, sales, logs");
    assert_eq!(form.focus, Field::Schemas);
}

#[test]
fn space_removes_a_mark_and_escape_keeps_the_text() {
    let mut form = form_with(&["public", "sales"]);
    form.open_schema_list();
    form.fill_schema_list(list(&["public", "sales"]));
    press(&mut form, KeyCode::Char(' '));
    press(&mut form, KeyCode::Esc);
    assert!(form.schema_list.is_none());
    assert_eq!(form.text(Field::Schemas), "public, sales");

    form.open_schema_list();
    form.fill_schema_list(list(&["public", "sales"]));
    press(&mut form, KeyCode::Char(' '));
    press(&mut form, KeyCode::Enter);
    assert_eq!(form.text(Field::Schemas), "sales");
}

#[test]
fn a_name_that_the_server_does_not_list_stays_marked_at_the_end() {
    let mut form = form_with(&["gone"]);
    form.open_schema_list();
    form.fill_schema_list(list(&["public"]));
    let open = form.schema_list.as_ref().unwrap();
    assert_eq!(open.list.list().items(), ["public", "gone"]);
    press(&mut form, KeyCode::Enter);
    assert_eq!(form.text(Field::Schemas), "gone");
}

#[test]
fn escape_stops_a_read_that_did_not_end() {
    let mut form = form_with(&[]);
    form.test = Some(3);
    form.open_schema_list();
    press(&mut form, KeyCode::Esc);
    assert!(form.schema_list.is_none());
    assert_eq!(form.test, None);
}

fn draw_form(form: &ConnectionForm) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| {
            sqln::ui::form::draw_form(frame, form, &Theme::plain());
        })
        .unwrap();
    buffer_text(&terminal)
}

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for row in buffer.content.chunks(usize::from(buffer.area.width)) {
        text.extend(row.iter().map(|cell| cell.symbol()));
        text.push('\n');
    }
    text
}

#[test]
fn the_label_follows_the_kind_and_the_list_shows_the_marks() {
    let mut form = form_with(&["sales"]);
    form.focus_on(Field::Schemas);
    let screen = draw_form(&form);
    assert!(screen.contains("Schemas"), "{screen}");
    assert!(screen.contains("public, sales") || screen.contains("sales"));

    let mut redis = ConnectionForm::editing(1, &network(Engine::Redis), false);
    redis.focus_on(Field::Schemas);
    let screen = draw_form(&redis);
    assert!(screen.contains("Databases"), "{screen}");

    form.open_schema_list();
    let screen = draw_form(&form);
    assert!(screen.contains("Reading the names."), "{screen}");
    form.fill_schema_list(list(&["public", "sales"]));
    let screen = draw_form(&form);
    assert!(screen.contains("[ ] public"), "{screen}");
    assert!(screen.contains("[x] sales"), "{screen}");
}

// ---- the catalog read of the manager ----

struct Fake;
struct FakeSession;
struct NoCancel;

#[async_trait]
impl Canceller for NoCancel {
    async fn cancel(&self) -> Result<(), String> {
        Ok(())
    }
}

#[async_trait]
impl Connector for Fake {
    fn dialect(&self) -> Dialect {
        Dialect::Postgres
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        Ok(Box::new(FakeSession))
    }
}

#[async_trait]
impl Session for FakeSession {
    fn canceller(&self) -> Arc<dyn Canceller> {
        Arc::new(NoCancel)
    }

    async fn apply_context(&mut self, _: &SessionContext) -> Result<(), ExecError> {
        Ok(())
    }

    async fn run(&mut self, _: &str, sink: ResultSink) -> Result<StatementEnd, ExecError> {
        sink.finish().await;
        Ok(StatementEnd::default())
    }

    async fn transaction_state(&mut self) -> TransactionState {
        TransactionState::None
    }

    async fn control(&mut self, _: TransactionCommand) -> Result<(), ExecError> {
        Ok(())
    }

    async fn reset(&mut self) -> ResetOutcome {
        ResetOutcome::Clean
    }

    async fn read_catalog(&mut self) -> Result<Catalog, ExecError> {
        Ok(catalog(&["app", "logs", "public"], "public"))
    }

    async fn read_columns(&mut self, _: &str, _: &str) -> Result<Vec<String>, ExecError> {
        Ok(Vec::new())
    }
}

fn next(events: &mut Receiver<ExecEvent>) -> ExecEvent {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(event) = events.try_recv() {
            return event;
        }
        assert!(Instant::now() < deadline, "no event arrived");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn the_manager_reads_the_listed_schemas_and_a_test_can_read_all_names() {
    let (mut manager, mut events) = Manager::new(Arc::new(Budget::new(1 << 20))).unwrap();
    let connection = manager.add_connection(Arc::new(Fake), Settings::default());
    manager.load_catalog(connection, 1, list(&["logs"]));
    let ExecEvent::CatalogLoaded { catalog, .. } = next(&mut events) else {
        panic!("a catalog")
    };
    assert_eq!(names_of(&catalog.unwrap()), ["logs", "public"]);

    manager.test_connection(7, Arc::new(Fake), Duration::from_secs(5), true);
    let ExecEvent::ConnectionTested { test, result } = next(&mut events) else {
        panic!("a test")
    };
    assert_eq!(test, 7);
    assert_eq!(result.unwrap(), Some(list(&["app", "logs", "public"])));

    manager.test_connection(8, Arc::new(Fake), Duration::from_secs(5), false);
    let ExecEvent::ConnectionTested { result, .. } = next(&mut events) else {
        panic!("a test")
    };
    assert_eq!(result.unwrap(), None);
}

// ---- the application ----

fn app_with(definition: Definition) -> App {
    let options = Options {
        connections: vec![definition],
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    App::new(options, clipboard, Theme::plain())
}

fn screen(app: &mut App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    buffer_text(&terminal)
}

fn connection_note(app: &App) -> Option<RowNote> {
    let rows = app.navigator().rows(app.connections());
    let row = rows.iter().find(|row| row.kind == RowKind::Connection)?;
    app.navigator().suffix(row, app.connections())
}

#[test]
fn a_connection_with_a_list_shows_a_note_in_the_tree() {
    let path = crate::common::temp_db("visible-note");
    let mut definition = Definition::sqlite(SqliteTarget::new(&path));
    definition.name = "db".into();
    let app = app_with(definition.clone());
    assert_eq!(connection_note(&app), None);
    definition.schemas = list(&["main"]);
    let mut app = app_with(definition);
    assert_eq!(connection_note(&app), Some(RowNote::Filtered));
    let shown = screen(&mut app);
    assert!(shown.contains("(filtered)"), "{shown}");
}

#[test]
fn a_list_that_cannot_connect_closes_with_the_error_in_the_form() {
    let mut app = app_with(network(Engine::Postgres));
    crate::common::select_row(&mut app, 0, &[RowKind::Connection], "remote");
    app.on_event(Event::Key(key(KeyCode::Char('e'))));
    crate::common::focus_form_field(&mut app, Field::Schemas);
    app.on_event(Event::Key(ctrl('o')));
    assert!(app.form().unwrap().schema_list.is_some());
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.form().unwrap().test.is_some() {
        app.drain_events();
        assert!(Instant::now() < deadline, "the read did not end");
        std::thread::sleep(Duration::from_millis(1));
    }
    let form = app.form().unwrap();
    assert!(form.schema_list.is_none());
    assert!(
        matches!(form.note, Some(FormNote::Error(_))),
        "{:?}",
        form.note
    );
}
