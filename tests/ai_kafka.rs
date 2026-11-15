//! The AI completion of Kafka consoles: the guide of the language, the focus on the topic
//! of the statement, and the limit of the context when a cluster has many topics.

use super::*;
use sqln::catalog::{Catalog, ColumnNote, Relation, Schema};
use sqln::connector::StatementEnd;
use sqln::connector::kafka::GUIDE;
use sqln::sql::Dialect;

fn kafka_context(prefix: &str, suffix: &str) -> Context {
    Context {
        prefix: prefix.into(),
        original: String::new(),
        suffix: suffix.into(),
        dialect: Dialect::Kafka,
        schema: None,
        instruction: None,
    }
}

fn topic(name: &str, paths: &[&str]) -> Object {
    Object {
        schema: "topics".into(),
        name: name.into(),
        kind: "Topic".into(),
        metadata: paths
            .iter()
            .map(|path| json!({"section":"columns","values":{"path":path,"type":"string"}}))
            .collect(),
        targets: Vec::new(),
    }
}

fn group(name: &str) -> Object {
    Object {
        schema: "groups".into(),
        name: name.into(),
        kind: "Group".into(),
        metadata: Vec::new(),
        targets: Vec::new(),
    }
}

fn cluster() -> Snapshot {
    snapshot(vec![
        topic("orders", &["value.customer.id", "value.status"]),
        topic("payments", &["value.amount"]),
        topic("payouts", &[]),
        topic("my topic", &["value.note"]),
        group("billing"),
    ])
}

fn system(prepared: &sqln::ai::Prepared) -> String {
    prepared.body["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn user(prepared: &sqln::ai::Prepared) -> Value {
    serde_json::from_str(prepared.body["messages"][1]["content"].as_str().unwrap()).unwrap()
}

fn names(prepared: &sqln::ai::Prepared) -> Vec<String> {
    user(prepared)["metadata"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|object| object["name"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn each_dialect_has_its_language_in_the_system_message() {
    let server = Server::new("1", Duration::ZERO);
    let p = server.provider();
    let kafka = ai::prepare(&p, &kafka_context("SELECT ", ""), &cluster()).unwrap();
    let kafka_system = system(&kafka);
    assert!(
        kafka_system.starts_with("You complete SQL"),
        "{kafka_system}"
    );
    assert!(kafka_system.ends_with(GUIDE), "the guide follows the rules");
    assert!(kafka_system.contains("exactly as metadata writes them, with the same case"));
    assert!(
        !kafka_system.contains("Agent tools"),
        "the editor gets no rule of the agent tools"
    );
    assert_eq!(kafka_system, ai::instruction(Dialect::Kafka));

    let sqlite = ai::prepare(&p, &context(), &snapshot(Vec::new())).unwrap();
    let sqlite_system = system(&sqlite);
    assert!(
        sqlite_system.contains("Write SQLite SQL"),
        "{sqlite_system}"
    );
    assert!(!sqlite_system.contains("SQLN Kafka language"));
    for dialect in [Dialect::Postgres, Dialect::MySql, Dialect::DuckDb] {
        assert!(!ai::instruction(dialect).contains("SQLN Kafka language"));
    }
}

#[test]
fn a_known_topic_at_the_cursor_goes_alone_with_its_fields() {
    let server = Server::new("1", Duration::ZERO);
    let p = server.provider();
    let snap = cluster();
    for (prefix, suffix, wanted) in [
        ("SELECT value.", " FROM orders WHERE key = 'a'", "orders"),
        ("CONSUME orders WHERE value.", "", "orders"),
        ("TAIL payments WHERE ", "", "payments"),
        ("PARTITIONS orders", "", "orders"),
        (
            "SELECT * FROM payments;\nSELECT key FROM orders WHERE ",
            "\nLIMIT 5;",
            "orders",
        ),
        ("SELECT * FROM \"my topic\" WHERE ", "", "my topic"),
    ] {
        let prepared = ai::prepare(&p, &kafka_context(prefix, suffix), &snap).unwrap();
        assert_eq!(names(&prepared), [wanted], "{prefix}");
        let context = &user(&prepared)["context"];
        assert_eq!(
            context["focus"],
            json!([format!("topics.{wanted}")]),
            "{prefix}"
        );
        assert_eq!(
            prepared.omitted, 0,
            "{prefix}: the other topics are not omitted"
        );
        assert!(!prepared.reduced, "{prefix}");
    }
    let prepared = ai::prepare(&p, &kafka_context("SELECT ", " FROM orders"), &snap).unwrap();
    let text = prepared.body["messages"][1]["content"].as_str().unwrap();
    assert!(text.contains("value.customer.id"), "{text}");
    assert!(!text.contains("value.amount"), "{text}");
}

#[test]
fn without_a_known_topic_the_typed_start_comes_first() {
    let server = Server::new("1", Duration::ZERO);
    let p = server.provider();
    let snap = cluster();
    let prepared = ai::prepare(&p, &kafka_context("SELECT * FROM PAY", ""), &snap).unwrap();
    assert_eq!(
        names(&prepared),
        ["payments", "payouts", "my topic", "orders", "billing"],
        "the typed start first, then the topics, then the groups"
    );
    assert!(user(&prepared)["context"].get("focus").is_none());
    let prepared = ai::prepare(&p, &kafka_context("SELECT * FROM ", ""), &snap).unwrap();
    assert_eq!(
        names(&prepared),
        ["my topic", "orders", "payments", "payouts", "billing"]
    );
    assert_eq!(prepared.omitted, 0);
}

#[test]
fn a_kafka_request_sends_no_empty_fields() {
    let server = Server::new("1", Duration::ZERO);
    let p = server.provider();
    for context in [
        kafka_context("SELECT * FROM ", ""),
        kafka_context("SELECT ", " FROM orders"),
    ] {
        let prepared = ai::prepare(&p, &context, &cluster()).unwrap();
        let text = prepared.body["messages"][1]["content"].as_str().unwrap();
        for empty in ["\"targets\"", "\"schema\":null", "\"metadata\":[]"] {
            assert!(!text.contains(empty), "{empty}: {text}");
        }
    }
    let prepared = ai::prepare(&p, &context(), &snapshot(Vec::new())).unwrap();
    assert_eq!(
        user(&prepared)["context"]["schema"],
        "main",
        "a known schema stays"
    );
}

#[test]
fn the_guide_and_many_topics_fit_the_context_window() {
    let server = Server::new("1", Duration::ZERO);
    let mut p = server.provider();
    p.context_window_tokens = 16_384;
    let mut objects: Vec<Object> = (0..3000)
        .map(|n| topic(&format!("team-{n:04}.events"), &[]))
        .collect();
    objects.push(topic("zz-target", &[]));
    let snap = snapshot(objects);
    let context = kafka_context("SELECT * FROM zz-t", "");
    let a = ai::prepare(&p, &context, &snap).unwrap();
    let b = ai::prepare(&p, &context, &snap).unwrap();
    assert_eq!(a.body, b.body, "one input gives one body");
    let limit = p.context_window_tokens - p.max_output_tokens;
    assert!(
        a.body.to_string().len() <= limit,
        "the body fits the window"
    );
    assert!(a.omitted > 0 && a.reduced, "a reduced context says so");
    assert_eq!(names(&a)[0], "zz-target", "the typed start stays");
    assert!(system(&a).ends_with(GUIDE), "the guide always goes");
}

#[test]
fn the_focus_topic_belongs_to_kafka_only() {
    let mut sqlite = context();
    sqlite.prefix = "SELECT * FROM users WHERE ".into();
    assert_eq!(ai::focus_topic(&sqlite), None);
    assert_eq!(
        ai::focus_topic(&kafka_context("SELECT * FROM orders WHERE ", "")),
        Some("orders".into())
    );
    assert_eq!(ai::focus_topic(&kafka_context("SELECT * FROM ", "")), None);
    assert_eq!(
        ai::focus_topic(&kafka_context("SELECT * FROM orders WHERE key = 'open", "")),
        Some("orders".into()),
        "an open quote keeps the topic"
    );
}

fn kafka_app(dialect: Dialect, provider: Provider) -> App {
    let handler: crate::common::mock::RunHandler =
        Arc::new(|_, _| Box::pin(async { Ok(StatementEnd::default()) }));
    let connector = crate::common::mock::MockConnector::new(dialect, handler);
    let relation = |name: &str| Relation {
        name: name.into(),
        ..Relation::default()
    };
    connector.set_catalog(Catalog {
        schemas: vec![
            Schema {
                name: "topics".into(),
                relations: vec![relation("orders"), relation("payments")],
                ..Schema::default()
            },
            Schema {
                name: "groups".into(),
                relations: vec![relation("billing")],
                ..Schema::default()
            },
        ],
        current_schema: Some("topics".into()),
    });
    let note = |detail: &str, values: &[&str]| ColumnNote {
        detail: detail.into(),
        values: values.iter().map(|value| (*value).to_owned()).collect(),
    };
    connector.set_columns(
        "orders",
        vec!["value.status".into(), "value.total".into()],
        vec![note("enum", &["'OPEN'", "'PAID'"]), note("double", &[])],
    );
    connector.set_columns(
        "payments",
        vec!["value.payer_iban".into()],
        vec![note("string", &[])],
    );
    App::new(
        Options {
            connector: Some(Arc::new(connector)),
            ai_completion: Preferences {
                enabled: Some(true),
                max_simultaneous: None,
                provider: Some(provider),
            },
            ..Options::default()
        },
        Rc::new(RefCell::new(MemoryClipboard::default())),
        Theme::plain(),
    )
}

#[test]
fn a_kafka_console_sends_the_guide_and_the_fields_of_its_topic() {
    let server = Server::new("value.status", Duration::ZERO);
    let mut app = kafka_app(Dialect::Kafka, server.provider());
    ghost_at_first_line(&mut app, "SELECT  FROM orders");
    let shown = screen(&mut app);
    assert!(shown.contains("SELECT value.status FROM orders"), "{shown}");
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert!(request.contains("SQLN Kafka language"), "the guide goes");
    assert!(
        request.contains("value.status") && request.contains("PAID"),
        "{request}"
    );
    assert!(!request.contains("payer_iban"), "no field of another topic");
    assert!(
        !request.contains("payments"),
        "the topic of the statement goes alone"
    );
}

#[test]
fn a_kafka_console_offers_the_ai_rows_and_redis_does_not() {
    let server = Server::new("1", Duration::ZERO);
    for (dialect, offered) in [(Dialect::Kafka, true), (Dialect::Redis, false)] {
        let mut app = kafka_app(dialect, server.provider());
        app.on_event(Event::Paste("SELECT ".into()));
        app.on_event(Event::Key(ctrl(' ')));
        let shown = screen(&mut app);
        assert_eq!(
            shown.contains("Complete with AI"),
            offered,
            "{dialect:?}: {shown}"
        );
    }
}
