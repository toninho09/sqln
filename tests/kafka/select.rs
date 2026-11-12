use super::*;
use sqln::connector::kafka::command::{Command, Read, Start, parse};
use sqln::sql::{split_tokens, statement_at, tokenize};

fn read(sql: &str, live: bool) -> Read {
    match parse(sql).unwrap() {
        Command::Tail(read) if live => read,
        Command::Consume(read) if !live => read,
        other => panic!("wrong read: {other:?}"),
    }
}

#[test]
fn select_uses_the_same_read_options_as_consume_and_tail() {
    for (sql, native, live) in [
        ("SELECT * FROM orders", "CONSUME orders", false),
        ("SELECT * FROM orders EMIT CHANGES;", "TAIL orders", true),
        (
            "SELECT *\nFROM orders START AT OFFSET 42 WHERE value.id = 7 PARTITION 2 LIMIT 3 SCAN 90;",
            "CONSUME orders FROM 42 WHERE value.id = 7 PARTITION 2 LIMIT 3 SCAN 90",
            false,
        ),
        (
            "SELECT * FROM orders START AT EARLIEST EMIT CHANGES",
            "TAIL orders FROM earliest",
            true,
        ),
        (
            "SELECT * FROM orders START AT TIMESTAMP '2026-01-02T00:00:01Z'",
            "CONSUME orders FROM @2026-01-02T00:00:01Z",
            false,
        ),
    ] {
        let select = read(sql, live);
        let native = read(native, live);
        assert_eq!(select.topic, native.topic);
        assert_eq!(select.start, native.start);
        assert_eq!(select.partition, native.partition);
        assert_eq!(select.limit, native.limit);
        assert_eq!(select.scan, native.scan);
        assert_eq!(select.filter.is_some(), native.filter.is_some());
        assert!(select.projection.is_none());
        assert!(!changes_state(sql, Dialect::Kafka));
        assert_eq!(is_continuous(sql, Dialect::Kafka), live);
    }
    assert_eq!(
        read("select * from orders start at latest", false).start,
        Start::Latest
    );
}

#[test]
fn select_rejects_unsupported_sql_and_invalid_live_limits_before_a_read() {
    for sql in [
        "SELECT * FROM orders LIMIT 1 EMIT CHANGES",
        "SELECT * FROM orders SCAN 10 EMIT CHANGES",
        "SELECT * FROM orders EMIT CHANGES LIMIT 1",
        "SELECT * FROM orders EMIT CHANGES EMIT CHANGES",
        "SELECT * FROM orders EMIT",
        "SELECT * FROM orders START AT earliest START AT latest",
        "SELECT * FROM orders FROM earliest",
        "SELECT * FROM orders START earliest",
        "SELECT * FROM orders START AT OFFSET -1",
        "SELECT * FROM orders START AT OFFSET 1.5",
        "SELECT * FROM orders START AT TIMESTAMP 'not a time'",
        "SELECT * FROM orders PARTITION -1",
        "SELECT * FROM orders LIMIT 0",
        "SELECT * FROM orders SCAN 0",
        "SELECT * FROM orders WHERE key MATCHES '('",
        "SELECT * FROM orders JOIN other",
        "SELECT * FROM orders ORDER BY timestamp",
        "SELECT count(*) FROM orders",
        "SELECT unknown FROM orders",
        "SELECT key.field FROM orders",
        "SELECT value..field FROM orders",
        "SELECT key, FROM orders",
        "SELECT *, key FROM orders",
        "SELECT * FROM orders; SELECT * FROM other;",
        "SELECT * FROM",
    ] {
        let error = parse(sql).expect_err(sql);
        assert!(error.offset.is_some(), "{sql}: {error:?}");
    }
    let sql = "SELECT * FROM orders LIMIT 1 LIMIT 2";
    assert_eq!(parse(sql).unwrap_err().offset, sql.rfind("LIMIT"));
}

#[test]
fn select_scripts_keep_multiline_queries_and_native_line_commands() {
    let sql = "# before\nTOPICS\nCONSUME orders\nSELECT key, value.id AS id\nFROM orders\n# filter\nWHERE key = 'a;b'\nLIMIT 2;\nTAIL orders\nSELECT * FROM orders\nEMIT CHANGES;";
    let statements = split(sql, Dialect::Kafka);
    assert_eq!(statements.len(), 5);
    assert_eq!(statements[0].text(sql), "TOPICS");
    assert_eq!(statements[1].text(sql), "CONSUME orders");
    assert!(statements[2].terminated);
    assert!(!statements[3].terminated);
    assert!(statements[4].terminated);
    for statement in &statements {
        parse(statement.text(sql)).unwrap();
    }
    assert_eq!(
        split_tokens(sql, Dialect::Kafka, &tokenize(sql, Dialect::Kafka)),
        statements
    );
    assert_eq!(
        statement_at(sql, Dialect::Kafka, sql.find("WHERE").unwrap()),
        Some(statements[2].clone())
    );
    assert_eq!(
        split("SELECT * FROM orders;SELECT * FROM other;", Dialect::Kafka).len(),
        2
    );
    assert_eq!(split("SELECT *\nFROM orders", Dialect::Kafka).len(), 1);
    let native = "CONSUME topic;name\nTAIL other";
    assert_eq!(
        split(native, Dialect::Kafka)[0].text(native),
        "CONSUME topic;name"
    );
}

#[test]
fn quotes_and_comments_do_not_change_the_read_mode_or_split_a_query() {
    for sql in [
        "SELECT * FROM orders WHERE key = 'EMIT CHANGES'",
        "SELECT * FROM \"EMIT CHANGES\"",
        "SELECT value AS \"EMIT CHANGES\" FROM orders",
        "SELECT * FROM orders\n# EMIT CHANGES\n",
        "SELECT * FROM orders WHERE key = 'it''s; quoted'",
        "SELECT * FROM orders WHERE key = 'first\n; second'",
        "SELECT * FROM orders WHERE key = 'first\n# EMIT CHANGES\nlast'",
    ] {
        assert!(!is_continuous(sql, Dialect::Kafka), "{sql}");
        assert_eq!(split(sql, Dialect::Kafka).len(), 1, "{sql}");
        read(sql, false);
    }
    read(
        "# before\nSELECT * FROM \"order\"\"events\" EMIT CHANGES;\n# after",
        true,
    );
    let escaped = read("SELECT * FROM orders WHERE key = 'it''s; quoted'", false);
    assert!(
        escaped
            .filter
            .unwrap()
            .matches(&sqln::connector::kafka::filter::Message {
                key: Some("it's; quoted"),
                value: None,
                json: None,
                headers: "",
            })
    );
    let quoted = read("SELECT * FROM \"order\"\"events\"", false);
    assert_eq!(quoted.topic, "order\"events");
    assert!(is_continuous(
        "SELECT * FROM orders\nEMIT CHANGES;\n# after",
        Dialect::Kafka
    ));
}

#[test]
fn projections_keep_types_bytes_null_and_exact_json_numbers() {
    let source: Row = [
        Value::Number("2"),
        Value::Number("9007199254740993"),
        Value::Null,
        Value::binary(&[0xff, 0x00]),
        Value::Text(r#"{"id":9007199254740993,"name":"Ana","null":null,"items":[true,{"a":1}]}"#),
        Value::Text(r#"{"source":"web"}"#),
        Value::Text("value 7"),
    ]
    .into_iter()
    .collect();
    let projection = read("SELECT key, offset, value.id AS id, value.name AS name, value.null, value.missing, value.items.0, value.items.1, timestamp, headers, schema FROM orders", false).projection.unwrap();
    assert_eq!(&projection.names()[..4], ["key", "offset", "id", "name"]);
    let row = projection.apply(source.clone());
    assert_eq!(row.get(0), source.get(3));
    assert_eq!(row.get(1), Some(Value::Number("9007199254740993")));
    assert_eq!(row.get(2), Some(Value::Number("9007199254740993")));
    assert_eq!(row.get(3), Some(Value::Text("Ana")));
    assert_eq!(row.get(4), Some(Value::Null));
    assert_eq!(row.get(5), Some(Value::Null));
    assert_eq!(row.get(6), Some(Value::Text("true")));
    assert_eq!(row.get(7), Some(Value::Text(r#"{"a":1}"#)));
    assert_eq!(row.get(8), Some(Value::Null));
    assert_eq!(row.get(9), source.get(5));
    assert_eq!(row.get(10), source.get(6));
    for value in [Value::Null, Value::Text("not JSON"), Value::binary(&[0xff])] {
        let source: Row = [Value::Null, Value::Null, Value::Null, Value::Null, value]
            .into_iter()
            .collect();
        let projection = read("SELECT value, value.id FROM orders", false)
            .projection
            .unwrap();
        let row = projection.apply(source);
        assert_eq!(row.get(0), Some(value));
        assert_eq!(row.get(1), Some(Value::Null));
    }
}

#[test]
fn select_completion_uses_the_statement_and_the_catalog_without_sql_tables() {
    assert!(labels("SELECT ").iter().any(|item| item.0 == "value"));
    assert!(labels("SELECT key, ").iter().any(|item| item.0 == "offset"));
    assert_eq!(labels("SELECT *\nFROM ord"), labels("CONSUME ord"));
    assert_eq!(labels("TOPICS\nSELECT *\nFROM ord"), labels("CONSUME ord"));
    assert_eq!(
        labels("SELECT * FROM orders START A"),
        [("AT".into(), "AT".into())]
    );
    assert_eq!(
        labels("SELECT * FROM orders START AT EAR"),
        [("EARLIEST".into(), "EARLIEST".into())]
    );
    assert_eq!(
        labels("SELECT * FROM orders EMIT CH"),
        [("CHANGES".into(), "CHANGES".into())]
    );
    assert!(labels("SELECT * FROM orders EMIT CHANGES ").is_empty());
    assert!(
        labels("SELECT key AS start FROM orders ")
            .iter()
            .any(|item| item.0 == "START")
    );
    assert!(
        labels("SELECT * FROM START ")
            .iter()
            .any(|item| item.0 == "WHERE")
    );
    assert!(!labels("SELECT * ").iter().any(|item| item.0 == "AS"));
    assert!(
        labels("SELECT * FROM orders LIMIT 1 ")
            .iter()
            .all(|item| item.0 != "LIMIT" && item.0 != "EMIT")
    );
    assert!(labels("SELECT * FROM orders WHERE key = 'unterminated").is_empty());
    let sql = "SELECT *\nFROM ord WHERE key = 'x'";
    let cursor = sql.find(" WHERE").unwrap();
    let result = suggest(sql, cursor, Dialect::Kafka, Some(&catalog()));
    assert_eq!(&sql[result.replace], "ord");
    assert!(result.items.iter().any(|item| item.label == "orders"));
}

#[test]
fn multiline_select_completion_and_run_selection_work_in_both_editor_modes() {
    use crossterm::event::KeyCode;
    use edtui::Index2;
    use sqln::editor::RunScope;
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut fixture = common::Fixture::with_dialect("SELECT *\nFROM ", mode, Dialect::Kafka);
        fixture.catalog = catalog();
        if mode == KeyboardMode::Vim {
            fixture.press(common::key(KeyCode::Char('i')));
        }
        fixture.editor.set_cursor(Index2::new(1, 5));
        fixture.write("ord");
        let selected = fixture
            .labels()
            .iter()
            .position(|name| name == "orders")
            .unwrap();
        for _ in 0..selected {
            fixture.press(common::key(KeyCode::Down));
        }
        fixture.press(common::key(KeyCode::Tab));
        assert_eq!(fixture.text(), "SELECT *\nFROM orders");
        let run = fixture.editor.plan_run(RunScope::Default).unwrap();
        assert_eq!(run.statements, ["SELECT *\nFROM orders"]);
    }
}

#[test]
fn a_select_live_result_uses_custom_run_and_stop_keys_at_minimum_size() {
    use crossterm::event::KeyCode;
    use sqln::connector::StatementEnd;
    use sqln::input::Keymap;
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let handler: common::mock::RunHandler = Arc::new(|sql, mut sink| {
            Box::pin(async move {
                assert!(matches!(parse(&sql).unwrap(), Command::Tail(_)));
                sink.set_columns(vec!["key".to_owned()]).await;
                sink.push([Value::Text("visible-event")].into_iter().collect())
                    .await;
                sink.flush().await;
                sink.stop_token().cancelled().await;
                let (rows, limit) = sink.finish().await;
                Ok(StatementEnd {
                    rows,
                    limit,
                    has_columns: true,
                    ..StatementEnd::default()
                })
            })
        });
        let connector = Arc::new(common::mock::MockConnector::new(Dialect::Kafka, handler));
        let overrides = [
            ("run".to_owned(), vec!["f6".to_owned()]),
            ("stop".to_owned(), vec!["f7".to_owned()]),
        ]
        .into_iter()
        .collect();
        let options = Options {
            connector: Some(connector.clone()),
            keyboard: mode,
            keymap: Some(Keymap::with_overrides(mode, &overrides).unwrap()),
            ..Options::default()
        };
        let mut screen = Screen {
            app: App::new(
                options,
                Rc::new(RefCell::new(MemoryClipboard::default())),
                Theme::plain(),
            ),
        };
        let sql = "SELECT key\nFROM orders\nEMIT CHANGES;";
        screen.app.on_event(Event::Paste(sql.to_owned()));
        screen.app.on_event(Event::Key(common::key(KeyCode::F(6))));
        screen.wait_for("1 row received. Live.");
        for (width, height) in [(80, 24), (120, 35), (80, 24)] {
            let text = screen.screen_at(width, height);
            assert!(text.contains("Live"), "{text}");
            assert!(text.contains("visible-event"), "{text}");
        }
        screen.app.on_event(Event::Key(common::key(KeyCode::F(7))));
        let deadline = Instant::now() + Duration::from_secs(10);
        while screen.app.console().is_busy() {
            screen.app.drain_events();
            assert!(Instant::now() < deadline, "the read does not stop");
            std::thread::sleep(Duration::from_millis(1));
        }
        let text = screen.screen_at(80, 24);
        assert!(text.contains("visible-event"), "{text}");
        assert!(!text.contains("Live"), "{text}");
        assert_eq!(screen.app.console().editor.text(), sql);
        assert_eq!(connector.statements(), [sql.trim_end_matches(';')]);
    }
}

#[test]
fn select_and_consume_deliver_the_same_messages_and_scan_limits() {
    let Some(uri) = kafka_uri() else { return };
    let topic = unique("select");
    create_topic(&uri, &topic, 2);
    produce(
        &uri,
        &topic,
        &[
            Out::new(0, "a", br#"{"id":9007199254740993,"status":"failed"}"#),
            Out::new(0, "b", br#"{"id":2,"status":"ok"}"#),
            Out::new(1, "c", &[0xff]),
            Out {
                partition: 1,
                key: Some(b"d"),
                value: None,
                time: None,
                header: None,
            },
        ],
    );
    let before = group_names(&uri);
    let mut h = Harness::open(&uri);
    let native = h.run_ok(&format!("CONSUME {topic}"));
    let select = h.run_ok(&format!("SELECT *\nFROM {topic};"));
    assert_eq!(select.columns, native.columns);
    let mut expected = native.grid();
    let mut actual = select.grid();
    expected.sort();
    actual.sort();
    assert_eq!(actual, expected);
    let select = h.run_ok(&format!(
        "SELECT key, value.id AS id FROM {topic} WHERE value.status = 'failed' SCAN 10 LIMIT 2;"
    ));
    assert_eq!(select.columns[0], ["key", "id"]);
    assert_eq!(select.grid(), [["a", "9007199254740993"]]);
    assert!(
        select
            .notices
            .iter()
            .any(|notice| notice == "Scanned 4 messages; 1 matched.")
    );
    let select = h.run_ok(&format!(
        "SELECT key FROM {topic} PARTITION 0 START AT OFFSET 1 SCAN 1"
    ));
    assert_eq!(select.column("key"), ["b"]);
    assert!(
        h.run_ok(&format!("SELECT * FROM {topic} START AT LATEST"))
            .rows
            .is_empty()
    );
    assert_eq!(group_names(&uri), before);
}

#[test]
fn select_emit_changes_keeps_live_delivery_stop_and_session_reuse() {
    let Some(uri) = kafka_uri() else { return };
    let topic = unique("select_live");
    create_topic(&uri, &topic, 1);
    produce(&uri, &topic, &[Out::new(0, "old", br#"{"id":1}"#)]);
    let before = group_names(&uri);
    let mut h = Harness::open(&uri);
    assert_eq!(
        h.run(&format!("SELECT * FROM {topic} EMIT CHANGES")).end,
        Some(ExecutionEnd::Refused)
    );
    let execution = h.start_live(&format!(
        "SELECT key, value.id AS id FROM {topic} EMIT CHANGES;"
    ));
    assert_eq!(h.wait_columns(), ["key", "id"]);
    std::thread::sleep(Duration::from_millis(500));
    produce(&uri, &topic, &[Out::new(0, "fresh", br#"{"id":2}"#)]);
    // No old message can precede the new message in the live result.
    assert_eq!(h.wait_row(|_| true), ["fresh", "2"]);
    h.manager.stop(h.console);
    assert_eq!(h.collect(execution).end, Some(ExecutionEnd::Stopped));
    let execution = h.start_live(&format!(
        "SELECT key FROM {topic} START AT EARLIEST EMIT CHANGES"
    ));
    assert_eq!(h.wait_columns(), ["key"]);
    assert_eq!(h.wait_row(|_| true), ["old"]);
    h.manager.stop(h.console);
    assert_eq!(h.collect(execution).end, Some(ExecutionEnd::Stopped));
    assert_eq!(h.run_ok("TOPICS").columns.len(), 1);
    assert_eq!(group_names(&uri), before);
}
