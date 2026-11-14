use super::*;
use sqln::connector::target::{topic_filter, topic_visible};

fn patterns(text: &str) -> Vec<String> {
    topic_filter(text).unwrap()
}

#[test]
fn the_topic_filter_reads_patterns_with_commas() {
    assert_eq!(
        patterns(" orders.* , ,!*.dlq,billing-* "),
        ["orders.*", "!*.dlq", "billing-*"]
    );
    assert!(patterns("").is_empty(), "an empty text shows all topics");
    for text in ["orders a", "orders\u{1b}", "!", "a, !"] {
        let error = topic_filter(text).unwrap_err();
        assert!(error.contains("commas"), "{text}: {error}");
    }
}

#[test]
fn includes_select_topics_and_excludes_remove_them() {
    let visible = |filter: &str, name: &str| topic_visible(&patterns(filter), name);
    assert!(visible("", "anything"), "no pattern shows all topics");
    assert!(visible("orders.*", "orders.v1"));
    assert!(visible("orders.*", "orders."), "* matches an empty text");
    assert!(!visible("orders.*", "orders"));
    assert!(
        !visible("orders.*", "Orders.v1"),
        "the case of the letters matches"
    );
    assert!(visible("*-events-*", "a-events-b"));
    assert!(
        visible("a*b*c", "aXXbYYbZc"),
        "* goes back after a wrong guess"
    );
    assert!(!visible("a*b*c", "aXXbYY"));
    assert!(visible("orders.*, billing", "billing"));
    assert!(!visible("orders.*, billing", "billing.v2"));
    assert!(
        visible("!*.dlq", "orders"),
        "only excludes keep the other topics"
    );
    assert!(!visible("!*.dlq", "orders.dlq"));
    assert!(
        !visible("orders.*, !*.dlq", "orders.dlq"),
        "an exclude wins"
    );
    assert!(visible("orders.*, !*.dlq", "orders.v1"));
    assert!(visible("pedido-ç*", "pedido-çã"), "UTF-8 names match");
}

#[test]
fn a_kafka_text_reads_the_topic_filter() {
    let target = parse_uri("kafka://127.0.0.1?topics=orders.*,!*.dlq").unwrap();
    assert_eq!(target.kafka.topics, ["orders.*", "!*.dlq"]);
    let target = parse_uri("kafka://127.0.0.1?topics=").unwrap();
    assert!(target.kafka.topics.is_empty());
    for (text, part) in [
        ("kafka://127.0.0.1?topics=a%20b", "commas"),
        ("redis://127.0.0.1?topics=a", "only for kafka"),
    ] {
        let error = parse_uri(text).unwrap_err();
        assert!(error.contains(part), "{text}: {error}");
    }
}

#[test]
fn the_form_saves_the_topic_filter_in_both_entry_modes() {
    let topics = |form: &ConnectionForm| match form.definition().unwrap().source {
        Source::Network(target) => target.kafka.topics,
        _ => panic!("not a network source"),
    };
    let mut form = kafka_form();
    assert!(form.fields().contains(&Field::Topics));
    form.focus_on(Field::Topics);
    form.on_paste(" orders.*, !*.dlq ");
    assert_eq!(topics(&form), ["orders.*", "!*.dlq"]);
    let definition = form.definition().unwrap();
    let edited = ConnectionForm::editing(1, &definition, false);
    assert_eq!(edited.definition().unwrap(), definition);

    let mut form = kafka_form();
    form.use_uri = true;
    form.focus_on(Field::Uri);
    form.on_paste("kafka://127.0.0.1?topics=from-text*");
    assert_eq!(topics(&form), ["from-text*"]);
    form.focus_on(Field::Topics);
    form.on_paste("from-field*");
    assert_eq!(topics(&form), ["from-field*"], "the field wins");

    let mut form = kafka_form();
    form.focus_on(Field::Topics);
    form.on_paste("orders a");
    let error = form.definition().unwrap_err();
    assert_eq!(error.field, Field::Topics);
    assert!(error.message.contains("commas"), "{}", error.message);
}

#[test]
fn the_topic_filter_field_shows_in_the_smallest_screen() {
    let mut form = kafka_form();
    form.focus_on(Field::Topics);
    form.on_paste("orders.*, !*.dlq");
    form.select_section(Field::Topics.group());
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| {
            sqln::ui::form::draw_form(frame, &form, &Theme::plain());
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let shown: String = buffer
        .content
        .chunks(usize::from(buffer.area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>() + "\n")
        .collect();
    assert!(shown.contains("Topics"), "{shown}");
    assert!(shown.contains("orders.*, !*.dlq"), "{shown}");
}

#[test]
fn a_saved_kafka_connection_keeps_the_topic_filter() {
    let mut form = kafka_form();
    let definition = form.definition().unwrap();
    let json = serde_json::to_string(&to_entry("k1", &definition)).unwrap();
    assert!(!json.contains("topics"), "no empty filter: {json}");

    form.focus_on(Field::Topics);
    form.on_paste("orders.*, !*.dlq");
    let definition = form.definition().unwrap();
    let entry = to_entry("k1", &definition);
    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains(r#""topics":["orders.*","!*.dlq"]"#), "{json}");
    let mut read = to_definition(&entry, "/base".into());
    read.base = None;
    assert_eq!(read, definition.without_password());
}

#[test]
fn the_settings_refuse_a_wrong_topic_filter() {
    let refused = |config: &str| {
        let dir = crate::common::temp_dir!(&format!("kafka-topics-{}", uuid::Uuid::new_v4()));
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
        r#"{"version":1,"connections":[{"id":"r","name":"r","kind":"redis","host":"h","topics":["a"]}]}"#,
    );
    assert!(message.contains("only for kafka"), "{message}");
    for topics in [r#"["a b"]"#, r#"["a,b"]"#, r#"["!"]"#] {
        let message = refused(&format!(
            r#"{{"version":1,"connections":[{{"id":"k","name":"k","kind":"kafka","host":"h","topics":{topics}}}]}}"#
        ));
        assert!(message.contains("topics"), "{topics}: {message}");
    }
}

#[test]
fn the_topic_filter_changes_the_catalog_and_not_the_topics_command() {
    let Some(uri) = kafka_uri() else { return };
    let base = unique("filter");
    let (keep, skip) = (format!("{base}-keep"), format!("{base}-skip.dlq"));
    create_topic(&uri, &keep, 1);
    create_topic(&uri, &skip, 1);
    let filtered = format!("{uri}?topics={base}-*,!*.dlq");
    let catalog = runtime().block_on(async {
        let mut session = network_connector(parse_uri(&filtered).unwrap())
            .connect()
            .await
            .unwrap();
        session.read_catalog().await.unwrap()
    });
    let names: Vec<&str> = catalog.schemas[0]
        .relations
        .iter()
        .map(|relation| relation.name.as_str())
        .collect();
    assert_eq!(names, [keep.as_str()], "only the matching topics");
    assert_eq!(catalog.schemas[1].name, GROUPS, "the groups stay");

    let mut harness = Harness::open(&filtered);
    let run = harness.run_ok("TOPICS");
    let listed: Vec<String> = run.rows.iter().map(|row| texts(row)[0].clone()).collect();
    assert!(listed.contains(&keep), "{listed:?}");
    assert!(
        listed.contains(&skip),
        "TOPICS lists all topics: {listed:?}"
    );
}
