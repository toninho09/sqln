// The SQL formatter: its style, and the rule that no token changes.

mod common;

use common::{Fixture, ctrl, key};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use edtui::Index2;
use sqln::sql::{Dialect, format_statement, tokenize};

const DIALECTS: [Dialect; 3] = [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite];

fn format(sql: &str) -> String {
    format_statement(sql, Dialect::Postgres).unwrap()
}

fn tokens_of(sql: &str, dialect: Dialect) -> Vec<&str> {
    let tokens = tokenize(sql, dialect);
    let texts = tokens.iter().map(|token| &sql[token.start..token.end]);
    texts.collect()
}

#[test]
fn each_clause_starts_a_line() {
    assert_eq!(
        format(
            "select a from t join u on t.id = u.id where a = 1 group by a having a > 1 order by a limit 5 offset 1"
        ),
        "select a\nfrom t\njoin u on t.id = u.id\nwhere a = 1\ngroup by a\nhaving a > 1\norder by a\nlimit 5\noffset 1"
    );
}

#[test]
fn a_list_has_the_comma_at_the_end_and_an_indent() {
    assert_eq!(
        format("SELECT a, b,c FROM t ORDER BY a, b"),
        "SELECT a,\n  b,\n  c\nFROM t\nORDER BY a,\n  b"
    );
}

#[test]
fn and_or_start_a_line_in_a_condition() {
    assert_eq!(
        format("SELECT 1 FROM t WHERE a = 1 AND b = 2 OR c = 3"),
        "SELECT 1\nFROM t\nWHERE a = 1\n  AND b = 2\n  OR c = 3"
    );
    assert_eq!(
        format("SELECT 1 FROM t JOIN u ON a = b AND c = d"),
        "SELECT 1\nFROM t\nJOIN u ON a = b\n  AND c = d"
    );
}

#[test]
fn the_and_of_between_stays_in_the_line() {
    assert_eq!(
        format("SELECT 1 FROM t WHERE a BETWEEN 1 AND 2 AND b = 3"),
        "SELECT 1\nFROM t\nWHERE a BETWEEN 1 AND 2\n  AND b = 3"
    );
}

#[test]
fn a_subquery_has_an_indent_and_a_call_stays_in_one_line() {
    assert_eq!(
        format("SELECT coalesce(a, b) FROM (SELECT a, b FROM t WHERE a = 1) AS x"),
        "SELECT coalesce(a, b)\nFROM (\n  SELECT a,\n    b\n  FROM t\n  WHERE a = 1\n) AS x"
    );
}

#[test]
fn with_starts_one_line_for_each_query() {
    assert_eq!(
        format("WITH x AS (SELECT 1), y AS (SELECT 2) DELETE FROM t WHERE a IN (SELECT 1)"),
        "WITH x AS (\n  SELECT 1\n),\ny AS (\n  SELECT 2\n)\nDELETE FROM t\nWHERE a IN (\n  SELECT 1\n)"
    );
}

#[test]
fn writes_have_the_same_style() {
    assert_eq!(
        format("UPDATE t SET a = 1, b = 2 WHERE c = 3"),
        "UPDATE t\nSET a = 1,\n  b = 2\nWHERE c = 3"
    );
    assert_eq!(
        format(
            "INSERT INTO t (a, b) VALUES (1, 2), (3, 4) ON CONFLICT (a) DO UPDATE SET b = 5 RETURNING a"
        ),
        "INSERT INTO t (a, b)\nVALUES (1, 2),\n  (3, 4)\nON CONFLICT (a) DO UPDATE\nSET b = 5\nRETURNING a"
    );
    assert_eq!(format("DELETE FROM t"), "DELETE FROM t");
}

#[test]
fn a_word_with_another_role_starts_no_line() {
    for sql in [
        "SELECT a IS DISTINCT FROM b",
        "SELECT extract(year FROM a)",
        "SELECT t.from",
        "SELECT left(a, 1)",
        "SELECT sum(a) OVER (PARTITION BY b ORDER BY c)",
        "SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY a)",
        "SELECT CASE WHEN a = 1 AND b = 2 THEN 'x' ELSE 'y' END",
        "SELECT CASE WHEN a IN (SELECT 1 FROM t) THEN 1 END",
        "INSERT INTO t DEFAULT VALUES",
    ] {
        assert_eq!(format(sql), sql);
    }
    assert_eq!(
        format("SELECT a FROM t FOR UPDATE"),
        "SELECT a\nFROM t\nFOR UPDATE"
    );
}

#[test]
fn a_join_phrase_stays_in_one_line() {
    assert_eq!(
        format("SELECT 1 FROM t LEFT OUTER JOIN u ON a = b CROSS JOIN v NATURAL LEFT JOIN w"),
        "SELECT 1\nFROM t\nLEFT OUTER JOIN u ON a = b\nCROSS JOIN v\nNATURAL LEFT JOIN w"
    );
}

#[test]
fn a_gap_between_tokens_never_appears_or_goes() {
    assert_eq!(
        format("SELECT a::int,b->>'c' FROM t WHERE a>=1 AND b<>2"),
        "SELECT a::int,\n  b->>'c'\nFROM t\nWHERE a>=1\n  AND b<>2"
    );
    assert_eq!(format("SELECT   1  +   2"), "SELECT 1 + 2");
}

#[test]
fn the_case_of_the_text_stays() {
    assert_eq!(format("SeLeCt a FrOm t"), "SeLeCt a\nFrOm t");
}

#[test]
fn the_text_in_strings_and_quotes_stays() {
    let sql = "SELECT 'a,  b\n  from c', \"x  y\", $tag$ select  1 from t $tag$";
    let formatted = format(sql);
    assert!(formatted.contains("'a,  b\n  from c'"), "{formatted}");
    assert!(formatted.contains("\"x  y\""), "{formatted}");
    assert!(
        formatted.contains("$tag$ select  1 from t $tag$"),
        "{formatted}"
    );
}

#[test]
fn two_strings_on_two_lines_stay_on_two_lines() {
    assert_eq!(format("SELECT 'a'\n'b'"), "SELECT 'a'\n'b'");
}

#[test]
fn a_line_comment_ends_with_its_line() {
    assert_eq!(
        format("SELECT a, -- first\n b -- second\n FROM t"),
        "SELECT a, -- first\n  b -- second\nFROM t"
    );
}

#[test]
fn a_comment_on_its_own_line_stays_on_its_own_line() {
    assert_eq!(
        format("SELECT a,\n-- note\nb\nFROM t /* last */ WHERE a = 1"),
        "SELECT a,\n  -- note\n  b\nFROM t /* last */\nWHERE a = 1"
    );
}

#[test]
fn another_kind_of_statement_stays_as_it_is() {
    for sql in [
        "CREATE TABLE t (a int,   b int)",
        "ALTER TABLE t ADD a  int",
        "DROP TABLE   t",
        "BEGIN",
    ] {
        assert_eq!(format(sql), sql);
    }
    let routine = "CREATE PROCEDURE p() BEGIN SELECT 1; SELECT   2; END";
    assert_eq!(
        format_statement(routine, Dialect::MySql),
        Some(routine.to_owned())
    );
}

#[test]
fn redis_stays_as_it_is() {
    assert_eq!(
        format_statement("SELECT 1", Dialect::Redis),
        Some("SELECT 1".to_owned())
    );
}

#[test]
fn an_open_token_stops_the_format() {
    assert_eq!(format_statement("SELECT 'a", Dialect::Postgres), None);
    assert_eq!(format_statement("SELECT /* a", Dialect::Postgres), None);
    assert_eq!(format_statement("SELECT \"a", Dialect::Postgres), None);
}

#[test]
fn the_comment_of_mysql_counts_as_a_comment() {
    assert_eq!(
        format_statement("SELECT a # note\nFROM t", Dialect::MySql),
        Some("SELECT a # note\nFROM t".to_owned())
    );
}

/// Each statement stays with the same tokens, and a second format changes nothing.
#[test]
fn no_token_changes_and_a_second_format_changes_nothing() {
    let statements = [
        "select a,b,c from t where a=1 and (b=2 or c=3) order by a",
        "WITH RECURSIVE r AS (SELECT 1 UNION ALL SELECT n + 1 FROM r) SELECT * FROM r",
        "select (select max(a) from t where b in (select c from u)) as m, 'x' from v",
        "SELECT a FROM t UNION ALL SELECT b FROM u INTERSECT SELECT c FROM v",
        "UPDATE t SET a = (SELECT 1 FROM u WHERE b = 2), c = 3",
        "INSERT INTO t SELECT * FROM u WHERE a NOT BETWEEN 1 AND 2 OR b",
        "SELECT a, -- one\n b /* two */, c\n-- three\nFROM t",
        "SELECT CASE a WHEN 1 THEN (SELECT 1) ELSE 2 END, b FROM t",
        "DELETE FROM t USING u WHERE t.a = u.a RETURNING t.a, t.b",
        "SELECT `a`, `b` FROM `t` WHERE `a` = 'it''s'",
        "SELECT [a] FROM t WHERE b >= 1 AND c != 2",
        "SELECT 'a'\n  'b', c FROM t",
        "VALUES (1, 2), (3, 4)",
    ];
    for dialect in DIALECTS {
        for sql in statements {
            let Some(once) = format_statement(sql, dialect) else {
                continue;
            };
            assert_eq!(tokens_of(sql, dialect), tokens_of(&once, dialect), "{sql}");
            assert_eq!(
                format_statement(&once, dialect).as_ref(),
                Some(&once),
                "{sql}"
            );
        }
    }
}

#[test]
fn the_sample_list_has_no_refusal_in_postgres() {
    assert!(format_statement("SELECT 1 FROM t", Dialect::Postgres).is_some());
}

fn format_key(f: &mut Fixture) {
    f.editor.format_sql().unwrap();
}

#[test]
fn the_editor_formats_the_statement_under_the_cursor_only() {
    let mut f = Fixture::conventional("SELECT a FROM t;\nSELECT b FROM u;\n");
    f.editor.set_cursor(Index2::new(1, 3));
    format_key(&mut f);
    assert_eq!(f.text(), "SELECT a FROM t;\nSELECT b\nFROM u;\n");
    assert_eq!(f.editor.cursor(), Index2::new(1, 0));
}

#[test]
fn the_editor_formats_each_statement_of_the_selection_with_one_undo() {
    let mut f = Fixture::conventional("SELECT a FROM t;\nSELECT b FROM u;\n");
    f.press(ctrl('a'));
    format_key(&mut f);
    assert_eq!(f.text(), "SELECT a\nFROM t;\nSELECT b\nFROM u;\n");
    assert_eq!(f.editor.selected_text(), None);
    f.press(ctrl('z'));
    assert_eq!(f.text(), "SELECT a FROM t;\nSELECT b FROM u;\n");
}

#[test]
fn a_cursor_outside_a_statement_refuses_like_a_run() {
    let mut f = Fixture::conventional("SELECT 1;\n\n\nSELECT 2;");
    f.editor.set_cursor(Index2::new(1, 0));
    assert!(f.editor.format_sql().is_err());
}

#[test]
fn a_formatted_text_reports_no_change_and_no_undo_step() {
    let mut f = Fixture::conventional("SELECT a\nFROM t");
    let revision = f.editor.revision();
    format_key(&mut f);
    assert_eq!(f.editor.revision(), revision);
}

#[test]
fn the_vim_mode_leaves_the_selection_after_a_format() {
    let mut f = Fixture::vim("SELECT a FROM t");
    f.press_all(&[
        key(KeyCode::Char('V')),
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    ]);
    f.press(key(KeyCode::Char('V')));
    format_key(&mut f);
    assert_eq!(f.text(), "SELECT a\nFROM t");
    assert!(!f.editor.is_inserting());
    assert_eq!(f.editor.selected_text(), None);
}
