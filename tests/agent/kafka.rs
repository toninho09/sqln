//! The MCP agent on a Kafka connection: the read policy, the syntax guide, the field
//! paths of a topic, the default scan, and the statement ends of a task reply.

use super::*;
use crate::common::mock::{MockConnector, RunHandler};
use sqln::catalog::ColumnNote;
use sqln::connector::StatementEnd;
use sqln::connector::kafka::GUIDE;
use sqln::grid::Value as Cell;
use std::sync::Arc;

fn kafka_harness() -> Harness {
    let handler: RunHandler = Arc::new(|_, mut sink| {
        Box::pin(async move {
            sink.set_columns(vec!["key".to_owned()]).await;
            sink.push([Cell::Text("order-1")].into_iter().collect())
                .await;
            let (rows, limit) = sink.finish().await;
            Ok(StatementEnd {
                rows,
                limit,
                has_columns: true,
                ..StatementEnd::default()
            })
        })
    });
    let connector = MockConnector::new(Dialect::Kafka, handler);
    connector.set_columns(
        "orders",
        vec!["value.status".into(), "value.total".into()],
        vec![
            ColumnNote {
                detail: "enum".into(),
                values: vec!["'OPEN'".into(), "'PAID'".into()],
            },
            ColumnNote {
                detail: "double".into(),
                values: Vec::new(),
            },
        ],
    );
    Harness::options(
        Options {
            connector: Some(Arc::new(connector)),
            ..Default::default()
        },
        KeyboardMode::Conventional,
    )
}

fn run(h: &mut Harness, sql: &str) -> Value {
    let job = h.query(sql);
    h.wait_job(&job)
}

#[test]
fn kafka_reads_pass_the_read_policy_and_other_commands_do_not() {
    for sql in [
        "TOPICS",
        "PARTITIONS orders",
        "GROUPS",
        "GROUP billing",
        "CONSUME orders FROM earliest LIMIT 5",
        "SELECT key, value.status FROM orders LIMIT 5;",
        "# a note\nSELECT key FROM orders WHERE value.total > 10 SCAN 100",
        // The runner refuses these reads outside a console, in every access level.
        "TAIL orders",
        "SELECT * FROM orders EMIT CHANGES",
    ] {
        assert!(
            agent::policy::validate(&[sql.into()], Dialect::Kafka).is_ok(),
            "{sql}"
        );
    }
    for sql in [
        "DELETE orders",
        "PRODUCE orders x",
        "SQLN.TOPIC orders 5",
        "",
    ] {
        let error = agent::policy::validate(&[sql.into()], Dialect::Kafka).unwrap_err();
        assert_eq!(error.code, "read_only_refused", "{sql}");
    }
    let error = agent::policy::validate(
        &["SELECT * FROM orders ORDER BY offset".into()],
        Dialect::Kafka,
    )
    .unwrap_err();
    assert_eq!(error.code, "invalid_sql");
    for part in ["SELECT accepts", "Position: byte", "syntax.Kafka"] {
        assert!(error.message.contains(part), "{part}: {}", error.message);
    }
}

#[test]
fn an_agent_reads_kafka_without_full_access_but_not_a_read_that_does_not_end() {
    let mut h = kafka_harness();
    h.authorize();
    assert_eq!(h.access()["access"], "propose");
    let reply = run(&mut h, "SELECT key FROM orders LIMIT 1");
    assert_eq!(reply["status"], "completed", "{reply}");
    assert!(reply.to_string().contains("order-1"), "{reply}");
    let reply = run(&mut h, "SELECT key FROM orders ORDER BY offset");
    assert!(reply.to_string().contains("syntax.Kafka"), "{reply}");
    for sql in ["TAIL orders", "SELECT * FROM orders EMIT CHANGES"] {
        let reply = run(&mut h, sql);
        assert_eq!(reply["status"], "failed", "{sql}: {reply}");
        assert!(reply.to_string().contains("does not end"), "{sql}: {reply}");
    }

    let mut h = kafka_harness();
    h.grant(3);
    assert_eq!(h.access()["access"], "full");
    let reply = run(&mut h, "TAIL orders");
    assert_eq!(reply["status"], "failed", "{reply}");
    assert!(reply.to_string().contains("does not end"), "{reply}");
}

#[test]
fn the_context_carries_the_kafka_guide_only_with_a_kafka_connection() {
    let mut h = kafka_harness();
    h.authorize();
    let context = h.tool(json!({"method":"get_context"})).unwrap();
    assert_eq!(context["connections"][0]["dialect"], "Kafka");
    assert_eq!(context["syntax"]["Kafka"], GUIDE);

    let mut h = Harness::new("no-kafka-guide");
    h.authorize();
    let context = h.tool(json!({"method":"get_context"})).unwrap();
    assert!(context.get("syntax").is_none(), "{context}");
}

#[test]
fn describe_reads_the_fields_of_a_kafka_topic() {
    let mut h = kafka_harness();
    h.authorize();
    let connection = h.connection();
    let fields = |h: &mut Harness, topic: &str| -> Value {
        let reply = h
            .tool(
                json!({"method":"describe","connection_id":connection,"schema":"topics",
                "object":{"name":topic,"kind":"table"}}),
            )
            .unwrap();
        let job = h.wait_job(reply["job_id"].as_str().unwrap());
        assert_eq!(job["status"], "completed", "{job}");
        assert_eq!(job["artifact_format"], "json", "{job}");
        serde_json::from_str(job["artifact"]["text"].as_str().unwrap()).unwrap()
    };
    let orders = fields(&mut h, "orders");
    assert_eq!(
        orders,
        json!({"topic":"orders","fields":[
            {"path":"value.status","type":"enum","values":["'OPEN'","'PAID'"]},
            {"path":"value.total","type":"double"},
        ]})
    );
    let empty = fields(&mut h, "payments");
    assert_eq!(empty["fields"], json!([]));
    assert!(
        empty["message"]
            .as_str()
            .unwrap()
            .contains("Schema Registry"),
        "{empty}"
    );
    let error = h
        .tool(
            json!({"method":"describe","connection_id":connection,"schema":"groups",
            "object":{"name":"billing","kind":"table"}}),
        )
        .unwrap_err();
    assert_eq!(error.code, "unsupported");
    assert!(error.message.contains("kind topic"), "{}", error.message);
    // The plain form: the kind topic, without a schema.
    let reply = h
        .tool(json!({"method":"describe","connection_id":connection,
            "object":{"name":"orders","kind":"topic"}}))
        .unwrap();
    let job = h.wait_job(reply["job_id"].as_str().unwrap());
    let topic: Value = serde_json::from_str(job["artifact"]["text"].as_str().unwrap()).unwrap();
    assert_eq!(topic["topic"], "orders", "{job}");
    // A topic has no schema, so the kind topic reads its fields with any schema.
    let reply = h
        .tool(
            json!({"method":"describe","connection_id":connection,"schema":"public",
            "object":{"name":"orders","kind":"topic"}}),
        )
        .unwrap();
    let job = h.wait_job(reply["job_id"].as_str().unwrap());
    assert_eq!(job["status"], "completed", "{job}");
    let topic: Value = serde_json::from_str(job["artifact"]["text"].as_str().unwrap()).unwrap();
    assert_eq!(topic["topic"], "orders", "{job}");
}

#[test]
fn the_guide_comes_once_per_session_and_again_on_request() {
    let mut h = kafka_harness();
    h.authorize();
    let first = h.tool(json!({"method":"get_context"})).unwrap();
    assert_eq!(first["syntax"]["Kafka"], GUIDE);
    let later = h.tool(json!({"method":"get_context"})).unwrap();
    let note = later["syntax"]["Kafka"].as_str().unwrap();
    assert!(note.contains("include_syntax"), "{note}");
    let again = h
        .tool(json!({"method":"get_context","include_syntax":true}))
        .unwrap();
    assert_eq!(again["syntax"]["Kafka"], GUIDE);
}

#[test]
fn the_context_names_production_and_read_only() {
    let mut h = kafka_harness();
    h.authorize();
    let context = h.tool(json!({"method":"get_context"})).unwrap();
    let connection = &context["connections"][0];
    assert_eq!(connection["production"], false, "{connection}");
    assert_eq!(connection["read_only"], false, "{connection}");
    assert!(
        connection["capabilities"].get("data_edit").is_none(),
        "{connection}"
    );
}

/// A Kafka source that records each statement. A read with `SCAN` ends at its scan with
/// no match, as a read of a topic whose messages are past the scan.
fn recording_harness() -> (Harness, Arc<std::sync::Mutex<Vec<String>>>) {
    recording_harness_with(sqln::exec::Settings::default())
}

fn recording_harness_with(
    settings: sqln::exec::Settings,
) -> (Harness, Arc<std::sync::Mutex<Vec<String>>>) {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let record = seen.clone();
    let handler: RunHandler = Arc::new(move |sql: String, mut sink| {
        record.lock().unwrap().push(sql.clone());
        Box::pin(async move {
            let progress = Arc::new(sqln::progress::ReadProgress::new(true));
            sink.track(progress.clone());
            sink.set_columns(vec!["key".to_owned()]).await;
            progress.add_scanned(10);
            progress.set_end(if sql.contains("SCAN") {
                sqln::progress::ReadEnd::Scan
            } else {
                sqln::progress::ReadEnd::Limit
            });
            let (rows, limit) = sink.finish().await;
            Ok(StatementEnd {
                rows,
                limit,
                has_columns: true,
                ..StatementEnd::default()
            })
        })
    });
    let h = Harness::options(
        Options {
            connector: Some(Arc::new(MockConnector::new(Dialect::Kafka, handler))),
            settings,
            ..Default::default()
        },
        KeyboardMode::Conventional,
    );
    (h, seen)
}

/// Runs the text of the user console and waits for its end.
fn run_user(h: &mut Harness, sql: &str) {
    h.app.on_event(crossterm::event::Event::Paste(sql.into()));
    h.key(crossterm::event::KeyCode::F(5));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while h.app.console().is_busy() || h.app.console().results.is_empty() {
        h.app.drain_events();
        assert!(std::time::Instant::now() < deadline, "the run did not end");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn max_scan_of_the_settings_bounds_user_and_agent_reads() {
    let settings = sqln::exec::Settings {
        max_scan: 500,
        ..sqln::exec::Settings::default()
    };
    let (mut h, seen) = recording_harness_with(settings);
    // A user read without SCAN gets Max scan, and Messages says so.
    run_user(&mut h, "SELECT * FROM orders");
    assert_eq!(
        seen.lock().unwrap().last().unwrap(),
        "SELECT * FROM orders SCAN 500"
    );
    let added = h
        .app
        .console()
        .messages
        .iter()
        .filter(|message| message.text.contains("SQLN added SCAN 500"))
        .count();
    assert_eq!(added, 1, "{:?}", h.app.console().messages);
    // The agent gets the same number.
    h.authorize();
    let reply = run(&mut h, "SELECT * FROM orders LIMIT 5");
    assert_eq!(reply["default_scan"], 500, "{reply}");
    assert_eq!(
        seen.lock().unwrap().last().unwrap(),
        "SELECT * FROM orders LIMIT 5 SCAN 500"
    );
}

#[test]
fn a_user_read_with_its_own_scan_keeps_its_text() {
    let (mut h, seen) = recording_harness();
    run_user(&mut h, "SELECT * FROM orders SCAN 7");
    assert_eq!(
        seen.lock().unwrap().last().unwrap(),
        "SELECT * FROM orders SCAN 7"
    );
    assert!(
        !h.app
            .console()
            .messages
            .iter()
            .any(|message| message.text.contains("SQLN added SCAN")),
        "{:?}",
        h.app.console().messages
    );
}

#[test]
fn an_agent_read_without_a_bound_gets_the_default_scan_and_says_how_it_ended() {
    let (mut h, seen) = recording_harness();
    h.authorize();
    let reply = run(&mut h, "SELECT * FROM orders");
    assert_eq!(reply["status"], "completed", "{reply}");
    assert_eq!(
        reply["default_scan"],
        sqln::connector::kafka::command::DEFAULT_SCAN,
        "{reply}"
    );
    let ran = seen.lock().unwrap().last().unwrap().clone();
    assert!(ran.ends_with("SCAN 100000"), "{ran}");
    let read = &reply["results"][0]["read"];
    assert_eq!(read["end"], "scan", "{reply}");
    assert_eq!(read["matched"], 0, "{reply}");
    assert!(
        reply["next_action"]
            .as_str()
            .unwrap()
            .contains("raise SCAN"),
        "{reply}"
    );
    // LIMIT counts only the matches, and a start does not end the read: both get the SCAN.
    for sql in [
        "SELECT * FROM orders LIMIT 5",
        "SELECT * FROM orders START AT EARLIEST",
    ] {
        let reply = run(&mut h, sql);
        assert_eq!(reply["default_scan"], 100_000, "{reply}");
        let ran = seen.lock().unwrap().last().unwrap().clone();
        assert_eq!(ran, format!("{sql} SCAN 100000"));
    }
    // A read with its own SCAN keeps its text.
    let reply = run(&mut h, "SELECT * FROM orders SCAN 50");
    assert!(reply["default_scan"].is_null(), "{reply}");
    assert_eq!(
        seen.lock().unwrap().last().unwrap(),
        "SELECT * FROM orders SCAN 50"
    );
}

#[test]
fn a_statement_end_carries_its_notices_and_the_limit_of_the_grid() {
    let handler: RunHandler = Arc::new(|_, mut sink| {
        Box::pin(async move {
            sink.notices(
                vec!["NOTICE: \u{1b}[31mfirst".to_owned(), "x".repeat(4000)],
                0,
            )
            .await;
            sink.set_columns(vec!["a".to_owned()]).await;
            let (rows, _) = sink.finish().await;
            Ok(StatementEnd {
                rows,
                limit: sqln::sink::ReadLimit::RowCap,
                has_columns: true,
                ..StatementEnd::default()
            })
        })
    });
    let mut h = Harness::options(
        Options {
            connector: Some(Arc::new(MockConnector::new(Dialect::Sqlite, handler))),
            ..Default::default()
        },
        KeyboardMode::Conventional,
    );
    h.authorize();
    let reply = run(&mut h, "SELECT 1");
    let statement = &reply["statements"][0];
    assert_eq!(statement["limit"], "row_cap", "{reply}");
    let notices = statement["notices"].as_array().unwrap();
    let first = notices[0].as_str().unwrap();
    assert!(
        first.contains("first") && !first.contains('\u{1b}'),
        "{first}"
    );
    // The notices of one statement stay within their byte limit.
    let bytes: usize = notices
        .iter()
        .map(|text| text.as_str().unwrap().len())
        .sum();
    assert!(bytes <= 2048, "{bytes}");
}
