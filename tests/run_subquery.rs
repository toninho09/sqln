use crate::common::{Fixture, key, shift};
use crossterm::event::KeyCode;
use edtui::Index2;
use sqln::editor::{KeyboardMode, RunRefusal, RunScope};
use sqln::sql::Dialect;

fn at_marker(sql: &str, dialect: Dialect) -> Fixture {
    let offset = sql.find('|').unwrap();
    let before = &sql[..offset];
    let row = before.bytes().filter(|byte| *byte == b'\n').count();
    let col = before.rsplit('\n').next().unwrap().chars().count();
    let mut f = Fixture::with_dialect(
        &sql.replacen('|', "", 1),
        KeyboardMode::Conventional,
        dialect,
    );
    f.editor.set_cursor(Index2::new(row, col));
    f
}

#[test]
fn subquery_run_chooses_the_smallest_query_and_keeps_source_offsets() {
    let mut f = at_marker(
        "-- ação\nSELECT (SELECT 2 + (\n  SELECT |'日本語' AS inner_value\n)) AS outer_value;",
        Dialect::Postgres,
    );
    let text = f.editor.text();
    let cursor = f.editor.cursor();
    let revision = f.editor.revision();
    let plan = f.editor.plan_run(RunScope::Subquery).unwrap();
    assert_eq!(plan.statements, ["SELECT '日本語' AS inner_value"]);
    assert_eq!(plan.rows, [2]);
    assert_eq!(plan.starts, [text.find("SELECT '日本語'").unwrap()]);
    assert_eq!(f.editor.text(), text);
    assert_eq!(f.editor.cursor(), cursor);
    assert_eq!(f.editor.revision(), revision);
    assert!(f.editor.plan_run(RunScope::Default).unwrap().statements[0].contains("outer_value"));
}

#[test]
fn subquery_run_preserves_selection_in_both_keyboard_modes() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut f = Fixture::new("SELECT (SELECT 42 AS answer) AS outer_value;", mode);
        f.editor.set_cursor(Index2::new(0, 15));
        if mode == KeyboardMode::Conventional {
            f.press(shift(KeyCode::Right));
        } else {
            f.press_all(&[key(KeyCode::Char('v')), key(KeyCode::Right)]);
        }
        let cursor = f.editor.cursor();
        let selection = f.editor.selection_range();
        assert!(selection.is_some());
        assert_eq!(
            f.editor.plan_run(RunScope::Subquery).unwrap().statements,
            ["SELECT 42 AS answer"]
        );
        assert_eq!(f.editor.selection_range(), selection);
        assert_eq!(f.editor.cursor(), cursor);
    }
}

#[test]
fn subquery_run_ignores_parentheses_and_keywords_in_opaque_tokens() {
    for (dialect, literal) in [
        (Dialect::Postgres, "$$ ) (SELECT fake) $$"),
        (Dialect::Postgres, "'it''s ) (SELECT fake)'"),
        (Dialect::MySql, "'it\\'s ) (SELECT fake)'"),
        (Dialect::Sqlite, "[) (SELECT fake)]"),
        (Dialect::DuckDb, "\") (SELECT fake)\""),
    ] {
        let expected = format!("SELECT {literal} AS value /* ) (SELECT fake) */");
        let mut f = at_marker(&format!("SELECT (|{expected}) AS outer_value;"), dialect);
        assert_eq!(
            f.editor.plan_run(RunScope::Subquery).unwrap().statements,
            [expected]
        );
    }
}

#[test]
fn subquery_run_keeps_line_comments_and_query_clauses() {
    let mut f = at_marker(
        "SELECT (/* before */ SELECT |1 -- ) fake\n UNION ALL SELECT 2 ORDER BY 1 LIMIT 1\n) AS x;",
        Dialect::Postgres,
    );
    assert_eq!(
        f.editor.plan_run(RunScope::Subquery).unwrap().statements,
        ["SELECT 1 -- ) fake\n UNION ALL SELECT 2 ORDER BY 1 LIMIT 1"]
    );
}

#[test]
fn subquery_run_accepts_cte_bodies_and_with_queries_without_adding_dependencies() {
    for (sql, expected) in [
        (
            "WITH x AS (SELECT |1 AS n) SELECT * FROM x;",
            "SELECT 1 AS n",
        ),
        (
            "SELECT (WITH x AS (SELECT 1 AS n) SELECT |n FROM x);",
            "WITH x AS (SELECT 1 AS n) SELECT n FROM x",
        ),
        (
            "SELECT * FROM items i WHERE EXISTS (SELECT |i.id);",
            "SELECT i.id",
        ),
        (
            "WITH x AS (SELECT 1) SELECT (SELECT |* FROM x);",
            "SELECT * FROM x",
        ),
    ] {
        let mut f = at_marker(sql, Dialect::Postgres);
        assert_eq!(
            f.editor.plan_run(RunScope::Subquery).unwrap().statements,
            [expected]
        );
    }
}

#[test]
fn subquery_run_has_no_whole_statement_fallback() {
    for sql in [
        "|",
        "SELECT 1;\n|\nSELECT (SELECT 2);",
        "SELECT (SELECT |1 /* incomplete );",
        "SELECT (SELECT |\'incomplete );",
        "SELECT |1;",
        "SELECT |(1 + 2);",
        "SELECT '|(SELECT 2)';",
        "SELECT 1 /* |(SELECT 2) */;",
        "SELECT (SELECT |1;",
        "SELECT |1; SELECT (SELECT 2);",
        "SELECT (SELECT 1) AS |x;",
    ] {
        let mut f = at_marker(sql, Dialect::Postgres);
        assert_eq!(
            f.editor.plan_run(RunScope::Subquery),
            Err(RunRefusal::NoSubquery),
            "{sql}"
        );
    }
}

#[test]
fn subquery_run_includes_parenthesis_boundaries_but_not_the_alias() {
    for sql in ["SELECT |(SELECT 1) AS x;", "SELECT (SELECT 1|) AS x;"] {
        let mut f = at_marker(sql, Dialect::Postgres);
        assert_eq!(
            f.editor.plan_run(RunScope::Subquery).unwrap().statements,
            ["SELECT 1"]
        );
    }
}

#[test]
fn subquery_run_does_not_offer_relational_queries_to_command_sources() {
    for dialect in [Dialect::Redis, Dialect::Kafka] {
        let mut f = at_marker("SELECT (SELECT |1);", dialect);
        assert_eq!(
            f.editor.plan_run(RunScope::Subquery),
            Err(RunRefusal::NoSubquery)
        );
    }
}

#[test]
fn subquery_run_updates_after_edits_and_undo() {
    let mut f = at_marker("SELECT (SELECT |1);", Dialect::Postgres);
    assert_eq!(
        f.editor.plan_run(RunScope::Subquery).unwrap().statements,
        ["SELECT 1"]
    );
    f.press(key(KeyCode::Char('2')));
    assert_eq!(
        f.editor.plan_run(RunScope::Subquery).unwrap().statements,
        ["SELECT 21"]
    );
    f.press(crate::common::ctrl('z'));
    assert_eq!(
        f.editor.plan_run(RunScope::Subquery).unwrap().statements,
        ["SELECT 1"]
    );
}
