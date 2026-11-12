// Tests of Kafka and Schema Registry. The tests of text, forms, and settings need no
// server. The other tests need `scripts/test-env.sh`, and they print "skipped" without
// its variables.
//
// Each server test uses its own topics and groups, so the tests can run at the same time.

mod common;
#[path = "kafka/select.rs"]
mod select;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::Event;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use rdkafka::admin::{AdminClient, AdminOptions, NewTopic, TopicReplication};
use rdkafka::client::DefaultClientContext;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{BaseConsumer, CommitMode, Consumer};
use rdkafka::message::{Header, OwnedHeaders};
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::{Offset, TopicPartitionList};
use sqln::app::{App, Options};
use sqln::catalog::{Catalog, Columns, Relation, RelationKind, Schema};
use sqln::clipboard::MemoryClipboard;
use sqln::complete::suggest;
use sqln::connection::{Definition, Source};
use sqln::connector::kafka::{GROUPS, KafkaConnector, MESSAGE_COLUMNS, TOPICS};
use sqln::connector::target::{Engine, Sasl, Secret, parse_uri};
use sqln::connector::{Connector, network_connector};
use sqln::editor::KeyboardMode;
use sqln::exec::{
    ConsoleId, ExecEvent, ExecutionEnd, Manager, RunOptions, Settings, StatementStatus,
};
use sqln::form::{ConnectionForm, Field, Kind};
use sqln::grid::{Budget, Row, Value};
use sqln::sql::{Dialect, changes_state, is_continuous, split};
use sqln::store::config::{KindName, to_definition, to_entry};
use sqln::ui::theme::Theme;
use tokio::sync::mpsc::Receiver;

// The tests below need no server.

#[test]
fn a_kafka_text_reads_sasl_and_the_registry() {
    let target = parse_uri(
        "kafka://ana:pw@broker.example:9093?sasl=scram-sha-256&registry=https://sr.example",
    )
    .unwrap();
    assert_eq!(target.engine, Engine::Kafka);
    assert_eq!(target.user, "ana");
    assert_eq!(target.kafka.sasl, Sasl::ScramSha256);
    let registry = target.kafka.registry.unwrap();
    assert_eq!(registry.url, "https://sr.example");
    assert!(matches!(
        target.tls,
        sqln::connector::target::Tls::Verify(_)
    ));

    let local = parse_uri("kafka://127.0.0.1").unwrap();
    assert_eq!(local.kafka.sasl, Sasl::ScramSha512, "the default mechanism");
    assert_eq!(local.label(), "127.0.0.1:9092");
    assert!(matches!(local.tls, sqln::connector::target::Tls::Disabled));
    let always = parse_uri("kafkas://127.0.0.1").unwrap();
    assert!(matches!(
        always.tls,
        sqln::connector::target::Tls::Verify(_)
    ));
}

#[test]
fn a_kafka_text_refuses_what_kafka_does_not_have() {
    for (text, part) in [
        ("kafka://127.0.0.1/db", "no database"),
        ("kafka:///?socket=/tmp/k.sock", "socket"),
        ("kafka://127.0.0.1?sasl=gssapi", "sasl must be"),
        ("redis://127.0.0.1?sasl=plain", "only for kafka"),
        ("kafkas://127.0.0.1?sslmode=disable", "always uses TLS"),
    ] {
        let error = parse_uri(text).unwrap_err();
        assert!(error.contains(part), "{text}: {error}");
    }
}

fn kafka_form() -> ConnectionForm {
    let mut form = ConnectionForm::new();
    form.kind = Kind::Kafka;
    form.focus_on(Field::Name);
    form.on_paste("events");
    form.focus_on(Field::Host);
    form.on_paste("127.0.0.1");
    form
}

#[test]
fn the_form_of_kafka_has_sasl_and_registry_fields_and_no_database() {
    let form = kafka_form();
    let fields = form.fields();
    for field in [
        Field::Sasl,
        Field::RegistryUrl,
        Field::RegistryUser,
        Field::RegistryPassword,
    ] {
        assert!(fields.contains(&field), "{field:?}");
    }
    assert!(!fields.contains(&Field::Database));
    assert!(!fields.contains(&Field::Schemas));
    let definition = form.definition().unwrap();
    let Source::Network(target) = &definition.source else {
        panic!("not a network source");
    };
    assert_eq!(target.engine, Engine::Kafka);
    assert!(target.address.is_local());
    assert_eq!(definition.summary(), "Kafka 127.0.0.1:9092 · no TLS");
}

#[test]
fn the_form_keeps_the_registry_and_its_session_password() {
    let mut form = kafka_form();
    form.focus_on(Field::Sasl);
    form.on_key(common::key(crossterm::event::KeyCode::Right), false);
    assert_eq!(
        form.sasl,
        Sasl::Plain,
        "the choice turns after the last item"
    );
    form.focus_on(Field::RegistryUrl);
    form.on_paste("http://sr:8081");
    form.focus_on(Field::RegistryUser);
    form.on_paste("key");
    form.focus_on(Field::RegistryPassword);
    form.on_paste("secret");
    let definition = form.definition().unwrap();
    let Source::Network(target) = &definition.source else {
        panic!("not a network source");
    };
    let registry = target.kafka.registry.as_ref().unwrap();
    assert_eq!(registry.user, "key");
    assert_eq!(
        registry.password.as_ref().map(Secret::expose),
        Some("secret")
    );
    let edited = ConnectionForm::editing(1, &definition, false);
    assert_eq!(edited.definition().unwrap(), definition);
    let stored = definition.without_password();
    let Source::Network(stored) = &stored.source else {
        panic!("not a network source");
    };
    assert_eq!(stored.kafka.registry.as_ref().unwrap().password, None);
}

#[test]
fn the_form_refuses_a_registry_without_a_web_address() {
    let mut form = kafka_form();
    form.focus_on(Field::RegistryUrl);
    form.on_paste("sr:8081");
    let error = form.definition().unwrap_err();
    assert_eq!(error.field, Field::RegistryUrl);
    let mut form = kafka_form();
    form.focus_on(Field::RegistryUser);
    form.on_paste("key");
    assert_eq!(form.definition().unwrap_err().field, Field::RegistryUrl);
}

#[test]
fn a_saved_kafka_connection_keeps_sasl_and_registry_without_secrets() {
    let mut form = kafka_form();
    form.focus_on(Field::RegistryUrl);
    form.on_paste("http://sr:8081");
    form.focus_on(Field::RegistryUser);
    form.on_paste("key");
    form.focus_on(Field::RegistryPassword);
    form.on_paste("secret");
    let definition = form.definition().unwrap();
    let entry = to_entry("k1", &definition);
    assert_eq!(entry.kind, KindName::Kafka);
    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains(r#""kind":"kafka""#), "{json}");
    assert!(json.contains(r#""sasl":"scram-sha-512""#), "{json}");
    assert!(
        json.contains(r#""schemaRegistry":{"url":"http://sr:8081","username":"key"}"#),
        "{json}"
    );
    assert!(!json.contains("secret"), "no password in the file: {json}");
    let mut read = to_definition(&entry, "/base".into());
    read.base = None;
    assert_eq!(read, definition.without_password());
}

#[test]
fn the_settings_refuse_kafka_fields_on_other_engines() {
    let refused = |config: &str| {
        let dir = common::temp_dir(&format!("kafka-store-{}", uuid::Uuid::new_v4()));
        let store = sqln::store::Store {
            global: sqln::store::Root::global(dir.join("global")),
            project: None,
        };
        std::fs::create_dir_all(&store.global.dir).unwrap();
        std::fs::write(store.global.config_file(), config).unwrap();
        sqln::store::config::load(&store)
            .config
            .unwrap_err()
            .message
    };
    let message = refused(
        r#"{"version":1,"connections":[{"id":"r","name":"r","kind":"redis","host":"h","sasl":"plain"}]}"#,
    );
    assert!(message.contains("only for kafka"), "{message}");
    let message = refused(
        r#"{"version":1,"connections":[{"id":"k","name":"k","kind":"kafka","host":"h","database":"1"}]}"#,
    );
    assert!(message.contains("no database"), "{message}");
    let message = refused(
        r#"{"version":1,"connections":[{"id":"k","name":"k","kind":"kafka","host":"h","schemaRegistry":{"url":"sr"}}]}"#,
    );
    assert!(message.contains("http"), "{message}");
}

#[test]
fn each_line_is_one_kafka_command_and_only_tail_is_continuous() {
    let text = "TOPICS\n# a note\nCONSUME orders WHERE key = 'a;b'\n\nTAIL orders\n";
    let found: Vec<&str> = split(text, Dialect::Kafka)
        .iter()
        .map(|statement| statement.text(text))
        .collect();
    assert_eq!(
        found,
        ["TOPICS", "CONSUME orders WHERE key = 'a;b'", "TAIL orders"]
    );
    assert!(is_continuous("tail orders", Dialect::Kafka));
    assert!(is_continuous("# note\nTAIL orders", Dialect::Kafka));
    assert!(!is_continuous("CONSUME orders", Dialect::Kafka));
    assert!(!is_continuous("TAIL orders", Dialect::Postgres));
    for read in [
        "TOPICS",
        "CONSUME t",
        "GROUP g",
        "GROUPS",
        "PARTITIONS t",
        "TAIL t",
    ] {
        assert!(!changes_state(read, Dialect::Kafka), "{read}");
    }
    assert!(
        changes_state("DELETE t", Dialect::Kafka),
        "an unknown command asks first"
    );
}

fn catalog() -> Catalog {
    let relation = |name: &str| Relation {
        name: name.to_owned(),
        kind: RelationKind::Table,
        columns: Columns::NotLoaded,
        foreign_keys: Vec::new(),
        referencing: Vec::new(),
    };
    Catalog {
        schemas: vec![
            Schema {
                name: TOPICS.to_owned(),
                relations: vec![relation("orders"), relation("order events")],
                ..Schema::default()
            },
            Schema {
                name: GROUPS.to_owned(),
                relations: vec![relation("billing")],
                ..Schema::default()
            },
        ],
        current_schema: Some(TOPICS.to_owned()),
    }
}

fn labels(text: &str) -> Vec<(String, String)> {
    let catalog = catalog();
    suggest(text, text.len(), Dialect::Kafka, Some(&catalog))
        .items
        .into_iter()
        .map(|item| (item.label, item.insert))
        .collect()
}

#[test]
fn the_list_holds_commands_then_names_then_clauses() {
    let commands: Vec<String> = labels("CO").into_iter().map(|item| item.0).collect();
    assert_eq!(commands, ["CONSUME"]);
    let topics = labels("CONSUME ord");
    assert!(topics.contains(&("orders".to_owned(), "orders".to_owned())));
    assert!(
        topics.contains(&("order events".to_owned(), "\"order events\"".to_owned())),
        "a name with a space gets quotes: {topics:?}"
    );
    assert_eq!(
        labels("GROUP bi"),
        [("billing".to_owned(), "billing".to_owned())]
    );
    let clauses: Vec<String> = labels("CONSUME orders WH")
        .into_iter()
        .map(|item| item.0)
        .collect();
    assert_eq!(clauses, ["WHERE"]);
    assert!(labels("TOPICS x").is_empty(), "TOPICS takes no name");
    assert_eq!(labels("SEL"), [("SELECT".to_owned(), "SELECT".to_owned())]);
}

// The tests below need a server.

fn kafka_uri() -> Option<String> {
    match std::env::var("SQLN_TEST_KAFKA") {
        Ok(uri) => Some(uri),
        Err(_) => {
            println!("skipped: SQLN_TEST_KAFKA is not set");
            None
        }
    }
}

fn registry_url() -> Option<String> {
    match std::env::var("SQLN_TEST_SCHEMA_REGISTRY") {
        Ok(url) => Some(url),
        Err(_) => {
            println!("skipped: SQLN_TEST_SCHEMA_REGISTRY is not set");
            None
        }
    }
}

fn bootstrap(uri: &str) -> String {
    uri.trim_start_matches("kafka://").to_owned()
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn unique(name: &str) -> String {
    format!("sqln-{name}-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

fn create_topic(uri: &str, name: &str, partitions: i32) {
    let admin: AdminClient<DefaultClientContext> = ClientConfig::new()
        .set("bootstrap.servers", bootstrap(uri))
        .create()
        .unwrap();
    runtime().block_on(async {
        let topic = NewTopic::new(name, partitions, TopicReplication::Fixed(1));
        let results = admin
            .create_topics(&[topic], &AdminOptions::new())
            .await
            .unwrap();
        for result in results {
            result.unwrap();
        }
    });
}

/// One message to produce.
struct Out<'a> {
    partition: i32,
    key: Option<&'a [u8]>,
    value: Option<&'a [u8]>,
    time: Option<i64>,
    header: Option<(&'a str, &'a str)>,
}

impl<'a> Out<'a> {
    fn new(partition: i32, key: &'a str, value: &'a [u8]) -> Self {
        Self {
            partition,
            key: Some(key.as_bytes()),
            value: Some(value),
            time: None,
            header: None,
        }
    }
}

fn produce(uri: &str, topic: &str, messages: &[Out<'_>]) {
    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", bootstrap(uri))
        .set("message.timeout.ms", "30000")
        .create()
        .unwrap();
    runtime().block_on(async {
        let sends = messages.iter().map(|message| {
            let mut record = FutureRecord::<[u8], [u8]>::to(topic).partition(message.partition);
            if let Some(key) = message.key {
                record = record.key(key);
            }
            if let Some(value) = message.value {
                record = record.payload(value);
            }
            if let Some(time) = message.time {
                record = record.timestamp(time);
            }
            if let Some((name, value)) = message.header {
                record = record.headers(OwnedHeaders::new().insert(Header {
                    key: name,
                    value: Some(value),
                }));
            }
            producer.send(record, Duration::from_secs(30))
        });
        for result in futures_util::future::join_all(sends).await {
            result.map_err(|(error, _)| error).unwrap();
        }
    });
}

/// Lists the groups of the cluster. A group list creates no group.
fn group_names(uri: &str) -> Vec<String> {
    let consumer: BaseConsumer = ClientConfig::new()
        .set("bootstrap.servers", bootstrap(uri))
        .set("group.id", "sqln-test-admin")
        .create()
        .unwrap();
    let list = consumer
        .fetch_group_list(None, Duration::from_secs(10))
        .unwrap();
    let mut names: Vec<String> = list.groups().iter().map(|g| g.name().to_owned()).collect();
    names.sort();
    names
}

fn texts(row: &Row) -> Vec<String> {
    row.cells()
        .map(|value| match value {
            Value::Null => "NULL".to_owned(),
            Value::Text(text) | Value::Number(text) => text.to_string(),
            Value::Binary(_) => "BYTES".to_owned(),
        })
        .collect()
}

#[derive(Debug, Default)]
struct Run {
    columns: Vec<Vec<String>>,
    rows: Vec<Row>,
    notices: Vec<String>,
    statuses: Vec<StatementStatus>,
    end: Option<ExecutionEnd>,
}

impl Run {
    fn grid(&self) -> Vec<Vec<String>> {
        self.rows.iter().map(texts).collect()
    }

    /// Returns one column of each row.
    fn column(&self, name: &str) -> Vec<String> {
        let index = self.columns[0]
            .iter()
            .position(|column| column == name)
            .unwrap();
        self.grid()
            .into_iter()
            .map(|row| row[index].clone())
            .collect()
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
    fn open(uri: &str) -> Self {
        Self::with(network_connector(parse_uri(uri).unwrap()))
    }

    fn with(connector: Arc<dyn Connector>) -> Self {
        let settings = Settings::default();
        let budget = Arc::new(Budget::new(settings.budget_bytes));
        let (mut manager, events) = Manager::new(budget).unwrap();
        let connection = manager.add_connection(connector, settings);
        let console = manager.open_console(connection).unwrap();
        Self {
            manager,
            events,
            console,
        }
    }

    fn next(&mut self) -> ExecEvent {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Ok(event) = self.events.try_recv() {
                return event;
            }
            assert!(Instant::now() < deadline, "no event arrived");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn collect(&mut self, execution: u64) -> Run {
        let mut run = Run::default();
        loop {
            match self.next() {
                ExecEvent::Columns { columns, .. } => run.columns.push(columns),
                ExecEvent::Rows { rows, .. } => run.rows.extend(rows),
                ExecEvent::Notice { text, .. } => run.notices.push(text),
                ExecEvent::StatementFinished { status, .. } => run.statuses.push(status),
                ExecEvent::ExecutionFinished {
                    execution: finished,
                    end,
                    ..
                } if finished == execution => {
                    run.end = Some(end);
                    return run;
                }
                _ => {}
            }
        }
    }

    fn run(&mut self, statement: &str) -> Run {
        let execution = self.manager.run(
            self.console,
            vec![statement.to_owned()],
            RunOptions::default(),
        );
        self.collect(execution)
    }

    fn run_ok(&mut self, statement: &str) -> Run {
        let run = self.run(statement);
        assert_eq!(
            run.end,
            Some(ExecutionEnd::Completed),
            "{statement}: {run:?}"
        );
        run
    }

    fn start_live(&mut self, statement: &str) -> u64 {
        let options = RunOptions {
            live: true,
            ..RunOptions::default()
        };
        self.manager
            .run(self.console, vec![statement.to_owned()], options)
    }

    fn wait_columns(&mut self) -> Vec<String> {
        loop {
            match self.next() {
                ExecEvent::Columns { columns, .. } => return columns,
                ExecEvent::ExecutionFinished { end, message, .. } => {
                    panic!("the run ended: {end:?} {message:?}")
                }
                _ => {}
            }
        }
    }

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
}

#[test]
fn topics_and_partitions_show_the_cluster() {
    let Some(uri) = kafka_uri() else { return };
    let topic = unique("cluster");
    create_topic(&uri, &topic, 2);
    produce(
        &uri,
        &topic,
        &[
            Out::new(0, "a", b"1"),
            Out::new(1, "b", b"2"),
            Out::new(1, "c", b"3"),
        ],
    );
    let mut h = Harness::open(&uri);
    let run = h.run_ok("TOPICS");
    assert_eq!(
        run.columns[0],
        ["topic", "partitions", "replicas", "internal"]
    );
    let row = run.grid().into_iter().find(|row| row[0] == topic).unwrap();
    assert_eq!(row, [topic.clone(), "2".into(), "1".into(), "false".into()]);
    let run = h.run_ok(&format!("PARTITIONS {topic}"));
    let rows: Vec<Vec<String>> = run
        .grid()
        .into_iter()
        .map(|row| {
            vec![
                row[0].clone(),
                row[4].clone(),
                row[5].clone(),
                row[6].clone(),
            ]
        })
        .collect();
    assert_eq!(rows, [["0", "0", "1", "1"], ["1", "0", "2", "2"]]);
    let run = h.run("PARTITIONS sqln-no-such-topic");
    assert_eq!(run.end, Some(ExecutionEnd::Failed));
    assert!(!run.error().is_empty());
}

#[test]
fn consume_reads_to_the_end_of_each_partition_and_creates_no_group() {
    let Some(uri) = kafka_uri() else { return };
    let topic = unique("consume");
    create_topic(&uri, &topic, 2);
    let mut with_header = Out::new(0, "k1", b"first");
    with_header.header = Some(("source", "web"));
    let tombstone = Out {
        partition: 1,
        key: Some(b"k3"),
        value: None,
        time: None,
        header: None,
    };
    produce(
        &uri,
        &topic,
        &[with_header, Out::new(0, "k2", &[0xff, 0x00]), tombstone],
    );
    let before = group_names(&uri);
    let mut h = Harness::open(&uri);
    let run = h.run_ok(&format!("CONSUME {topic}"));
    assert_eq!(run.columns[0], MESSAGE_COLUMNS);
    let mut grid = run.grid();
    grid.sort();
    assert_eq!(grid.len(), 3);
    assert_eq!(grid[0][..2], ["0", "0"]);
    assert_eq!(grid[0][3..], ["k1", "first", r#"{"source":"web"}"#, "NULL"]);
    assert_eq!(grid[1][4], "BYTES", "bytes that are not UTF-8 stay bytes");
    assert_eq!(grid[2][3..5], ["k3", "NULL"], "a tombstone has no value");
    assert!(grid[0][2].ends_with('Z'), "the time is UTC: {}", grid[0][2]);

    assert_eq!(h.run_ok(&format!("CONSUME {topic} LIMIT 1")).rows.len(), 1);
    assert!(
        h.run_ok(&format!("CONSUME {topic} FROM latest"))
            .rows
            .is_empty()
    );
    assert_eq!(
        h.run_ok(&format!("CONSUME {topic} PARTITION 0 FROM 1"))
            .column("key"),
        ["k2"]
    );
    let run = h.run(&format!("CONSUME {topic} PARTITION 7"));
    assert_eq!(run.end, Some(ExecutionEnd::Failed));
    assert!(run.error().contains("no partition"), "{run:?}");
    let run = h.run(&format!("CONSUME {topic} FROM yesterday"));
    assert_eq!(run.end, Some(ExecutionEnd::Failed));

    let after = group_names(&uri);
    assert_eq!(after, before, "a read creates no consumer group");
    assert!(!after.iter().any(|name| name == "sqln-read"));
}

#[test]
fn a_filter_keeps_the_matching_messages_and_reports_the_scan() {
    let Some(uri) = kafka_uri() else { return };
    let topic = unique("filter");
    create_topic(&uri, &topic, 1);
    produce(
        &uri,
        &topic,
        &[
            Out::new(
                0,
                "order-1",
                br#"{"total": 5, "customer": {"name": "Ana"}}"#,
            ),
            Out::new(
                0,
                "order-2",
                br#"{"total": 50, "customer": {"name": "Bia"}}"#,
            ),
            Out::new(
                0,
                "refund-3",
                br#"{"total": 70, "customer": {"name": "Caio"}}"#,
            ),
            Out::new(0, "order-4", b"plain text"),
        ],
    );
    let mut h = Harness::open(&uri);
    let run = h.run_ok(&format!(
        "CONSUME {topic} WHERE value.total > 10 AND key MATCHES '^order-'"
    ));
    assert_eq!(run.column("key"), ["order-2"]);
    assert!(
        run.notices
            .iter()
            .any(|notice| notice == "Scanned 4 messages; 1 matched."),
        "{:?}",
        run.notices
    );
    let run = h.run_ok(&format!(
        "CONSUME {topic} WHERE value.customer.name = 'Caio' OR value CONTAINS 'PLAIN'"
    ));
    assert_eq!(run.column("key"), ["refund-3", "order-4"]);
    let run = h.run_ok(&format!("CONSUME {topic} WHERE key = 'order-1' SCAN 2"));
    assert_eq!(run.column("key"), ["order-1"]);
    assert!(
        run.notices
            .iter()
            .any(|notice| notice.starts_with("Scanned 2 messages"))
    );
    let run = h.run(&format!("CONSUME {topic} WHERE key MATCHES '('"));
    assert_eq!(run.end, Some(ExecutionEnd::Failed));
    assert!(run.error().contains("regular expression"), "{run:?}");
}

#[test]
fn from_a_time_starts_at_the_first_message_at_that_time() {
    let Some(uri) = kafka_uri() else { return };
    let topic = unique("time");
    create_topic(&uri, &topic, 1);
    let base = 1_700_000_000_000_i64;
    let mut messages = Vec::new();
    for (index, key) in ["a", "b", "c"].iter().enumerate() {
        let mut message = Out::new(0, key, b"v");
        message.time = Some(base + 60_000 * index as i64);
        messages.push(message);
    }
    produce(&uri, &topic, &messages);
    let mut h = Harness::open(&uri);
    // 1700000060000 ms is 2023-11-14T22:14:20Z.
    let run = h.run_ok(&format!("CONSUME {topic} FROM @2023-11-14T22:14:20Z"));
    assert_eq!(run.column("key"), ["b", "c"]);
    assert_eq!(run.column("timestamp")[0], "2023-11-14T22:14:20.000Z");
    let selected = h.run_ok(&format!(
        "SELECT key FROM {topic} START AT TIMESTAMP '2023-11-14T22:14:20Z';"
    ));
    assert_eq!(selected.column("key"), run.column("key"));
    assert!(
        h.run_ok(&format!("CONSUME {topic} FROM @2030-01-01"))
            .rows
            .is_empty()
    );
}

#[test]
fn tail_shows_new_messages_until_stop_and_creates_no_group() {
    let Some(uri) = kafka_uri() else { return };
    let topic = unique("tail");
    create_topic(&uri, &topic, 1);
    produce(&uri, &topic, &[Out::new(0, "old", b"before")]);
    let before = group_names(&uri);
    let mut h = Harness::open(&uri);
    let execution = h.start_live(&format!("TAIL {topic} WHERE value CONTAINS 'new'"));
    assert_eq!(h.wait_columns(), MESSAGE_COLUMNS);
    std::thread::sleep(Duration::from_millis(500));
    produce(
        &uri,
        &topic,
        &[
            Out::new(0, "skip", b"other"),
            Out::new(0, "fresh", b"new one"),
        ],
    );
    let row = h.wait_row(|row| row[3] == "fresh");
    assert_eq!(row[4], "new one");
    h.manager.stop(h.console);
    let run = h.collect(execution);
    assert_eq!(run.end, Some(ExecutionEnd::Stopped), "{run:?}");
    assert_eq!(run.statuses, [StatementStatus::Success]);
    assert!(
        run.rows.iter().all(|row| texts(row)[3] != "old"),
        "TAIL starts at the end"
    );
    assert_eq!(group_names(&uri), before);
    assert_eq!(h.run_ok("TOPICS").columns.len(), 1, "the session stays");
}

#[test]
fn a_tail_outside_a_console_is_refused() {
    let Some(uri) = kafka_uri() else { return };
    let mut h = Harness::open(&uri);
    let run = h.run("TAIL anything");
    assert_eq!(run.end, Some(ExecutionEnd::Refused), "{run:?}");
}

#[test]
fn group_shows_committed_offsets_and_lag_without_joining() {
    let Some(uri) = kafka_uri() else { return };
    let topic = unique("lag");
    let group = unique("group");
    create_topic(&uri, &topic, 2);
    produce(
        &uri,
        &topic,
        &[
            Out::new(0, "a", b"1"),
            Out::new(0, "b", b"2"),
            Out::new(0, "c", b"3"),
            Out::new(1, "d", b"4"),
        ],
    );
    let committer: BaseConsumer = ClientConfig::new()
        .set("bootstrap.servers", bootstrap(&uri))
        .set("group.id", &group)
        .create()
        .unwrap();
    let mut offsets = TopicPartitionList::new();
    offsets
        .add_partition_offset(&topic, 0, Offset::Offset(1))
        .unwrap();
    committer.commit(&offsets, CommitMode::Sync).unwrap();
    drop(committer);
    let mut h = Harness::open(&uri);
    let run = h.run_ok(&format!("GROUP {group}"));
    assert_eq!(
        run.columns[0],
        ["topic", "partition", "committed", "end", "lag", "member"]
    );
    assert_eq!(
        run.grid(),
        [[
            topic.clone(),
            "0".into(),
            "1".into(),
            "3".into(),
            "2".into(),
            "NULL".into()
        ]]
    );
    let groups = h.run_ok("GROUPS");
    assert!(groups.column("group").contains(&group));

    let missing = unique("missing");
    let run = h.run_ok(&format!("GROUP {missing}"));
    assert!(run.rows.is_empty());
    assert!(!run.notices.is_empty());
    assert!(
        !group_names(&uri).contains(&missing),
        "a lag read creates no group"
    );
}

#[test]
fn the_catalog_lists_topics_and_groups() {
    let Some(uri) = kafka_uri() else { return };
    let topic = unique("catalog");
    create_topic(&uri, &topic, 1);
    let catalog = runtime().block_on(async {
        let mut session = network_connector(parse_uri(&uri).unwrap())
            .connect()
            .await
            .unwrap();
        session.read_catalog().await.unwrap()
    });
    let names: Vec<&str> = catalog
        .schemas
        .iter()
        .map(|schema| schema.name.as_str())
        .collect();
    assert_eq!(names, [TOPICS, GROUPS]);
    let topics = &catalog.schemas[0].relations;
    assert!(topics.iter().any(|relation| relation.name == topic));
    assert!(
        topics
            .iter()
            .all(|relation| !relation.name.starts_with("__"))
    );
}

#[test]
fn a_topic_page_reads_the_newest_matches_first() {
    let Some(uri) = kafka_uri() else { return };
    let topic = unique("page");
    create_topic(&uri, &topic, 2);
    let base = 1_700_000_000_000_i64;
    let values: Vec<String> = (0..1_200).map(|n| format!(r#"{{"n": {n}}}"#)).collect();
    let keys: Vec<String> = (0..1_200).map(|n| n.to_string()).collect();
    let messages: Vec<Out<'_>> = (0..1_200)
        .map(|n| Out {
            partition: (n % 2) as i32,
            key: Some(keys[n].as_bytes()),
            value: Some(values[n].as_bytes()),
            time: Some(base + n as i64),
            header: None,
        })
        .collect();
    produce(&uri, &topic, &messages);
    let mut h = Harness::open(&uri);
    let run = h.run_ok(&format!("SQLN.TOPIC {topic} 5"));
    assert_eq!(run.column("key"), ["1199", "1198", "1197", "1196", "1195"]);
    let run = h.run_ok(&format!("SQLN.TOPIC {topic} 5 WHERE value.n < 100"));
    assert_eq!(run.column("key"), ["99", "98", "97", "96", "95"]);
    let run = h.run_ok(&format!("SQLN.TOPIC {topic} 0"));
    assert!(run.rows.is_empty());
}

#[test]
fn a_wrong_sasl_password_asks_for_a_password() {
    let Ok(uri) = std::env::var("SQLN_TEST_KAFKA_SASL") else {
        println!("skipped: SQLN_TEST_KAFKA_SASL is not set");
        return;
    };
    let mut target = parse_uri(&uri).unwrap();
    target.password = Some(Secret::new("wrong"));
    let error = runtime()
        .block_on(network_connector(target.clone()).connect())
        .err()
        .expect("a refusal");
    assert!(error.password_refused, "{error:?}");
    target.password = Some(Secret::new("spike"));
    assert!(
        runtime()
            .block_on(network_connector(target).connect())
            .is_ok()
    );
}

#[test]
fn a_network_failure_is_not_a_password_refusal() {
    let Some(_) = kafka_uri() else { return };
    let target = parse_uri("kafka://127.0.0.1:1").unwrap();
    let error = runtime()
        .block_on(network_connector(target).connect())
        .err()
        .expect("a failure");
    assert!(!error.password_refused);
    assert!(!error.message.is_empty());
}

/// Registers a schema and returns its id.
fn register(
    url: &str,
    subject: &str,
    kind: &str,
    schema: &str,
    references: serde_json::Value,
) -> u32 {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let body = serde_json::json!({"schema": schema, "schemaType": kind, "references": references});
    runtime().block_on(async {
        let answer: serde_json::Value = reqwest::Client::new()
            .post(format!("{url}/subjects/{subject}/versions"))
            .header("Content-Type", "application/vnd.schemaregistry.v1+json")
            .json(&body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        u32::try_from(answer["id"].as_u64().unwrap_or_else(|| panic!("{answer}"))).unwrap()
    })
}

fn framed(id: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0];
    bytes.extend_from_slice(&id.to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

#[test]
fn the_registry_decodes_avro_protobuf_and_json_schema() {
    let (Some(uri), Some(url)) = (kafka_uri(), registry_url()) else {
        return;
    };
    let topic = unique("registry");
    create_topic(&uri, &topic, 1);
    let address = r#"{"type":"record","name":"Address","namespace":"t","fields":[{"name":"city","type":"string"}]}"#;
    let address_subject = unique("address");
    register(
        &url,
        &address_subject,
        "AVRO",
        address,
        serde_json::json!([]),
    );
    let person = r#"{"type":"record","name":"Person","namespace":"t","fields":[{"name":"name","type":"string"},{"name":"home","type":"t.Address"}]}"#;
    let avro = register(
        &url,
        &format!("{topic}-value"),
        "AVRO",
        person,
        serde_json::json!([{"name": "t.Address", "subject": address_subject, "version": 1}]),
    );
    let proto = register(
        &url,
        &unique("proto"),
        "PROTOBUF",
        r#"syntax = "proto3"; message Order { int64 id = 1; string name = 2; }"#,
        serde_json::json!([]),
    );
    let json = register(
        &url,
        &unique("json"),
        "JSON",
        r#"{"type":"object"}"#,
        serde_json::json!([]),
    );
    // Avro: name "Ana", city "RJ".
    let avro_value = framed(avro, &[6, b'A', b'n', b'a', 4, b'R', b'J']);
    // Protobuf: the first message, id 7 and name "Bia".
    let proto_value = framed(proto, &[0, 0x08, 7, 0x12, 3, b'B', b'i', b'a']);
    let json_value = framed(json, br#"{"name":"Caio"}"#);
    let unknown = framed(999_999, b"x");
    produce(
        &uri,
        &topic,
        &[
            Out::new(0, "avro", &avro_value),
            Out::new(0, "proto", &proto_value),
            Out::new(0, "json", &json_value),
            Out::new(0, "unknown", &unknown),
        ],
    );
    let target = parse_uri(&format!("{uri}?registry={url}")).unwrap();
    let mut h = Harness::with(Arc::new(KafkaConnector::new(target)));
    let run = h.run_ok(&format!("CONSUME {topic}"));
    let values = run.column("value");
    assert_eq!(values[0], r#"{"home":{"city":"RJ"},"name":"Ana"}"#);
    assert_eq!(values[1], r#"{"id":"7","name":"Bia"}"#);
    assert_eq!(values[2], r#"{"name":"Caio"}"#);
    assert_eq!(values[3], "BYTES", "an unknown schema keeps the bytes");
    let schemas = run.column("schema");
    assert_eq!(schemas[0], format!("value {avro} avro"));
    assert_eq!(schemas[1], format!("value {proto} protobuf"));
    assert_eq!(schemas[2], format!("value {json} json"));
    assert!(
        run.notices.iter().any(|notice| notice.contains("999999")),
        "{:?}",
        run.notices
    );
    let run = h.run_ok(&format!(
        "CONSUME {topic} WHERE value.name = 'Bia' OR value.home.city = 'RJ'"
    ));
    assert_eq!(run.column("key"), ["avro", "proto"]);
    let selected = h.run_ok(&format!("SELECT key, value.name AS name FROM {topic};"));
    assert_eq!(selected.column("name"), ["Ana", "Bia", "Caio", "NULL"]);
    assert!(
        selected
            .notices
            .iter()
            .any(|notice| notice.contains("999999"))
    );
}

struct Screen {
    app: App,
}

impl Screen {
    fn open(uri: &str) -> Self {
        let target = parse_uri(uri).unwrap();
        let definition = Definition::new("events", Source::Network(target));
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

    fn wait_for(&mut self, wanted: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            self.app.drain_events();
            let screen = self.screen_at(80, 24);
            if screen.contains(wanted) {
                return screen;
            }
            assert!(Instant::now() < deadline, "{wanted} is not here:\n{screen}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

#[test]
fn a_topic_tab_shows_a_filter_without_an_order_on_a_small_screen() {
    use sqln::navigator::RowKind;
    let Some(uri) = kafka_uri() else { return };
    let topic = unique("tab");
    create_topic(&uri, &topic, 1);
    produce(&uri, &topic, &[Out::new(0, "k", b"tab-value")]);
    let mut screen = Screen::open(&uri);
    common::select_row(&mut screen.app, 0, &[RowKind::Table], &topic);
    screen
        .app
        .on_event(Event::Key(common::key(crossterm::event::KeyCode::Enter)));
    let deadline = Instant::now() + Duration::from_secs(30);
    while screen.app.table().is_none_or(|tab| tab.loaded() == 0) {
        screen.app.drain_events();
        assert!(Instant::now() < deadline, "the page did not arrive");
        std::thread::sleep(Duration::from_millis(2));
    }
    let text = screen.wait_for("(topic)");
    assert!(text.contains("WHERE"), "{text}");
    assert!(!text.contains("ORDER BY"), "{text}");
    let tab = screen.app.table().unwrap();
    assert!(tab.filters && tab.filter_only);
    assert_eq!(tab.kind(), "topic");
}
