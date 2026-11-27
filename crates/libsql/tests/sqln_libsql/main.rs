//! Tests of the libSQL connector against a fake Hrana server, and against a real `sqld`
//! in `server.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code: a panic is a failure.

mod server;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value as Json, json};
use sqln_core::grid::{Budget, Row, Value};
use sqln_core::sink::{ResultSink, SinkEvent, StatementKey};
use sqln_core::text;
use sqln_libsql::LibsqlConnector;
use sqln_session::connector::target::parse_uri;
use sqln_session::connector::{
    Connector, ErrorKind, ResetOutcome, Session, TransactionCommand, TransactionState,
};
use sqln_sql::sql::parameters::{Kind, Parameter};
use sqln_testkit::http::{Answer, HttpServer, body};
use tokio::sync::mpsc::{Receiver, channel};

const TOKEN: &str = "good-token";

/// A fake server. It checks the token, and `route` answers the requests of each
/// pipeline after the connection check.
fn server(route: impl Fn(&Json) -> Answer + Send + 'static, delay: Duration) -> HttpServer {
    HttpServer::start(
        move |request| {
            let good = request
                .to_ascii_lowercase()
                .contains(&format!("authorization: bearer {TOKEN}"));
            if !good {
                return (401, b"Unauthorized".to_vec());
            }
            let pipeline: Json = serde_json::from_str(body(request)).unwrap();
            if pipeline["requests"][0]["stmt"]["sql"] == "SELECT 1" {
                return answer(
                    Json::Null,
                    vec![
                        execute(&[], &[]),
                        json!({"type": "ok", "response": {"type": "close"}}),
                    ],
                );
            }
            route(&pipeline)
        },
        delay,
    )
}

fn answer(baton: Json, results: Vec<Json>) -> Answer {
    let body = json!({"baton": baton, "base_url": null, "results": results});
    (200, body.to_string().into_bytes())
}

fn execute(cols: &[(&str, Option<&str>)], rows: &[Json]) -> Json {
    let cols: Vec<Json> = cols
        .iter()
        .map(|(name, decltype)| json!({"name": name, "decltype": decltype}))
        .collect();
    json!({"type": "ok", "response": {"type": "execute", "result": {
        "cols": cols, "rows": rows, "affected_row_count": 3, "last_insert_rowid": null
    }}})
}

fn connector(server: &HttpServer, token: &str) -> LibsqlConnector {
    let target = parse_uri(&format!("libsql://:{token}@127.0.0.1:{}", server.port)).unwrap();
    LibsqlConnector::new(target)
}

async fn session(server: &HttpServer) -> Box<dyn Session> {
    connector(server, TOKEN).connect().await.unwrap()
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

fn pipelines(server: &HttpServer) -> Vec<Json> {
    server
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(|request| serde_json::from_str(body(request)).unwrap())
        .collect()
}

#[tokio::test]
async fn a_wrong_token_asks_for_another_password() {
    let server = server(|_| answer(Json::Null, Vec::new()), Duration::ZERO);
    let error = connector(&server, "bad").connect().await.err().unwrap();
    assert!(error.password_refused);
    assert_eq!(error.message, text::LIBSQL_REFUSED);
}

#[tokio::test]
async fn each_value_keeps_its_type_null_and_bytes() {
    let server = server(
        |_| {
            answer(
                json!("b1"),
                vec![execute(
                    &[
                        ("id", Some("INTEGER")),
                        ("price", None),
                        ("data", Some("BLOB")),
                        ("name", Some("TEXT")),
                    ],
                    &[
                        json!([{"type": "integer", "value": "7"}, {"type": "float", "value": 1.5},
                               {"type": "blob", "base64": "AP8"}, {"type": "text", "value": "ação"}]),
                        json!([{"type": "null"}, {"type": "null"}, {"type": "null"}, {"type": "null"}]),
                    ],
                )],
            )
        },
        Duration::ZERO,
    );
    let mut session = session(&server).await;
    let (sink, mut inbox) = new_sink(1000);
    let end = session.run("SELECT * FROM t", sink).await.unwrap();
    assert!(end.has_columns);
    assert_eq!(end.rows, 2);
    let (columns, types, rows) = received(&mut inbox);
    assert_eq!(columns, ["id", "price", "data", "name"]);
    assert_eq!(types[0].as_deref(), Some("INTEGER"));
    assert_eq!(rows[0].get(0), Some(Value::Number("7")));
    assert_eq!(rows[0].get(1), Some(Value::Number("1.5")));
    assert_eq!(rows[0].get(2), Some(Value::binary(&[0x00, 0xff])));
    assert_eq!(rows[0].get(3), Some(Value::Text("ação")));
    assert_eq!(rows[1].get(0), Some(Value::Null));
}

#[tokio::test]
async fn a_change_gives_its_row_count_and_the_stream_keeps_its_baton() {
    let server = server(
        |_| answer(json!("b1"), vec![execute(&[], &[])]),
        Duration::ZERO,
    );
    let mut session = session(&server).await;
    let end = session
        .run("UPDATE t SET x = 1", new_sink(10).0)
        .await
        .unwrap();
    assert!(!end.has_columns);
    assert_eq!(end.changed, Some(3));
    session
        .run("UPDATE t SET x = 2", new_sink(10).0)
        .await
        .unwrap();
    let pipelines = pipelines(&server);
    assert_eq!(pipelines[1]["baton"], Json::Null);
    assert_eq!(pipelines[2]["baton"], "b1");
}

#[tokio::test]
async fn parameters_go_as_positional_arguments() {
    let server = server(
        |_| answer(Json::Null, vec![execute(&[], &[])]),
        Duration::ZERO,
    );
    let mut session = session(&server).await;
    let parameters = [
        Parameter {
            name: "a".to_owned(),
            kind: Kind::Text,
            value: "x".to_owned(),
        },
        Parameter {
            name: "b".to_owned(),
            kind: Kind::Boolean,
            value: "true".to_owned(),
        },
        Parameter {
            name: "c".to_owned(),
            kind: Kind::Null,
            value: String::new(),
        },
    ];
    session
        .run_parameters(
            "UPDATE t SET a = ?, b = ?, c = ?",
            &parameters,
            new_sink(10).0,
        )
        .await
        .unwrap();
    let args = &pipelines(&server)[1]["requests"][0]["stmt"]["args"];
    assert_eq!(
        args,
        &json!([{"type": "text", "value": "x"}, {"type": "integer", "value": "1"}, {"type": "null"}])
    );
}

#[tokio::test]
async fn a_failed_statement_shows_the_error_of_the_server() {
    let server = server(
        |_| {
            answer(
                Json::Null,
                vec![json!({"type": "error", "error": {
                    "message": "SQL string could not be parsed", "code": "SQL_PARSE_ERROR"
                }})],
            )
        },
        Duration::ZERO,
    );
    let mut session = session(&server).await;
    let error = session.run("SELEC", new_sink(10).0).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Syntax);
    assert!(
        error.message.contains("SQL_PARSE_ERROR"),
        "{}",
        error.message
    );
    // The statement failed, but the session stays usable.
    assert!(!error.closes_session());
}

#[tokio::test]
async fn a_closed_stream_ends_the_session() {
    let server = server(
        |pipeline| {
            if pipeline["baton"].is_null() {
                answer(json!("b1"), vec![execute(&[], &[])])
            } else {
                (400, b"Received an invalid baton".to_vec())
            }
        },
        Duration::ZERO,
    );
    let mut session = session(&server).await;
    session.control(TransactionCommand::Begin).await.unwrap();
    let error = session.run("SELECT 2", new_sink(10).0).await.unwrap_err();
    assert_eq!(error.message, text::LIBSQL_STREAM_CLOSED);
    assert!(error.closes_session());
    assert_eq!(session.transaction_state().await, TransactionState::Unknown);
    assert_eq!(session.reset().await, ResetOutcome::Discard);
}

#[tokio::test]
async fn a_cancel_stops_the_wait_and_ends_the_session() {
    let server = server(
        |_| answer(json!("b1"), vec![execute(&[], &[])]),
        Duration::from_millis(400),
    );
    let mut session = session(&server).await;
    let canceller = session.canceller();
    let run = tokio::spawn(async move {
        let result = session.run("SELECT 2", new_sink(10).0).await;
        (session, result)
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    canceller.cancel().await.unwrap();
    let (mut session, result) = run.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Canceled);
    assert_eq!(error.message, text::LIBSQL_CANCELED);
    assert_eq!(session.transaction_state().await, TransactionState::Unknown);
    let error = session.run("SELECT 3", new_sink(10).0).await.unwrap_err();
    assert!(error.closes_session());
}
