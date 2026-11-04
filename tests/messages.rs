use std::time::Duration;

use ratatui::style::Style;
use ratatui::text::Line;
use sqln::command_log::{CommandState, History, LogEvent};
use sqln::console::MessageLevel;
use sqln::exec::StatementKey;
use sqln::sql::Dialect;
use sqln::ui::messages::viewport;
use sqln::ui::theme::Theme;

fn begin(history: &mut History, id: u64, sql: &str, dialect: Dialect) -> StatementKey {
    let key = StatementKey {
        console: 1,
        execution: id,
        index: 0,
    };
    let header = format!("Command {id} · app_db · User\n");
    history.on_event(LogEvent::Started {
        id,
        key: Some(key),
        user: true,
        system: false,
        header_len: header.len(),
        dialect,
        text: format!("{header}{sql}"),
    });
    key
}

fn finish(history: &mut History, id: u64, level: MessageLevel, text: &str) {
    history.on_event(LogEvent::Finished {
        id,
        level,
        text: text.to_owned(),
        elapsed: Duration::from_micros(8420),
    });
}

fn text(lines: &[Line<'_>]) -> String {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn contains_style(lines: &[Line<'_>], value: &str, style: Style) -> bool {
    lines.iter().any(|line| {
        line.spans
            .iter()
            .any(|span| span.content.contains(value) && line.style.patch(span.style) == style)
    })
}

#[test]
fn commands_use_a_heading_sql_and_one_compact_result_line() {
    let mut history = History::default();
    let key = begin(
        &mut history,
        24,
        "SELECT id, name\nFROM users;",
        Dialect::Postgres,
    );
    finish(&mut history, 24, MessageLevel::Info, "Completed.");
    history.summarize(
        MessageLevel::Info,
        "Statement 1: 12 rows",
        Some(key),
        CommandState::Completed,
    );
    let raw = history[0].text.clone();
    let theme = Theme::ansi();
    let lines = viewport(&mut history, &mut 0, 80, 30, &theme);
    let shown = text(&lines);
    assert!(shown.starts_with("── #24 · app_db · User ─"), "{shown}");
    assert!(
        shown.contains("\n\nSELECT id, name\nFROM users;\n\n"),
        "{shown}"
    );
    assert!(
        shown.contains("✓ Completed · 12 rows · 8.420 ms"),
        "{shown}"
    );
    assert!(!shown.contains("Statement 1:"));
    assert!(!shown.contains("Time:"));
    assert_eq!(shown.matches("Completed").count(), 1);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].text, raw);
    assert!(contains_style(&lines, "SELECT", theme.keyword));
    assert!(contains_style(&lines, "12 rows", theme.mark_good));
    assert!(contains_style(&lines, "8.420 ms", theme.hint));
}

#[test]
fn errors_color_the_result_without_removing_sql_highlighting_or_repeating_the_error() {
    let mut history = History::default();
    let key = begin(
        &mut history,
        1,
        "SELECT 42, 'value' FROM missing;",
        Dialect::Postgres,
    );
    finish(
        &mut history,
        1,
        MessageLevel::Error,
        "Error: no such table: missing",
    );
    history.summarize(
        MessageLevel::Error,
        "Statement 1: no such table: missing",
        Some(key),
        CommandState::Failed,
    );
    let theme = Theme::ansi();
    let lines = viewport(&mut history, &mut 0, 90, 30, &theme);
    let shown = text(&lines);
    assert_eq!(
        shown.matches("no such table: missing").count(),
        1,
        "{shown}"
    );
    assert!(contains_style(&lines, "Error · no such table", theme.error));
    assert!(contains_style(&lines, "SELECT", theme.keyword));
    assert!(contains_style(&lines, "42", theme.number));
    assert!(contains_style(&lines, "'value'", theme.string));
}

#[test]
fn warnings_keep_their_details_and_use_a_warning_result_with_a_muted_time() {
    let mut history = History::default();
    let key = begin(&mut history, 1, "SELECT 1;", Dialect::Postgres);
    history.note(
        MessageLevel::Warning,
        "Statement 1: NOTICE: a database notice",
        Some(key),
    );
    finish(&mut history, 1, MessageLevel::Info, "Completed.");
    history.summarize(
        MessageLevel::Info,
        "Statement 1: 1 row",
        Some(key),
        CommandState::Completed,
    );
    let theme = Theme::ansi();
    let lines = viewport(&mut history, &mut 0, 90, 30, &theme);
    let shown = text(&lines);
    assert!(shown.contains("! Completed · 1 row · 8.420 ms"), "{shown}");
    assert_eq!(shown.matches("NOTICE: a database notice").count(), 1);
    assert!(contains_style(&lines, "! Completed", theme.mark_neutral));
    assert!(
        contains_style(&lines, "NOTICE:", theme.mark_neutral),
        "{lines:?}"
    );
    assert!(contains_style(&lines, "8.420 ms", theme.hint));
    assert!(contains_style(&lines, "SELECT", theme.keyword));
}

#[test]
fn multiline_tokens_keep_their_styles_when_the_view_starts_inside_the_command() {
    let mut history = History::default();
    begin(
        &mut history,
        1,
        "SELECT $tag$first\nSELECT secret$tag$, /* first\nSELECT hidden */ 42;",
        Dialect::Postgres,
    );
    finish(&mut history, 1, MessageLevel::Info, "Completed.");
    let theme = Theme::ansi();
    let mut scroll = 0;
    let lines = viewport(&mut history, &mut scroll, 90, 40, &theme);
    assert!(contains_style(&lines, "SELECT secret", theme.string));
    assert!(contains_style(&lines, "SELECT hidden", theme.comment));
    scroll = lines
        .iter()
        .position(|line| text(std::slice::from_ref(line)).contains("SELECT secret"))
        .unwrap();
    let lines = viewport(&mut history, &mut scroll, 90, 1, &theme);
    assert!(contains_style(&lines, "SELECT secret", theme.string));
    assert!(!contains_style(&lines, "SELECT secret", theme.keyword));
}

#[test]
fn each_command_keeps_its_own_dialect_for_history_after_a_connection_change() {
    let mut history = History::default();
    begin(
        &mut history,
        1,
        "SELECT `select` # MySQL comment",
        Dialect::MySql,
    );
    finish(&mut history, 1, MessageLevel::Info, "Completed.");
    begin(
        &mut history,
        2,
        "SELECT $tag$PostgreSQL string$tag$",
        Dialect::Postgres,
    );
    finish(&mut history, 2, MessageLevel::Info, "Completed.");
    let theme = Theme::ansi();
    let lines = viewport(&mut history, &mut 0, 90, 40, &theme);
    assert!(contains_style(&lines, "`select`", theme.identifier));
    assert!(contains_style(&lines, "# MySQL comment", theme.comment));
    assert!(contains_style(
        &lines,
        "$tag$PostgreSQL string$tag$",
        theme.string
    ));
}

#[test]
fn sql_that_looks_like_a_status_message_stays_in_the_sql_body() {
    let mut history = History::default();
    begin(
        &mut history,
        1,
        "SELECT 'Time: 1.000 ms. Completed.';\n-- Statement 1: 2 rows",
        Dialect::Postgres,
    );
    finish(&mut history, 1, MessageLevel::Info, "Completed.");
    let theme = Theme::ansi();
    let lines = viewport(&mut history, &mut 0, 90, 30, &theme);
    assert!(contains_style(
        &lines,
        "'Time: 1.000 ms. Completed.'",
        theme.string
    ));
    assert!(contains_style(
        &lines,
        "-- Statement 1: 2 rows",
        theme.comment
    ));
    assert!(text(&lines).contains("✓ Completed · 8.420 ms"));
}

#[test]
fn plain_and_narrow_views_keep_readable_text_and_safe_control_characters() {
    let mut history = History::default();
    let key = begin(
        &mut history,
        1,
        "SELECT 'é中\x1b\t' AS label;",
        Dialect::Postgres,
    );
    finish(&mut history, 1, MessageLevel::Info, "Completed.");
    history.summarize(
        MessageLevel::Info,
        "Statement 1: 1 row",
        Some(key),
        CommandState::Completed,
    );
    let plain = Theme::plain();
    let lines = viewport(&mut history, &mut 0, 80, 30, &plain);
    let shown = text(&lines);
    assert!(shown.starts_with("-- #1 · app_db · User -"));
    assert!(shown.contains("ok Completed · 1 row · 8.420 ms"));
    assert!(shown.contains("é中�→"));
    assert!(!shown.contains('\x1b'));
    for width in [0, 1, 2, 12, 40] {
        let lines = viewport(&mut history, &mut 0, width, 40, &plain);
        assert!(lines.len() <= 40);
        assert!(!text(&lines).contains('\x1b'));
    }
}

#[test]
fn running_stopped_and_unknown_commands_do_not_show_a_success_result() {
    for (state, label) in [
        (CommandState::Running, "Running"),
        (CommandState::Stopped, "Stopped"),
        (CommandState::Unknown, "Result unknown"),
        (CommandState::TimedOut, "Timeout"),
    ] {
        let mut history = History::default();
        let key = begin(&mut history, 1, "SELECT slow();", Dialect::Postgres);
        if state != CommandState::Running {
            finish(
                &mut history,
                1,
                MessageLevel::Info,
                "Interrupted. Result unknown.",
            );
            history.summarize(
                MessageLevel::Error,
                "Statement 1: operation did not finish",
                Some(key),
                state,
            );
        }
        let lines = viewport(&mut history, &mut 0, 90, 30, &Theme::ansi());
        let shown = text(&lines);
        assert!(shown.contains(label), "{shown}");
        assert!(!shown.contains("Completed"), "{shown}");
    }
}

#[test]
fn filtered_system_commands_keep_their_blocks_until_the_setting_shows_them() {
    let mut history = History::default();
    let header = "Command 1 · app_db · Shared\n";
    history.on_event(LogEvent::Started {
        id: 1,
        key: None,
        user: false,
        system: true,
        header_len: header.len(),
        dialect: Dialect::Postgres,
        text: format!("{header}SELECT internal_metadata;"),
    });
    finish(&mut history, 1, MessageLevel::Info, "Completed.");
    let theme = Theme::ansi();
    assert!(viewport(&mut history, &mut 0, 90, 30, &theme).is_empty());
    history.set_show_system_commands(true);
    assert!(text(&viewport(&mut history, &mut 0, 90, 30, &theme)).contains("#1 · app_db · Shared"));
    history.set_show_system_commands(false);
    assert!(viewport(&mut history, &mut 0, 90, 30, &theme).is_empty());
    assert_eq!(history.len(), 1);
}

#[test]
fn compact_blocks_follow_the_end_without_moving_a_reader_of_older_commands() {
    let mut history = History::default();
    for id in 1..=20 {
        begin(
            &mut history,
            id,
            &format!("SELECT {id};"),
            Dialect::Postgres,
        );
        finish(&mut history, id, MessageLevel::Info, "Completed.");
    }
    let theme = Theme::ansi();
    let mut scroll = 0;
    assert!(text(&viewport(&mut history, &mut scroll, 80, 6, &theme)).contains("SELECT 20;"));
    scroll = 0;
    let older = text(&viewport(&mut history, &mut scroll, 80, 6, &theme));
    assert!(older.contains("SELECT 1;"));
    begin(&mut history, 21, "SELECT 21;", Dialect::Postgres);
    finish(&mut history, 21, MessageLevel::Info, "Completed.");
    assert_eq!(
        text(&viewport(&mut history, &mut scroll, 80, 6, &theme)),
        older
    );
    scroll = usize::MAX;
    assert!(text(&viewport(&mut history, &mut scroll, 80, 6, &theme)).contains("SELECT 21;"));
    assert!(text(&viewport(&mut history, &mut scroll, 25, 6, &theme)).contains("SELECT 21;"));
}

#[test]
fn truncated_commands_keep_their_time_and_fit_the_history_budget() {
    let mut history = History::with_limit(4096);
    let key = begin(
        &mut history,
        1,
        &format!("SELECT '{}';", "é".repeat(10_000)),
        Dialect::Postgres,
    );
    finish(
        &mut history,
        1,
        MessageLevel::Error,
        &"error detail ".repeat(1000),
    );
    history.summarize(
        MessageLevel::Error,
        &format!("Statement 1: {}", "error detail ".repeat(1000)),
        Some(key),
        CommandState::Failed,
    );
    let mut scroll = 0;
    let lines = viewport(&mut history, &mut scroll, 90, 100, &Theme::ansi());
    let shown = text(&lines);
    assert!(shown.contains("truncated"), "{shown}");
    assert!(shown.contains("8.420 ms"), "{shown}");
    assert!(history.bytes() <= 4096);
}
