//! Tests of the ClickHouse connector against a fake HTTP interface, and against a real
//! server in `server.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code: a panic is a failure.

mod server;

use std::sync::Arc;
use std::time::Duration;

use sqln_clickhouse::ClickHouseConnector;
use sqln_core::grid::{Budget, Row, Value};
use sqln_core::sink::{ResultSink, SinkEvent, StatementKey};
use sqln_core::text;
use sqln_session::connector::target::parse_uri;
use sqln_session::connector::{
    Connector, ErrorKind, ResetOutcome, Session, SessionContext, TransactionCommand,
    TransactionState,
};
use sqln_testkit::http::{Answer, HttpServer, body, request_line};
use tokio::sync::mpsc::{Receiver, channel};

const AUTH_FAILED: &str = "Code: 516. DB::Exception: sqln: Authentication failed: password is \
                           incorrect. (AUTHENTICATION_FAILED) (version 25.8.1.1)";

/// Answers `SELECT 1` of the connection check. `route` answers the other statements
/// with the request line and the statement.
fn server(route: impl Fn(&str, &str) -> Answer + Send + 'static) -> HttpServer {
    HttpServer::start(
        move |request| {
            if !request.contains("X-ClickHouse-Key: spike")
                && !request
                    .to_ascii_lowercase()
                    .contains("x-clickhouse-key: spike")
            {
                return (403, AUTH_FAILED.as_bytes().to_vec());
            }
            let sql = body(request);
            if sql == "SELECT 1" {
                return (200, b"1\n".to_vec());
            }
            route(request_line(request).1, sql)
        },
        Duration::ZERO,
    )
}

fn connector(server: &HttpServer, password: &str) -> ClickHouseConnector {
    let target = parse_uri(&format!(
        "clickhouse://sqln:{password}@127.0.0.1:{}/sales",
        server.port
    ))
    .unwrap();
    ClickHouseConnector::new(target)
}

async fn session(server: &HttpServer) -> Box<dyn Session> {
    connector(server, "spike").connect().await.unwrap()
}

pub(crate) fn new_sink(cap: usize) -> (ResultSink, Receiver<SinkEvent>) {
    let (events, inbox) = channel(1024);
    let key = StatementKey {
        console: 1,
        execution: 1,
        index: 0,
    };
    (
        ResultSink::new(key, events, Arc::new(Budget::new(1 << 30)), cap),
        inbox,
    )
}

/// Returns the columns, the types, and the rows that reached the sink.
pub(crate) fn received(
    inbox: &mut Receiver<SinkEvent>,
) -> (Vec<String>, Vec<Option<String>>, Vec<Row>) {
    let (mut columns, mut types, mut rows) = (Vec::new(), Vec::new(), Vec::new());
    while let Ok(event) = inbox.try_recv() {
        match event {
            SinkEvent::Columns {
                columns: names,
                types: kinds,
                ..
            } => {
                columns = names;
                types = kinds;
            }
            SinkEvent::Rows { rows: batch, .. } => rows.extend(batch),
            _ => {}
        }
    }
    (columns, types, rows)
}

#[tokio::test]
async fn a_wrong_password_asks_for_another_password() {
    let server = server(|_, _| (500, Vec::new()));
    let error = connector(&server, "bad").connect().await.err().unwrap();
    assert!(error.password_refused);
    assert_eq!(error.message, text::CLICKHOUSE_REFUSED);
    // The password goes in a header, never in the address.
    let requests = server.requests.lock().unwrap();
    assert!(!request_line(&requests[0]).1.contains("bad"));
}

#[tokio::test]
async fn each_value_keeps_its_type_null_and_bytes() {
    let server = server(|_, _| {
        let mut answer = b"id\tname\tdata\tprice\n\
                           UInt64\tNullable(String)\tString\tDecimal(10, 2)\n\
                           7\ta\\tb\\nc\t"
            .to_vec();
        answer.extend_from_slice(&[0xff, 0x00]);
        answer.extend_from_slice(b"\t1.50\n8\t\\N\t\\\\\t2.00\n");
        (200, answer)
    });
    let mut session = session(&server).await;
    let (sink, mut inbox) = new_sink(1000);
    let end = session.run("SELECT * FROM t", sink).await.unwrap();
    assert!(end.has_columns);
    assert_eq!(end.rows, 2);
    let (columns, types, rows) = received(&mut inbox);
    assert_eq!(columns, ["id", "name", "data", "price"]);
    assert_eq!(types[3].as_deref(), Some("Decimal(10, 2)"));
    assert_eq!(rows[0].get(0), Some(Value::Number("7")));
    assert_eq!(rows[0].get(1), Some(Value::Text("a\tb\nc")));
    assert_eq!(rows[0].get(2), Some(Value::binary(&[0xff, 0x00])));
    assert_eq!(rows[0].get(3), Some(Value::Number("1.50")));
    assert_eq!(rows[1].get(1), Some(Value::Null));
    assert_eq!(rows[1].get(2), Some(Value::Text("\\")));
}

#[tokio::test]
async fn a_statement_without_a_result_has_no_columns() {
    let server = server(|_, _| (200, Vec::new()));
    let mut session = session(&server).await;
    let (sink, _inbox) = new_sink(1000);
    let end = session
        .run("CREATE TABLE t (id UInt8) ENGINE = Memory", sink)
        .await
        .unwrap();
    assert!(!end.has_columns);
    assert_eq!(end.changed, None);
}

#[tokio::test]
async fn each_statement_carries_the_session_the_database_and_the_read_only_rule() {
    let server = server(|_, _| (200, Vec::new()));
    let mut session = session(&server).await;
    let context = SessionContext {
        read_only: true,
        schema: Some("audit".to_owned()),
        ..SessionContext::default()
    };
    session.apply_context(&context).await.unwrap();
    session.run("SELECT 2", new_sink(10).0).await.unwrap();
    assert_eq!(session.reset().await, ResetOutcome::Clean);
    session.run("SELECT 3", new_sink(10).0).await.unwrap();
    let requests = server.requests.lock().unwrap();
    let first = request_line(&requests[1]).1.to_owned();
    let second = request_line(&requests[2]).1.to_owned();
    assert!(first.contains("database=audit"), "{first}");
    assert!(first.contains("readonly=2"), "{first}");
    assert!(
        first.contains("default_format=TabSeparatedWithNamesAndTypes"),
        "{first}"
    );
    // The cleanup returns to the database of the connection, without the rule, in a new
    // server session.
    assert!(second.contains("database=sales"), "{second}");
    assert!(!second.contains("readonly"), "{second}");
    let session_id = |line: &str| {
        line.split(['?', '&'])
            .find(|part| part.starts_with("session_id="))
            .unwrap()
            .to_owned()
    };
    assert_ne!(session_id(&first), session_id(&second));
}

#[tokio::test]
async fn a_failed_statement_shows_the_error_and_its_position() {
    let server = server(|_, _| {
        (
            400,
            b"Code: 62. DB::Exception: Syntax error: failed at position 8 ('FROMM'). \
              (SYNTAX_ERROR) (version 25.8.1.1)"
                .to_vec(),
        )
    });
    let mut session = session(&server).await;
    let error = session
        .run("SELECT FROMM t", new_sink(10).0)
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Syntax);
    assert_eq!(error.offset, Some(7));
    assert!(error.message.contains("SYNTAX_ERROR"), "{}", error.message);
}

#[tokio::test]
async fn an_error_in_the_middle_of_the_answer_ends_the_statement() {
    let server = server(|_, _| {
        (
            200,
            b"n\nUInt64\n1\n2\nCode: 241. DB::Exception: Memory limit exceeded. \
              (MEMORY_LIMIT_EXCEEDED)\n"
                .to_vec(),
        )
    });
    let mut session = session(&server).await;
    let (sink, mut inbox) = new_sink(1000);
    let error = session.run("SELECT n", sink).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    // The rows before the error stay in the grid.
    assert_eq!(received(&mut inbox).2.len(), 2);
}

#[tokio::test]
async fn the_row_cap_stops_the_read() {
    let server = server(|_, _| (200, b"n\nUInt8\n1\n2\n3\n4\n".to_vec()));
    let mut session = session(&server).await;
    let (sink, mut inbox) = new_sink(2);
    let end = session.run("SELECT n", sink).await.unwrap();
    assert!(end.reached_limit());
    assert_eq!(received(&mut inbox).2.len(), 2);
}

#[tokio::test]
async fn transactions_are_refused_with_a_reason() {
    let server = server(|_, _| (200, Vec::new()));
    let mut session = session(&server).await;
    assert_eq!(session.transaction_state().await, TransactionState::None);
    let error = session
        .control(TransactionCommand::Begin)
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert_eq!(error.message, text::CLICKHOUSE_NO_TRANSACTIONS);
}

#[tokio::test]
async fn the_catalog_hides_the_system_databases_and_marks_views() {
    let server = server(|_, sql| {
        let answer: &[u8] = if sql.starts_with("SELECT currentDatabase()") {
            b"currentDatabase()\nString\nsales\n"
        } else if sql.contains("FROM system.databases") {
            assert!(sql.contains("'system'"), "{sql}");
            b"name\nString\naudit\nsales\n"
        } else if sql.contains("FROM system.tables") {
            b"database\tname\tengine\nString\tString\tString\n\
              sales\torders\tMergeTree\nsales\tdaily\tMaterializedView\n"
        } else {
            b""
        };
        (200, answer.to_vec())
    });
    let mut session = session(&server).await;
    let catalog = session.read_catalog().await.unwrap();
    assert_eq!(catalog.current_schema.as_deref(), Some("sales"));
    let names: Vec<&str> = catalog.schemas.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["audit", "sales"]);
    let sales = &catalog.schemas[1];
    assert_eq!(sales.relations.len(), 2);
    assert_eq!(
        sales.relations[1].kind,
        sqln_core::catalog::RelationKind::View
    );
}
