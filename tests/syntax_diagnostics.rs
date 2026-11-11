//! Local syntax checks do not change text or start database executions.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::common::{Fixture, ctrl, key, temp_db};
use crossterm::event::{Event, KeyCode};
use edtui::Index2;
use ratatui::{Terminal, backend::TestBackend, style::Modifier};
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::editor::KeyboardMode;
use sqln::input::{Action, Keymap};
use sqln::sql::{
    Dialect,
    diagnostics::{MAX_BYTES, check},
};
use sqln::ui::{draw_editor, theme::Theme};

#[test]
fn common_syntax_and_named_parameters_work_for_each_sql_dialect() {
    for dialect in [
        Dialect::Postgres,
        Dialect::MySql,
        Dialect::Sqlite,
        Dialect::DuckDb,
    ] {
        for sql in [
            "",
            "-- comment",
            "/* comment */",
            "SELECT 1; SELECT 2;",
            "SELECT :customer, :_other FROM orders WHERE id = :customer;",
            "WITH x AS (SELECT 1 AS n) SELECT n FROM x;",
            "INSERT INTO t (n) VALUES (:n); UPDATE t SET n=:n WHERE id=:id; DELETE FROM t WHERE id=:id;",
        ] {
            let report = check(sql, dialect);
            assert!(
                report.diagnostics.is_empty(),
                "{dialect:?}: {sql}: {report:?}"
            );
            assert!(report.note.is_none());
        }
    }
    for (dialect, sql) in [
        (
            Dialect::Postgres,
            "SELECT :name::text, $$:not_a_parameter$$, '{}'::jsonb ? 'key';",
        ),
        (Dialect::MySql, "SELECT `name` FROM t LIMIT 2;"),
        (Dialect::Sqlite, "SELECT [name] FROM t;"),
        (
            Dialect::DuckDb,
            "SELECT * EXCLUDE (id) FROM t QUALIFY row_number() OVER () = 1;",
        ),
        (
            Dialect::Postgres,
            "CREATE FUNCTION f() RETURNS int LANGUAGE SQL AS $$ SELECT 1; $$;",
        ),
    ] {
        let report = check(sql, dialect);
        assert!(
            report.diagnostics.is_empty(),
            "{dialect:?}: {sql}: {report:?}"
        );
    }
}

#[test]
fn unsupported_mysql_routines_remain_warnings_and_other_instructions_still_check() {
    let sql =
        "DELIMITER //\nCREATE PROCEDURE p() BEGIN SELECT 1; END//\nDELIMITER ;\nSELECT (1 + );";
    let report = check(sql, Dialect::MySql);
    assert_eq!(report.diagnostics.len(), 2, "{report:?}");
    assert!(report.diagnostics[0].message.contains("BEGIN"));
    assert_eq!(
        report.diagnostics[1].location.map(|(line, _)| line),
        Some(4)
    );
    assert!(sqln::ui::text::syntax_details(&report).contains("Run remains available"));
}

#[test]
fn errors_map_to_original_unicode_text_after_parameters_and_other_statements() {
    let sql = "SELECT '雪';\nSELECT 'ação', :customer, (1 + );\nSELECT (2 + );";
    let report = check(sql, Dialect::Postgres);
    assert_eq!(report.diagnostics.len(), 2, "{report:?}");
    for item in &report.diagnostics {
        let range = item.range.clone().expect("located error");
        assert_eq!(&sql[range.clone()], ")", "{item:?}");
        let before = &sql[..range.start];
        assert_eq!(
            item.location,
            Some((
                before.lines().count(),
                before.rsplit('\n').next().unwrap().chars().count() + 1
            ))
        );
    }
}

#[test]
fn unfinished_tokens_and_eof_have_safe_ranges() {
    for sql in [
        "SELECT (",
        "SELECT '雪",
        "SELECT * FROM",
        "SELECT 1; /* open",
        "/* open",
    ] {
        let report = check(sql, Dialect::Postgres);
        assert!(!report.diagnostics.is_empty(), "{sql}");
        for item in report.diagnostics {
            if let Some(range) = item.range {
                assert!(range.start < range.end);
                assert!(sql.get(range).is_some());
            }
        }
    }
    assert!(
        check("SELECT 1 -- normal comment", Dialect::Postgres)
            .diagnostics
            .is_empty()
    );
}

#[test]
fn checks_bound_input_statements_diagnostics_messages_and_recursion() {
    assert!(
        check(&"x".repeat(MAX_BYTES + 1), Dialect::Sqlite)
            .note
            .is_some()
    );
    assert!(
        check(&"SELECT 1;".repeat(129), Dialect::Sqlite)
            .note
            .is_some()
    );
    let report = check(&"SELECT FROM WHERE;".repeat(40), Dialect::Sqlite);
    assert_eq!(report.diagnostics.len(), 32);
    assert!(report.note.is_some());
    let report = check(
        &format!("SELECT {}1{}", "(".repeat(150), ")".repeat(150)),
        Dialect::Postgres,
    );
    assert!(!report.diagnostics.is_empty());
    let report = check(
        &format!("SELECT * FROM t WHERE 1 = 1 {}", "x".repeat(2000)),
        Dialect::Postgres,
    );
    assert_eq!(report.diagnostics.len(), 1, "{report:?}");
    assert_eq!(report.diagnostics[0].message.chars().count(), 512);
    assert!(report.diagnostics[0].message.ends_with('…'));
    assert!(check("not sql", Dialect::Redis).diagnostics.is_empty());
}

#[test]
fn long_expression_checks_do_not_abort_the_worker() {
    const CHILD: &str = "SQLN_TEST_SYNTAX_STACK_CHILD";
    if std::env::var_os(CHILD).is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                for dialect in [
                    Dialect::Postgres,
                    Dialect::MySql,
                    Dialect::Sqlite,
                    Dialect::DuckDb,
                ] {
                    for sql in [
                        format!("SELECT {}1", "1+".repeat(50_000)),
                        format!("SELECT 1{}", " UNION ALL SELECT 1".repeat(10_000)),
                        format!("SELECT 1{}", "::integer".repeat(20_000)),
                        format!("SELECT {});", "1+".repeat(50_000)),
                    ] {
                        assert!(sql.len() < MAX_BYTES);
                        let report = check(&sql, dialect);
                        assert!(report.diagnostics.is_empty(), "{dialect:?}: {report:?}");
                        assert!(
                            report.note.as_deref().is_some_and(|note| {
                                note.contains("1024 tokens")
                                    && note.contains("Run remains available")
                            }),
                            "{dialect:?}: {report:?}"
                        );
                    }
                }
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }

    let test_name = std::thread::current().name().unwrap().to_owned();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test_name, "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "worker process failed: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn token_limits_preserve_small_statements_comments_literals_and_locations() {
    for dialect in [
        Dialect::Postgres,
        Dialect::MySql,
        Dialect::Sqlite,
        Dialect::DuckDb,
    ] {
        let boundary = format!("SELECT {}1", "1 /* comment */ + ".repeat(511));
        let report = check(&boundary, dialect);
        assert!(report.diagnostics.is_empty(), "{dialect:?}: {report:?}");
        assert!(report.note.is_none(), "{dialect:?}: {report:?}");

        let sql = format!("{boundary}+1;\nSELECT (1 + );");
        let report = check(&sql, dialect);
        assert!(
            report
                .note
                .as_deref()
                .is_some_and(|note| note.contains("1024 tokens")),
            "{dialect:?}: {report:?}"
        );
        assert_eq!(report.diagnostics.len(), 1, "{dialect:?}: {report:?}");
        let warning = &report.diagnostics[0];
        assert_eq!(warning.location, Some((2, 13)));
        assert_eq!(&sql[warning.range.clone().unwrap()], ")");

        let sql = format!("SELECT '{}'", "1+".repeat(50_000));
        let report = check(&sql, dialect);
        assert!(report.diagnostics.is_empty(), "{dialect:?}: {report:?}");
        assert!(report.note.is_none(), "{dialect:?}: {report:?}");
    }
}

fn scan(fixture: &mut Fixture) {
    let now = Instant::now() + Duration::from_secs(2);
    fixture.editor.poll_syntax(now, |work| work());
    fixture
        .editor
        .poll_syntax(now + Duration::from_millis(401), |work| work());
    fixture
        .editor
        .poll_syntax(now + Duration::from_millis(402), |work| work());
}

#[test]
fn checks_debounce_and_reject_old_revisions_and_dialects() {
    let mut f = Fixture::conventional("SELECT (1 + );");
    let now = Instant::now();
    f.editor.poll_syntax(now, |_| panic!("must wait"));
    f.editor
        .poll_syntax(now + Duration::from_millis(399), |_| panic!("must wait"));
    let mut pending = None;
    f.editor
        .poll_syntax(now + Duration::from_millis(400), |work| {
            pending = Some(work)
        });
    f.press(ctrl('a'));
    f.editor.on_paste("SELECT 1");
    f.editor.set_dialect(Dialect::MySql);
    f.editor.poll_syntax(now + Duration::from_millis(401), |_| {
        panic!("job already pending")
    });
    pending.take().unwrap()();
    f.editor
        .poll_syntax(now + Duration::from_millis(402), |_| panic!("must wait"));
    assert!(f.editor.syntax_report().is_none());
    scan(&mut f);
    assert!(f.editor.syntax_report().unwrap().diagnostics.is_empty());
    f.editor.set_dialect(Dialect::Postgres);
    assert!(f.editor.syntax_report().is_none());
    scan(&mut f);
    assert!(f.editor.syntax_report().is_some());
    f.editor.set_dialect(Dialect::Redis);
    f.editor
        .poll_syntax(now + Duration::from_secs(3), |_| panic!("Redis is not SQL"));
    assert!(f.editor.syntax_report().is_none());
}

#[test]
fn a_dialect_change_rejects_a_pending_report_without_a_text_edit() {
    let mut f = Fixture::with_dialect("SELECT `name`", KeyboardMode::Conventional, Dialect::MySql);
    let now = Instant::now();
    f.editor.poll_syntax(now, |_| panic!("must wait"));
    let mut pending = None;
    f.editor
        .poll_syntax(now + Duration::from_millis(401), |work| {
            pending = Some(work)
        });
    f.editor.set_dialect(Dialect::Postgres);
    pending.take().unwrap()();
    f.editor.poll_syntax(now + Duration::from_millis(402), |_| {
        panic!("new dialect must wait")
    });
    assert!(f.editor.syntax_report().is_none());
    scan(&mut f);
    assert!(!f.editor.syntax_report().unwrap().diagnostics.is_empty());
    f.editor.set_dialect(Dialect::MySql);
    scan(&mut f);
    assert!(f.editor.syntax_report().unwrap().diagnostics.is_empty());
}

#[test]
fn current_reports_disappear_on_edit_and_worker_failure_allows_retry() {
    let mut f = Fixture::conventional("SELECT (1 + );");
    scan(&mut f);
    assert!(!f.editor.syntax_report().unwrap().diagnostics.is_empty());
    f.write("x");
    assert!(f.editor.syntax_report().is_none());
    let now = Instant::now();
    f.editor.poll_syntax(now, |_| {});
    f.editor
        .poll_syntax(now + Duration::from_millis(401), |_| {});
    f.editor
        .poll_syntax(now + Duration::from_millis(402), |_| {});
    assert!(
        f.editor
            .syntax_report()
            .unwrap()
            .note
            .as_ref()
            .unwrap()
            .contains("worker stopped")
    );
    f.write("y");
    scan(&mut f);
    assert!(f.editor.syntax_report().unwrap().note.is_none());
}

#[test]
fn oversized_editor_does_not_start_a_worker() {
    let mut f = Fixture::conventional(&"雪".repeat(MAX_BYTES / 3 + 1));
    let now = Instant::now();
    f.editor.poll_syntax(now, |_| panic!("oversized input"));
    f.editor.poll_syntax(now + Duration::from_millis(401), |_| {
        panic!("oversized input")
    });
    assert!(
        f.editor
            .syntax_report()
            .unwrap()
            .note
            .as_ref()
            .unwrap()
            .contains("256 KiB")
    );
}

#[test]
fn warning_underlines_preserve_cursor_selection_and_use_no_color() {
    let mut f = Fixture::conventional("SELECT (1 + );");
    f.editor.set_cursor(Index2::new(0, 0));
    scan(&mut f);
    let before = f.text();
    let cursor = f.editor.cursor();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| draw_editor(frame, frame.area(), &mut f.editor, true, &Theme::plain()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert!(
        buffer
            .content()
            .iter()
            .any(|cell| cell.symbol() == ")" && cell.modifier.contains(Modifier::UNDERLINED))
    );
    assert!(buffer.content().chunks(80).any(|row| {
        row.iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .contains("Syntax warning")
    }));
    assert_eq!(f.text(), before);
    assert_eq!(f.editor.cursor(), cursor);
    f.press(crate::common::shift(KeyCode::Right));
    let selected = f.selected();
    let cursor = f.editor.cursor();
    for (width, height) in [(30, 6), (1, 1), (100, 30)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw_editor(frame, frame.area(), &mut f.editor, true, &Theme::plain()))
            .unwrap();
        assert_eq!(f.selected(), selected);
        assert_eq!(f.editor.cursor(), cursor);
    }
}

#[test]
fn app_checks_both_modes_and_opens_full_warnings_with_a_custom_shortcut() {
    for keyboard in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let database = temp_db(&format!("syntax-{keyboard:?}"));
        rusqlite::Connection::open(&database).unwrap();
        let keymap = Keymap::with_overrides(
            keyboard,
            &HashMap::from([("sqlDiagnostics".into(), vec!["ctrl+w z".into()])]),
        )
        .unwrap();
        let mut app = App::new(
            Options {
                database: Some(database),
                keyboard,
                keymap: Some(keymap),
                ..Options::default()
            },
            Rc::new(RefCell::new(MemoryClipboard::default())),
            Theme::plain(),
        );
        if keyboard == KeyboardMode::Vim {
            app.on_event(Event::Key(key(KeyCode::Char('i'))));
        }
        let sql = "SELECT (1 + );";
        app.on_event(Event::Paste(sql.into()));
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.console().editor.syntax_report().is_none() {
            app.drain_events();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(!app.console().is_busy());
        assert!(app.console().results.is_empty());
        let cursor = app.console().editor.cursor();
        assert_eq!(Action::SqlDiagnostics.name(), "sqlDiagnostics");
        app.on_event(Event::Key(ctrl('w')));
        app.on_event(Event::Key(key(KeyCode::Char('z'))));
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(screen.contains("SQL syntax warnings"), "{screen}");
        assert!(screen.contains("Run remains available"));
        app.on_event(Event::Key(key(KeyCode::F(5))));
        assert!(!app.console().is_busy(), "the report must not run SQL");
        app.on_event(Event::Key(key(KeyCode::Esc)));
        assert_eq!(app.console().editor.text(), sql);
        assert_eq!(app.console().editor.cursor(), cursor);
        app.on_event(Event::Key(key(KeyCode::F(5))));
        assert!(
            app.console().is_busy(),
            "syntax warnings must not block Run"
        );
    }
}
