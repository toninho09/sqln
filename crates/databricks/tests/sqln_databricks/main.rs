//! Tests of the Databricks connector against a fake SQL Statement Execution API.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arrow_array::{ArrayRef, BinaryArray, Float64Array, Int64Array, RecordBatch, StringArray};
use arrow_ipc::writer::StreamWriter;
use serde_json::{Value as Json, json};
use sqln_core::grid::{Budget, Row, Value};
use sqln_core::sink::{ReadLimit, ResultSink, SinkEvent, StatementKey};
use sqln_core::text;
use sqln_databricks::DatabricksConnector;
use sqln_session::connector::target::parse_uri;
use sqln_session::connector::{Connector, Session, SessionContext};
use sqln_testkit::http::{Answer, HttpServer, body, request_line};
use tokio::sync::mpsc::{Receiver, channel};

const TOKEN: &str = "good-token";
const STATEMENTS: &str = "/api/2.0/sql/statements/";

/// Answers the warehouse check with the token rule. `route` answers the other requests
/// with the method, the path, and the full request text.
fn server(route: impl Fn(&str, &str, &str) -> Answer + Send + 'static) -> HttpServer {
    HttpServer::start(
        move |request| {
            let (method, path) = request_line(request);
            if path.starts_with("/api/2.0/sql/warehouses/") {
                let good = request
                    .to_ascii_lowercase()
                    .contains(&format!("authorization: bearer {TOKEN}"));
                return if good && path.ends_with("/W1") {
                    (200, b"{}".to_vec())
                } else if good {
                    (404, b"{}".to_vec())
                } else {
                    (401, b"{}".to_vec())
                };
            }
            route(method, path, request)
        },
        Duration::ZERO,
    )
}

fn connector(server: &HttpServer, token: &str) -> DatabricksConnector {
    let target = parse_uri(&format!(
        "databricks://:{token}@127.0.0.1:{}/main?warehouse=W1",
        server.port
    ))
    .unwrap();
    DatabricksConnector::new(target)
}

async fn session(server: &HttpServer) -> Box<dyn Session> {
    connector(server, TOKEN).connect().await.unwrap()
}

/// The address of the fake server, from the `Host` header of a request.
fn base(request: &str) -> String {
    let host = request
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("host").then(|| value.trim())
        })
        .unwrap();
    format!("http://{host}")
}

fn json(value: Json) -> Answer {
    (200, value.to_string().into_bytes())
}

fn link(request: &str, chunk: usize, file: &str) -> Json {
    json!({"chunk_index": chunk, "external_link": format!("{}/files/{file}", base(request))})
}

fn succeeded(columns: &[&str], chunks: usize, links: Vec<Json>) -> Json {
    let columns: Vec<Json> = columns.iter().map(|name| json!({"name": name})).collect();
    json!({
        "statement_id": "s1",
        "status": {"state": "SUCCEEDED"},
        "manifest": {"schema": {"columns": columns}, "total_chunk_count": chunks},
        "result": {"external_links": links},
    })
}

fn state(state: &str) -> Answer {
    json(json!({"statement_id": "s1", "status": {"state": state}}))
}

fn stream(batch: &RecordBatch) -> Vec<u8> {
    let mut writer = StreamWriter::try_new(Vec::new(), &batch.schema()).unwrap();
    writer.write(batch).unwrap();
    writer.finish().unwrap();
    writer.into_inner().unwrap()
}

/// A batch with a number, a text, bytes, and a decimal number in each row.
fn mixed(ids: &[Option<i64>]) -> Vec<u8> {
    let count = ids.len();
    let columns: Vec<(&str, ArrayRef)> = vec![
        ("id", Arc::new(Int64Array::from(ids.to_vec()))),
        ("name", Arc::new(StringArray::from(vec!["ação"; count]))),
        (
            "data",
            Arc::new(BinaryArray::from(vec![&[0x00u8, 0xff][..]; count])),
        ),
        ("price", Arc::new(Float64Array::from(vec![1.5; count]))),
    ];
    stream(&RecordBatch::try_from_iter(columns).unwrap())
}

/// A batch of text cells.
fn texts(names: &[&str], rows: &[&[Option<&str>]]) -> Vec<u8> {
    let columns: Vec<(&str, ArrayRef)> = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let cells: Vec<Option<&str>> = rows.iter().map(|row| row[index]).collect();
            (*name, Arc::new(StringArray::from(cells)) as ArrayRef)
        })
        .collect();
    stream(&RecordBatch::try_from_iter(columns).unwrap())
}

fn new_sink(cap: usize) -> (ResultSink, Receiver<SinkEvent>) {
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

/// Returns the columns and the rows that reached the sink.
fn received(inbox: &mut Receiver<SinkEvent>) -> (Vec<String>, Vec<Row>) {
    let (mut columns, mut rows) = (Vec::new(), Vec::new());
    while let Ok(event) = inbox.try_recv() {
        match event {
            SinkEvent::Columns { columns: names, .. } => columns = names,
            SinkEvent::Rows { rows: batch, .. } => rows.extend(batch),
            _ => {}
        }
    }
    (columns, rows)
}

fn paths(server: &HttpServer) -> Vec<String> {
    server
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(|request| request_line(request).1.to_owned())
        .collect()
}

#[tokio::test]
async fn a_wrong_token_asks_for_another_password() {
    let server = server(|_, _, _| (500, Vec::new()));
    let error = connector(&server, "bad").connect().await.err().unwrap();
    assert!(error.password_refused);
    assert_eq!(error.message, text::DATABRICKS_REFUSED);
}

#[tokio::test]
async fn a_connection_without_a_token_sends_no_request() {
    let server = server(|_, _, _| (500, Vec::new()));
    let target = parse_uri(&format!(
        "databricks://127.0.0.1:{}?warehouse=W1",
        server.port
    ))
    .unwrap();
    let error = DatabricksConnector::new(target)
        .connect()
        .await
        .err()
        .unwrap();
    assert!(error.password_refused);
    assert!(server.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_unknown_warehouse_names_the_warehouse() {
    let server = server(|_, _, _| (500, Vec::new()));
    let target = parse_uri(&format!(
        "databricks://:{TOKEN}@127.0.0.1:{}?warehouse=W2",
        server.port
    ))
    .unwrap();
    let error = DatabricksConnector::new(target)
        .connect()
        .await
        .err()
        .unwrap();
    assert!(!error.password_refused);
    assert_eq!(error.message, text::DATABRICKS_NO_SUCH_WAREHOUSE);
}

#[tokio::test]
async fn a_pending_statement_delivers_each_value_after_the_status_polls() {
    let polls = Arc::new(Mutex::new(0));
    let counted = polls.clone();
    let server = server(move |method, path, request| match (method, path) {
        ("POST", STATEMENTS) => state("PENDING"),
        ("GET", "/api/2.0/sql/statements/s1") => {
            let mut polls = counted.lock().unwrap();
            *polls += 1;
            if *polls < 2 {
                state("RUNNING")
            } else {
                json(succeeded(
                    &["id", "name", "data", "price"],
                    1,
                    vec![link(request, 0, "0")],
                ))
            }
        }
        ("GET", "/files/0") => (200, mixed(&[Some(7), None])),
        _ => (404, Vec::new()),
    });
    let mut session = session(&server).await;
    let (sink, mut inbox) = new_sink(1000);
    let end = session.run("SELECT 1", sink).await.unwrap();
    assert!(end.has_columns);
    assert_eq!(end.rows, 2);
    assert_eq!(*polls.lock().unwrap(), 2);
    let (columns, rows) = received(&mut inbox);
    assert_eq!(columns, ["id", "name", "data", "price"]);
    assert_eq!(rows[0].get(0), Some(Value::Number("7")));
    assert_eq!(rows[1].get(0), Some(Value::Null));
    assert_eq!(rows[0].get(1), Some(Value::Text("ação")));
    assert_eq!(rows[0].get(2), Some(Value::binary(&[0x00, 0xff])));
    assert_eq!(rows[0].get(3), Some(Value::Number("1.5")));
}

#[tokio::test]
async fn a_result_link_never_receives_the_token() {
    let server = server(|method, path, request| match (method, path) {
        ("POST", STATEMENTS) => json(succeeded(&["id"], 1, vec![link(request, 0, "0")])),
        ("GET", "/files/0") => (200, mixed(&[Some(1)])),
        _ => (404, Vec::new()),
    });
    let mut session = session(&server).await;
    let (sink, _inbox) = new_sink(1000);
    session.run("SELECT 1", sink).await.unwrap();
    let requests = server.requests.lock().unwrap();
    for request in requests.iter() {
        let has_token = request.contains(TOKEN);
        if request_line(request).1.starts_with("/files/") {
            assert!(!has_token, "{request}");
            assert!(!request.to_ascii_lowercase().contains("authorization"));
        } else {
            assert!(has_token, "{request}");
        }
    }
}

#[tokio::test]
async fn the_parts_arrive_in_order_and_the_row_cap_stops_the_downloads() {
    let server = server(|method, path, request| match (method, path) {
        ("POST", STATEMENTS) => json(succeeded(
            &["id", "name", "data", "price"],
            2,
            vec![link(request, 0, "0")],
        )),
        ("GET", "/api/2.0/sql/statements/s1/result/chunks/1") => {
            json(json!({"external_links": [link(request, 1, "1")]}))
        }
        ("GET", "/files/0") => (200, mixed(&[Some(1), Some(2)])),
        ("GET", "/files/1") => (200, mixed(&[Some(3), Some(4)])),
        _ => (404, Vec::new()),
    });
    let mut session = session(&server).await;

    let (sink, mut inbox) = new_sink(1000);
    assert_eq!(session.run("SELECT 1", sink).await.unwrap().rows, 4);
    let (_, rows) = received(&mut inbox);
    let ids: Vec<_> = rows.iter().map(|row| row.text(0).unwrap()).collect();
    assert_eq!(ids, ["1", "2", "3", "4"]);

    server.requests.lock().unwrap().clear();
    let (sink, _inbox) = new_sink(1);
    let end = session.run("SELECT 1", sink).await.unwrap();
    assert_eq!(end.limit, ReadLimit::RowCap);
    assert!(!paths(&server).contains(&"/files/1".to_owned()));
}

#[tokio::test]
async fn an_expired_link_gets_a_new_link() {
    let server = server(|method, path, request| match (method, path) {
        ("POST", STATEMENTS) => json(succeeded(&["id"], 1, vec![link(request, 0, "old")])),
        ("GET", "/api/2.0/sql/statements/s1/result/chunks/0") => {
            json(json!({"external_links": [link(request, 0, "new")]}))
        }
        ("GET", "/files/old") => (403, Vec::new()),
        ("GET", "/files/new") => (200, mixed(&[Some(9)])),
        _ => (404, Vec::new()),
    });
    let mut session = session(&server).await;
    let (sink, mut inbox) = new_sink(1000);
    session.run("SELECT 1", sink).await.unwrap();
    let (_, rows) = received(&mut inbox);
    assert_eq!(rows[0].text(0).as_deref(), Some("9"));
}

#[tokio::test]
async fn a_cancel_stops_the_statement_in_the_warehouse() {
    let canceled = Arc::new(AtomicBool::new(false));
    let seen = canceled.clone();
    let server = server(move |method, path, _| match (method, path) {
        ("POST", STATEMENTS) => state("RUNNING"),
        ("POST", "/api/2.0/sql/statements/s1/cancel") => {
            seen.store(true, Ordering::SeqCst);
            (200, b"{}".to_vec())
        }
        ("GET", "/api/2.0/sql/statements/s1") if seen.load(Ordering::SeqCst) => state("CANCELED"),
        ("GET", "/api/2.0/sql/statements/s1") => state("RUNNING"),
        _ => (404, Vec::new()),
    });
    let mut session = session(&server).await;
    let canceller = session.canceller();
    let (sink, _inbox) = new_sink(1000);
    let run = tokio::spawn(async move { session.run("SELECT 1", sink).await });
    tokio::time::sleep(Duration::from_millis(150)).await;
    canceller.cancel().await.unwrap();
    let error = run.await.unwrap().unwrap_err();
    assert_eq!(error.message, text::DATABRICKS_CANCELED);
    assert!(canceled.load(Ordering::SeqCst));
}

#[tokio::test]
async fn a_failed_statement_shows_the_error_of_databricks() {
    let server = server(|method, path, _| match (method, path) {
        ("POST", STATEMENTS) => json(json!({
            "statement_id": "s1",
            "status": {"state": "FAILED", "error": {
                "error_code": "PARSE_SYNTAX_ERROR",
                "message": "Syntax error at or near 'SELEC'.",
            }},
        })),
        _ => (404, Vec::new()),
    });
    let mut session = session(&server).await;
    let (sink, _inbox) = new_sink(1000);
    let error = session.run("SELEC 1", sink).await.unwrap_err();
    assert_eq!(
        error.message,
        "Syntax error at or near 'SELEC'. (PARSE_SYNTAX_ERROR)"
    );
}

#[tokio::test]
async fn the_catalog_names_each_schema_with_its_catalog() {
    // The download answers with the result of the last statement.
    let statement = Arc::new(Mutex::new(String::new()));
    let server = server(move |method, path, request| match (method, path) {
        ("POST", STATEMENTS) => {
            let sql: Json = serde_json::from_str(body(request)).unwrap();
            *statement.lock().unwrap() = sql["statement"].as_str().unwrap().to_owned();
            json(succeeded(&["v"], 1, vec![link(request, 0, "0")]))
        }
        ("GET", "/files/0") => {
            let sql = statement.lock().unwrap().clone();
            let bytes = if sql.contains("current_catalog()") {
                texts(&["c"], &[&[Some("main.default")]])
            } else if sql.contains("information_schema.schemata") {
                texts(
                    &["s"],
                    &[&[Some("main.default")], &[Some("samples.nyctaxi")]],
                )
            } else {
                texts(
                    &["s", "t", "k"],
                    &[
                        &[Some("main.default"), Some("orders"), Some("MANAGED")],
                        &[Some("samples.nyctaxi"), Some("trips"), Some("VIEW")],
                    ],
                )
            };
            (200, bytes)
        }
        _ => (404, Vec::new()),
    });
    let mut session = session(&server).await;
    let catalog = session.read_catalog().await.unwrap();
    assert_eq!(catalog.current_schema.as_deref(), Some("main.default"));
    let names: Vec<_> = catalog.schemas.iter().map(|schema| &schema.name).collect();
    assert_eq!(names, ["main.default", "samples.nyctaxi"]);
    let trips = &catalog.schemas[1].relations[0];
    assert_eq!(trips.name, "trips");
    assert_eq!(trips.kind, sqln_core::catalog::RelationKind::View);
    assert_eq!(
        catalog.schemas[0].relations[0].kind,
        sqln_core::catalog::RelationKind::Table
    );
}

#[tokio::test]
async fn a_read_only_connection_refuses_a_write_without_a_request() {
    let server = server(|_, _, _| (500, Vec::new()));
    let mut session = session(&server).await;
    session
        .apply_context(&SessionContext {
            read_only: true,
            ..SessionContext::default()
        })
        .await
        .unwrap();
    let before = server.requests.lock().unwrap().len();
    let (sink, _inbox) = new_sink(1000);
    let error = session
        .run("INSERT INTO t VALUES (1)", sink)
        .await
        .unwrap_err();
    assert_eq!(error.message, text::DATABRICKS_READ_ONLY);
    assert_eq!(server.requests.lock().unwrap().len(), before);
}

#[tokio::test]
async fn each_statement_carries_the_catalog_and_the_schema_of_the_session() {
    let server = server(|method, path, _| match (method, path) {
        ("POST", STATEMENTS) => json(succeeded(&[], 0, Vec::new())),
        _ => (404, Vec::new()),
    });
    let mut session = session(&server).await;
    let sent = |server: &HttpServer| -> Json {
        let requests = server.requests.lock().unwrap();
        serde_json::from_str(body(requests.last().unwrap())).unwrap()
    };

    let (sink, _inbox) = new_sink(1000);
    let end = session.run("CREATE TABLE t (id INT)", sink).await.unwrap();
    assert!(!end.has_columns);
    let request = sent(&server);
    assert_eq!(request["catalog"], "main");
    assert!(request.get("schema").is_none());
    assert_eq!(request["warehouse_id"], "W1");

    session
        .apply_context(&SessionContext {
            schema: Some("samples.nyctaxi".to_owned()),
            ..SessionContext::default()
        })
        .await
        .unwrap();
    let (sink, _inbox) = new_sink(1000);
    session.run("SELECT 1", sink).await.unwrap();
    let request = sent(&server);
    assert_eq!(request["catalog"], "samples");
    assert_eq!(request["schema"], "nyctaxi");
}
