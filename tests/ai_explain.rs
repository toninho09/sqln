use super::*;

// ---------------------------------------------------------------------------
// AI explanations of an error and of a plan.

/// Types one statement into the editor and runs it.
fn run_statement(app: &mut App, sql: &str) {
    app.on_event(Event::Paste(sql.into()));
    app.on_event(Event::Key(key(KeyCode::F(5))));
}

/// Replaces the editor text.
fn replace_text(app: &mut App, sql: &str) {
    app.on_event(Event::Key(ctrl('a')));
    app.on_event(Event::Paste(sql.into()));
}

fn explain_error(app: &mut App) {
    app.on_event(Event::Key(ctrl('w')));
    app.on_event(Event::Key(shift(KeyCode::Char('M'))));
}

fn explain_plan(app: &mut App) {
    app.on_event(Event::Key(ctrl('w')));
    app.on_event(Event::Key(shift(KeyCode::Char('T'))));
}

fn show_plan(app: &mut App) {
    app.on_event(Event::Key(ctrl('w')));
    app.on_event(Event::Key(shift(KeyCode::Char('E'))));
}

/// Returns the JSON user message of the last AI request.
fn last_payload(server: &Server) -> Value {
    let body = server.requests.lock().unwrap().last().unwrap().clone();
    // The captured text is one whole HTTP request: the JSON body starts at the first
    // brace.
    let start = body.find('{').unwrap();
    let json: Value = serde_json::from_str(&body[start..]).unwrap();
    let user = json["messages"][1]["content"].as_str().unwrap();
    serde_json::from_str(user).unwrap()
}

#[test]
fn the_explanation_of_an_error_shows_in_a_window() {
    let server = Server::new("1. The table is missing.\n2. Create it.", Duration::ZERO);
    let mut app = app("ai-explain-error", server.provider());
    run_statement(&mut app, "SELECT * FROM missing;");
    pump(&mut app, |s| s.contains("no such table"));
    // The palette offers the command in this state.
    app.on_event(Event::Key(ctrl('p')));
    app.on_event(Event::Paste("explain the last error".into()));
    let palette = pump(&mut app, |s| s.contains("Explain the last error with AI"));
    assert!(palette.contains("Ctrl+W M"), "{palette}");
    app.on_event(Event::Key(key(KeyCode::Esc)));
    explain_error(&mut app);
    let dialog = pump(&mut app, |s| s.contains("Send the error to AI"));
    assert!(dialog.contains("failed statement"), "{dialog}");
    app.on_event(Event::Key(key(KeyCode::Tab)));
    app.on_event(Event::Key(key(KeyCode::Enter)));
    let shown = pump(&mut app, |s| s.contains("AI error explanation"));
    assert!(shown.contains("The table is missing"), "{shown}");
    let payload = last_payload(&server);
    assert_eq!(payload["dialect"], "Sqlite");
    assert!(
        payload["statement"]
            .as_str()
            .unwrap()
            .contains("SELECT * FROM missing")
    );
    assert!(payload["error"].as_str().unwrap().contains("no such table"));
    assert!(payload.get("plan").is_none());
    app.on_event(Event::Key(key(KeyCode::Esc)));
    let closed = screen(&mut app);
    assert!(!closed.contains("AI error explanation"), "{closed}");
}

#[test]
fn the_prefix_menu_shows_available_ai_explanations_with_effective_keys() {
    use sqln::input::Keymap;
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        for (binding, keys) in [
            ("default", None),
            ("custom", Some(["ctrl+w u", "ctrl+w U"])),
            ("outside-prefix", Some(["f8", "f9"])),
        ] {
            for plan in [false, true] {
                let name = format!("ai-prefix-{mode:?}-{binding}-{plan}");
                let server = Server::new("unused", Duration::ZERO);
                let keymap = keys.map(|keys| {
                    let overrides = std::collections::HashMap::from([
                        ("aiExplainError".into(), vec![keys[0].into()]),
                        ("aiExplainPlan".into(), vec![keys[1].into()]),
                    ]);
                    Keymap::with_overrides(mode, &overrides).unwrap()
                });
                let mut app = app_with_options(
                    &name,
                    server.provider(),
                    Options {
                        keyboard: mode,
                        keymap,
                        ..Options::default()
                    },
                );
                let initial = crate::common::prefix_page(&app);
                assert!(
                    !initial.iter().any(|row| row.contains("Explain error (AI)")
                        || row.contains("Explain plan (AI)")),
                    "{initial:?}"
                );
                if plan {
                    app.on_event(Event::Paste("SELECT name FROM users;".into()));
                    show_plan(&mut app);
                    pump(&mut app, |screen| screen.contains("SCAN users"));
                } else {
                    run_statement(&mut app, "SELECT * FROM missing;");
                    pump(&mut app, |screen| screen.contains("no such table"));
                }
                app.on_event(Event::Key(ctrl('w')));
                let shown = screen(&mut app);
                let entries = crate::common::prefix_page(&app);
                let (label, default_key, custom_key) = if plan {
                    ("Explain plan (AI)", "T", "U")
                } else {
                    ("Explain error (AI)", "M", "u")
                };
                if binding == "outside-prefix" {
                    assert!(!shown.contains(label), "{shown}");
                    assert!(
                        !entries.iter().any(|row| row.contains(label)),
                        "{entries:?}"
                    );
                } else {
                    let key = if binding == "default" {
                        default_key
                    } else {
                        custom_key
                    };
                    assert!(entries.contains(&format!("{key} {label}")), "{entries:?}");
                    assert!(shown.contains(label), "{shown}");
                }
                assert!(server.requests.lock().unwrap().is_empty());
                app.on_event(Event::Key(key(KeyCode::Esc)));
            }
        }
    }
}

#[test]
fn an_explanation_without_an_error_refuses_with_a_reason() {
    let server = Server::new("answer", Duration::ZERO);
    let mut app = app("ai-explain-no-error", server.provider());
    explain_error(&mut app);
    pump(&mut app, |s| s.contains("No failed statement"));
    assert!(server.requests.lock().unwrap().is_empty());
}

#[test]
fn a_refused_consent_sends_no_request() {
    let server = Server::new("answer", Duration::ZERO);
    let mut app = app("ai-explain-refused", server.provider());
    run_statement(&mut app, "SELECT * FROM missing;");
    pump(&mut app, |s| s.contains("no such table"));
    explain_error(&mut app);
    pump(&mut app, |s| s.contains("Send the error to AI"));
    app.on_event(Event::Key(key(KeyCode::Enter)));
    pump(&mut app, |s| !s.contains("Send the error to AI"));
    assert!(server.requests.lock().unwrap().is_empty());
}

#[test]
fn the_explanation_of_a_plan_sends_the_tree_without_the_explain_word() {
    let server = Server::new("1. The scan reads every row.", Duration::ZERO);
    let mut app = app("ai-explain-plan", server.provider());
    run_statement(&mut app, "SELECT * FROM users;");
    pump(&mut app, |s| s.contains("name"));
    show_plan(&mut app);
    pump(&mut app, |s| s.contains("SCAN"));
    explain_plan(&mut app);
    pump(&mut app, |s| s.contains("Send the plan to AI"));
    app.on_event(Event::Key(key(KeyCode::Tab)));
    app.on_event(Event::Key(key(KeyCode::Enter)));
    let shown = pump(&mut app, |s| s.contains("AI plan explanation"));
    assert!(shown.contains("The scan reads every row"), "{shown}");
    let payload = last_payload(&server);
    assert_eq!(payload["dialect"], "Sqlite");
    assert!(
        payload["statement"]
            .as_str()
            .unwrap()
            .starts_with("SELECT * FROM users"),
        "{payload}"
    );
    assert!(payload["plan"].as_str().unwrap().contains("SCAN"));
    assert!(payload.get("error").is_none());
    assert!(payload.get("metadata").is_none());
}

#[test]
fn an_explanation_after_a_completion_sends_the_loaded_schema() {
    let server = Server::new("answer", Duration::ZERO);
    let mut app = app("ai-explain-metadata", server.provider());
    ghost_at_first_line(&mut app, "SELECT \n FROM users;");
    app.on_event(Event::Key(key(KeyCode::Esc)));
    replace_text(&mut app, "SELECT * FROM missing;");
    app.on_event(Event::Key(key(KeyCode::F(5))));
    pump(&mut app, |s| s.contains("no such table"));
    // Completion consent does not cover error text. Approve this request explicitly.
    explain_error(&mut app);
    pump(&mut app, |s| s.contains("Send the error to AI"));
    app.on_event(Event::Key(key(KeyCode::Tab)));
    app.on_event(Event::Key(key(KeyCode::Enter)));
    pump(&mut app, |s| s.contains("AI error explanation"));
    let payload = last_payload(&server);
    let metadata = payload["metadata"].to_string();
    assert!(metadata.contains("users"), "{payload}");
}

#[test]
fn a_disabled_provider_refuses_the_explanations() {
    let server = Server::new("answer", Duration::ZERO);
    let mut app = App::new(
        Options {
            database: None,
            ai_completion: Preferences::default(),
            ..Options::default()
        },
        Rc::new(RefCell::new(MemoryClipboard::default())),
        Theme::plain(),
    );
    run_statement(&mut app, "SELECT 1;");
    explain_error(&mut app);
    pump(&mut app, |s| s.contains("AI is unavailable"));
    assert!(server.requests.lock().unwrap().is_empty());
}

fn allow_explanation(app: &mut App, label: &str) {
    pump(app, |s| s.contains(label));
    app.on_event(Event::Key(key(KeyCode::Tab)));
    app.on_event(Event::Key(key(KeyCode::Enter)));
}

#[test]
fn redis_errors_use_the_same_explanation_flow() {
    use sqln::connector::ExecError;
    use sqln::sql::Dialect;
    use sqln_testkit::mock::MockConnector;
    let server = Server::new("Use GET on a string key.", Duration::ZERO);
    let connector = MockConnector::new(
        Dialect::Redis,
        Arc::new(|_, _| {
            Box::pin(async {
                Err(ExecError::new(
                    "WRONGTYPE Operation against a key holding the wrong kind of value",
                ))
            })
        }),
    );
    let mut app = App::new(
        Options {
            connector: Some(Arc::new(connector)),
            ai_completion: Preferences {
                enabled: Some(true),
                provider: Some(server.provider()),
                ..Preferences::default()
            },
            ..Options::default()
        },
        Rc::new(RefCell::new(MemoryClipboard::default())),
        Theme::plain(),
    );
    run_statement(&mut app, "GET account");
    pump(&mut app, |s| s.contains("WRONGTYPE"));
    // MockConnector does not emit driver traces. Supply the same trace events that
    // the Redis driver emits for a failed user command.
    use sqln::command_log::{LogEvent, MessageLevel, next_id};
    let id = next_id();
    app.console_mut().messages.on_event(LogEvent::Started {
        id,
        key: None,
        user: true,
        system: false,
        header_len: 0,
        dialect: Dialect::Redis,
        text: "GET account".into(),
    });
    app.console_mut().messages.on_event(LogEvent::Finished {
        id,
        level: MessageLevel::Error,
        text: "WRONGTYPE".into(),
        elapsed: Duration::ZERO,
    });
    explain_error(&mut app);
    allow_explanation(&mut app, "Send the error to AI");
    pump(&mut app, |s| s.contains("AI error explanation"));
    let payload = last_payload(&server);
    assert_eq!(payload["dialect"], "Redis");
    assert!(
        payload["statement"]
            .as_str()
            .unwrap()
            .contains("GET account")
    );
    assert!(payload.get("metadata").is_none());
}

#[test]
fn stop_cancels_an_explanation_without_running_sql() {
    let server = Server::new("Late answer", Duration::from_millis(300));
    let mut app = app("ai-explain-cancel", server.provider());
    run_statement(&mut app, "SELECT * FROM missing;");
    pump(&mut app, |s| s.contains("no such table"));
    explain_error(&mut app);
    allow_explanation(&mut app, "Send the error to AI");
    let deadline = Instant::now() + Duration::from_secs(10);
    while server.requests.lock().unwrap().is_empty() {
        app.drain_events();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    app.on_event(Event::Key(ctrl('w')));
    app.on_event(Event::Key(key(KeyCode::Char('x'))));
    pump(&mut app, |s| s.contains("AI explanation cancelled"));
    std::thread::sleep(Duration::from_millis(350));
    app.drain_events();
    let shown = screen(&mut app);
    assert!(!shown.contains("AI error explanation"), "{shown}");
    assert!(!shown.contains("Late answer"), "{shown}");
}

#[test]
fn the_plan_explanation_keeps_the_executed_statement_after_an_edit() {
    let server = Server::new("This plan scans users.", Duration::ZERO);
    let mut app = app("ai-explain-plan-edited", server.provider());
    app.on_event(Event::Paste("SELECT name FROM users;".into()));
    show_plan(&mut app);
    pump(&mut app, |s| s.contains("SCAN users"));
    app.on_event(Event::Key(ctrl('w')));
    app.on_event(Event::Key(key(KeyCode::Char('k'))));
    replace_text(&mut app, "SELECT id FROM other;");
    explain_plan(&mut app);
    allow_explanation(&mut app, "Send the plan to AI");
    pump(&mut app, |s| s.contains("AI plan explanation"));
    let payload = last_payload(&server);
    assert!(
        payload["statement"]
            .as_str()
            .unwrap()
            .starts_with("SELECT name FROM users")
    );
    assert!(!payload["statement"].as_str().unwrap().contains("other"));
}

#[test]
fn explanation_shortcuts_respect_both_modes_and_custom_bindings() {
    use sqln::input::{Chord, Keymap, Region};
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let keys = Keymap::new(mode);
        for (key, action) in [("M", Action::AiExplainError), ("T", Action::AiExplainPlan)] {
            assert_eq!(
                keys.after(
                    Region::Global,
                    Chord::parse("ctrl+w").unwrap(),
                    Chord::parse(key).unwrap()
                ),
                Some(action)
            );
        }
        let overrides = std::collections::HashMap::from([
            ("aiExplainError".into(), vec!["f8".into()]),
            ("aiExplainPlan".into(), vec!["f9".into()]),
        ]);
        let keys = Keymap::with_overrides(mode, &overrides).unwrap();
        assert_eq!(keys.keys(Action::AiExplainError), vec!["f8"]);
        assert_eq!(keys.keys(Action::AiExplainPlan), vec!["f9"]);
    }
}

#[test]
fn starting_an_execution_discards_a_late_explanation() {
    let server = Server::new("Late answer", Duration::from_millis(300));
    let mut app = app("ai-explain-stale", server.provider());
    run_statement(&mut app, "SELECT * FROM missing;");
    pump(&mut app, |s| s.contains("no such table"));
    explain_error(&mut app);
    allow_explanation(&mut app, "Send the error to AI");
    let deadline = Instant::now() + Duration::from_secs(10);
    while server.requests.lock().unwrap().is_empty() {
        app.drain_events();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    replace_text(&mut app, "SELECT 42 AS fresh;");
    app.on_event(Event::Key(key(KeyCode::F(5))));
    pump(&mut app, |s| s.contains("fresh") && s.contains("42"));
    std::thread::sleep(Duration::from_millis(350));
    app.drain_events();
    let shown = screen(&mut app);
    assert!(!shown.contains("AI error explanation"), "{shown}");
    assert!(!shown.contains("Late answer"), "{shown}");
}

#[test]
fn an_explanation_preserves_an_open_value_selection_and_search() {
    use sqln::input::Keymap;
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let server = Server::new("The AI answer waits.", Duration::from_millis(150));
        let keys = Keymap::with_overrides(
            mode,
            &std::collections::HashMap::from([
                ("aiExplainError".into(), vec!["f8".into()]),
                ("gridOpenValue".into(), vec!["f9".into()]),
            ]),
        )
        .unwrap();
        let mut app = app_with_options(
            &format!("ai-explain-open-value-{mode:?}"),
            server.provider(),
            Options {
                keyboard: mode,
                keymap: Some(keys),
                ..Options::default()
            },
        );
        app.on_event(Event::Paste("SELECT * FROM missing;".into()));
        app.on_event(Event::Key(key(KeyCode::F(5))));
        pump(&mut app, |s| s.contains("no such table"));
        app.on_event(Event::Key(ctrl('w')));
        app.on_event(Event::Key(key(KeyCode::Char('k'))));
        app.console_mut().editor = sqln::editor::SqlEditor::new(
            "SELECT 'Original window text' AS value;",
            mode,
            sqln::sql::Dialect::Sqlite,
            Rc::new(RefCell::new(MemoryClipboard::default())),
        );
        app.on_event(Event::Key(key(KeyCode::F(5))));
        pump(&mut app, |s| s.contains("1 row") && s.contains("Idle"));
        assert!(app.console().messages.has_user_error());
        app.on_event(Event::Key(key(KeyCode::F(8))));
        let immediate = screen(&mut app);
        assert!(
            immediate.contains("Send the error to AI"),
            "{mode:?}: {immediate}"
        );
        allow_explanation(&mut app, "Send the error to AI");
        app.on_event(Event::Key(ctrl('w')));
        app.on_event(Event::Key(key(KeyCode::Char('j'))));
        app.on_event(Event::Key(key(KeyCode::F(9))));
        let shown = screen(&mut app);
        assert!(shown.contains("row 1"), "{shown}");
        for _ in 0..8 {
            app.on_event(Event::Key(shift(KeyCode::Right)));
        }
        app.on_event(Event::Key(ctrl('f')));
        app.on_event(Event::Paste("window".into()));
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            app.drain_events();
            std::thread::sleep(Duration::from_millis(2));
        }
        let shown = screen(&mut app);
        assert!(shown.contains("row 1"), "{shown}");
        assert!(shown.contains("window"), "{shown}");
        assert!(!shown.contains("AI error explanation"), "{shown}");
        assert_eq!(
            app.console().editor.text(),
            "SELECT 'Original window text' AS value;"
        );
        app.on_event(Event::Key(key(KeyCode::Esc)));
        // Search closes first. The value window still owns the next key.
        let shown = screen(&mut app);
        assert!(shown.contains("8 characters selected"), "{shown}");
        app.on_event(Event::Key(key(KeyCode::Esc)));
        pump(&mut app, |s| s.contains("AI error explanation"));
        assert_eq!(server.requests.lock().unwrap().len(), 1);
    }
}

#[test]
fn an_answer_with_many_short_lines_refuses_before_exceeding_the_window_budget() {
    let server = Server::new(&"x\n".repeat(80_000), Duration::ZERO);
    let mut app = app_with_options(
        "ai-explain-window-budget",
        server.provider(),
        Options {
            settings: sqln::exec::Settings {
                budget_bytes: 8 * 1024 * 1024,
                ..Default::default()
            },
            ..Options::default()
        },
    );
    run_statement(&mut app, "SELECT * FROM missing;");
    pump(&mut app, |s| s.contains("no such table"));
    let original = app.console().editor.text();
    for _ in 0..2 {
        explain_error(&mut app);
        allow_explanation(&mut app, "Send the error to AI");
        let shown = pump(&mut app, |s| {
            s.contains("AI answer exceeds the memory budget")
        });
        assert!(!shown.contains("AI error explanation"), "{shown}");
        assert_eq!(app.console().editor.text(), original);
    }
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

#[test]
fn closing_an_explanation_releases_its_window_reservation() {
    let server = Server::new(&"x\n".repeat(10_000), Duration::ZERO);
    let mut app = app_with_options(
        "ai-explain-window-release",
        server.provider(),
        Options {
            settings: sqln::exec::Settings {
                budget_bytes: 8 * 1024 * 1024,
                ..Default::default()
            },
            ..Options::default()
        },
    );
    run_statement(&mut app, "SELECT * FROM missing;");
    pump(&mut app, |s| s.contains("no such table"));
    for _ in 0..2 {
        explain_error(&mut app);
        allow_explanation(&mut app, "Send the error to AI");
        pump(&mut app, |s| s.contains("AI error explanation"));
        app.on_event(Event::Key(key(KeyCode::PageDown)));
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        app.on_event(Event::Key(key(KeyCode::Esc)));
    }
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

#[test]
fn a_new_execution_cancels_a_ready_answer_and_releases_its_reservation() {
    let server = Server::new(&"x\n".repeat(10_000), Duration::from_millis(150));
    let mut app = app_with_options(
        "ai-explain-ready-cancel",
        server.provider(),
        Options {
            settings: sqln::exec::Settings {
                budget_bytes: 8 * 1024 * 1024,
                ..Default::default()
            },
            ..Options::default()
        },
    );
    app.on_event(Event::Paste("SELECT 1;".into()));
    show_plan(&mut app);
    pump(&mut app, |s| {
        s.contains("SCAN CONSTANT ROW") && s.contains("Idle")
    });
    explain_plan(&mut app);
    allow_explanation(&mut app, "Send the plan to AI");
    app.on_event(Event::Key(ctrl('w')));
    app.on_event(Event::Key(key(KeyCode::Char('j'))));
    app.on_event(Event::Key(key(KeyCode::Enter)));
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        app.drain_events();
        std::thread::sleep(Duration::from_millis(2));
    }
    let shown = screen(&mut app);
    assert!(shown.contains("row 1"), "{shown}");
    app.on_event(Event::Key(key(KeyCode::Esc)));
    app.on_event(Event::Key(key(KeyCode::F(5))));
    let shown = pump(&mut app, |s| s.contains("1 row") && s.contains("Idle"));
    assert!(!shown.contains("AI plan explanation"), "{shown}");
    show_plan(&mut app);
    pump(&mut app, |s| {
        s.contains("SCAN CONSTANT ROW") && s.contains("Idle")
    });
    explain_plan(&mut app);
    allow_explanation(&mut app, "Send the plan to AI");
    pump(&mut app, |s| s.contains("AI plan explanation"));
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

#[test]
fn kafka_errors_use_the_driver_history_for_ai_explanations() {
    let Ok(uri) = std::env::var("SQLN_TEST_KAFKA") else {
        println!("skipped: SQLN_TEST_KAFKA is not set");
        return;
    };
    let server = Server::new("Use TOPICS to list topics.", Duration::ZERO);
    let connector =
        sqln::connector::network_connector(sqln::connector::target::parse_uri(&uri).unwrap());
    let mut app = App::new(
        Options {
            connector: Some(connector),
            ai_completion: Preferences {
                enabled: Some(true),
                provider: Some(server.provider()),
                ..Default::default()
            },
            ..Options::default()
        },
        Rc::new(RefCell::new(MemoryClipboard::default())),
        Theme::plain(),
    );
    run_statement(&mut app, "INVALID command");
    pump(&mut app, |s| s.contains("Unknown Kafka command"));
    explain_error(&mut app);
    allow_explanation(&mut app, "Send the error to AI");
    pump(&mut app, |s| s.contains("AI error explanation"));
    let payload = last_payload(&server);
    assert_eq!(payload["dialect"], "Kafka");
    assert_eq!(payload["statement"], "INVALID command");
    assert!(
        payload["error"]
            .as_str()
            .unwrap()
            .contains("Unknown Kafka command")
    );
}

#[test]
fn a_connection_change_keeps_history_but_excludes_its_errors_from_ai() {
    use sqln::connection::{Definition, Source};
    use sqln::connector::sqlite::SqliteTarget;

    let server = Server::new("Check the table name.", Duration::ZERO);
    let mut app = App::new(
        Options {
            connections: ["first", "second"]
                .map(|name| Definition::new(name, Source::Sqlite(SqliteTarget::in_memory())))
                .to_vec(),
            ai_completion: Preferences {
                enabled: Some(true),
                provider: Some(server.provider()),
                ..Default::default()
            },
            ..Default::default()
        },
        Rc::new(RefCell::new(MemoryClipboard::default())),
        Theme::plain(),
    );
    run_statement(&mut app, "SELECT * FROM private_first;");
    pump(&mut app, |s| {
        s.contains("no such table") && s.contains("Idle")
    });
    let first = app.console().connection;
    let history = app.console().messages.id;
    for _ in 0..2 {
        let previous = app.console().connection;
        app.on_event(Event::Key(ctrl('w')));
        app.on_event(Event::Key(key(KeyCode::Char('a'))));
        app.on_event(Event::Key(key(KeyCode::Enter)));
        pump(&mut app, |s| s.contains("runs no SQL"));
        app.on_event(Event::Key(key(KeyCode::Tab)));
        app.on_event(Event::Key(key(KeyCode::Enter)));
        assert_ne!(app.console().connection, previous);
        assert_eq!(app.console().messages.id, history);
        assert!(
            app.console()
                .messages
                .iter()
                .any(|message| { message.text.contains("SELECT * FROM private_first") })
        );
        assert!(!app.console().messages.has_user_error());
        assert!(
            !crate::common::prefix_page(&app)
                .iter()
                .any(|row| row.contains("Explain error (AI)"))
        );
        app.on_event(Event::Key(ctrl('p')));
        app.on_event(Event::Paste("explain the last error".into()));
        let palette = screen(&mut app);
        assert!(
            !palette.contains("Explain the last error with AI"),
            "{palette}"
        );
        app.on_event(Event::Key(key(KeyCode::Esc)));
        explain_error(&mut app);
        pump(&mut app, |s| s.contains("No failed statement"));
        assert!(server.requests.lock().unwrap().is_empty());
    }
    assert_eq!(app.console().connection, first);
    replace_text(&mut app, "SELECT * FROM current_error;");
    app.on_event(Event::Key(key(KeyCode::F(5))));
    pump(&mut app, |s| {
        s.contains("current_error") && s.contains("Idle")
    });
    explain_error(&mut app);
    let consent = pump(&mut app, |s| s.contains("Send the error to AI"));
    assert!(consent.contains("Connection: first"), "{consent}");
    allow_explanation(&mut app, "Send the error to AI");
    pump(&mut app, |s| s.contains("AI error explanation"));
    let payload = last_payload(&server);
    assert!(
        payload["statement"]
            .as_str()
            .unwrap()
            .contains("current_error")
    );
    assert!(!payload.to_string().contains("private_first"));
}

#[test]
fn a_ready_explanation_preserves_active_input_until_escape() {
    use sqln::console::Focus;
    use sqln::input::Keymap;

    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        for input in ["editor", "grid", "parameters", "navigator"] {
            let server = Server::new("The answer waits for input.", Duration::from_millis(150));
            let keymap = Keymap::with_overrides(
                mode,
                &std::collections::HashMap::from([
                    ("editorFind".into(), vec!["f8".into()]),
                    ("gridFind".into(), vec!["f8".into()]),
                ]),
            )
            .unwrap();
            let mut app = app_with_options(
                &format!("ai-input-{mode:?}-{input}"),
                server.provider(),
                Options {
                    keyboard: mode,
                    keymap: Some(keymap),
                    ..Default::default()
                },
            );
            app.on_event(Event::Paste("SELECT name FROM users;".into()));
            app.on_event(Event::Key(key(KeyCode::Right)));
            let deadline = Instant::now() + Duration::from_secs(10);
            while app.connections()[0].catalog.is_none() {
                app.drain_events();
                assert!(Instant::now() < deadline, "the catalog does not load");
                std::thread::sleep(Duration::from_millis(2));
            }
            show_plan(&mut app);
            pump(&mut app, |s| s.contains("SCAN users") && s.contains("Idle"));
            explain_plan(&mut app);
            allow_explanation(&mut app, "Send the plan to AI");
            let focus = if input == "grid" { 'j' } else { 'k' };
            app.on_event(Event::Key(ctrl('w')));
            app.on_event(Event::Key(key(KeyCode::Char(focus))));
            match input {
                "parameters" => {
                    app.console_mut()
                        .parameters
                        .update_names(vec!["value".into()]);
                    app.on_event(Event::Key(ctrl('w')));
                    app.on_event(Event::Key(shift(KeyCode::Char('P'))));
                    app.on_event(Event::Key(key(KeyCode::Tab)));
                }
                "navigator" => {
                    app.on_event(Event::Key(ctrl('w')));
                    app.on_event(Event::Key(key(KeyCode::Char('h'))));
                    app.on_event(Event::Key(key(KeyCode::Char('/'))));
                }
                "grid" => {
                    app.on_event(Event::Key(shift(KeyCode::Right)));
                    app.on_event(Event::Key(key(KeyCode::F(8))));
                }
                _ => app.on_event(Event::Key(key(KeyCode::F(8)))),
            }
            app.on_event(Event::Paste("name".into()));
            if input == "parameters" {
                app.on_event(Event::Key(key(KeyCode::Left)));
            }
            let editor = app.console().editor.text();
            let cursor = app.console().editor.cursor();
            let view = &app.console().results[app.console().active].view;
            let selection = (view.row, view.col, view.anchor);
            let shown = pump(&mut app, |s| {
                s.contains("Answer ready") || s.contains("AI plan explanation")
            });
            assert!(shown.contains("Answer ready"), "{mode:?} {input}: {shown}");
            assert!(!shown.contains("AI plan explanation"), "{shown}");
            assert_eq!(app.console().editor.text(), editor);
            assert_eq!(app.console().editor.cursor(), cursor);
            let view = &app.console().results[app.console().active].view;
            assert_eq!((view.row, view.col, view.anchor), selection);
            match input {
                "editor" => assert_eq!(app.console().editor.search().unwrap().find, "name"),
                "grid" => assert_eq!(
                    app.console().results[app.console().active]
                        .search
                        .as_ref()
                        .unwrap()
                        .query(),
                    "name"
                ),
                "parameters" => {
                    assert_eq!(app.console().focus, Focus::Parameters);
                    let field = &app.console().parameters.fields[0];
                    assert_eq!(field.input.text(), "name");
                    assert_eq!(field.input.cursor(), 3);
                }
                _ => assert!(app.navigator().filter_is_editing()),
            }
            app.on_event(Event::Key(key(KeyCode::Esc)));
            assert!(!app.console().search_is_open());
            assert!(app.console().editor.search().is_none());
            assert_ne!(app.console().focus, Focus::Parameters);
            assert!(!app.navigator().filter_is_editing());
            let shown = pump(&mut app, |s| s.contains("AI plan explanation"));
            assert!(shown.contains("The answer waits for input"), "{shown}");
            assert_eq!(server.requests.lock().unwrap().len(), 1);
        }
    }
}

#[test]
fn stopping_a_pending_answer_keeps_the_search_and_releases_its_memory() {
    let server = Server::new(&"x\n".repeat(10_000), Duration::from_millis(150));
    let mut app = app_with_options(
        "ai-explain-search-cancel",
        server.provider(),
        Options {
            settings: sqln::exec::Settings {
                budget_bytes: 8 * 1024 * 1024,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    app.on_event(Event::Paste("SELECT name FROM users;".into()));
    show_plan(&mut app);
    pump(&mut app, |s| s.contains("SCAN users") && s.contains("Idle"));
    app.on_event(Event::Key(ctrl('w')));
    app.on_event(Event::Key(key(KeyCode::Char('j'))));
    app.on_event(Event::Key(ctrl('f')));
    app.on_event(Event::Paste("SCAN".into()));
    for _ in 0..2 {
        explain_plan(&mut app);
        allow_explanation(&mut app, "Send the plan to AI");
        pump(&mut app, |s| s.contains("Answer ready"));
        app.on_event(Event::Key(ctrl('w')));
        app.on_event(Event::Key(key(KeyCode::Char('x'))));
        let shown = screen(&mut app);
        assert!(shown.contains("AI explanation cancelled"), "{shown}");
        assert!(app.console().search_is_open());
        assert_eq!(
            app.console().results[app.console().active]
                .search
                .as_ref()
                .unwrap()
                .query(),
            "SCAN"
        );
    }
    app.on_event(Event::Key(key(KeyCode::Esc)));
    let shown = screen(&mut app);
    assert!(!shown.contains("AI plan explanation"), "{shown}");
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

#[test]
fn an_incomplete_ai_answer_is_not_presented_as_a_complete_explanation() {
    let response =
        json!({"choices": [{"message": {"content": "partial advice"}, "finish_reason": "length"}]});
    let server = Server::response(200, response.to_string(), Duration::ZERO);
    let mut app = app("ai-explain-incomplete", server.provider());
    run_statement(&mut app, "SELECT * FROM missing;");
    pump(&mut app, |s| s.contains("no such table"));
    explain_error(&mut app);
    allow_explanation(&mut app, "Send the error to AI");
    let shown = pump(&mut app, |s| s.contains("AI answer is incomplete"));
    assert!(!shown.contains("AI error explanation"), "{shown}");
}
