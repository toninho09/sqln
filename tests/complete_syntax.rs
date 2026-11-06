use crate::common::{Fixture, ctrl, key};
use crossterm::event::KeyCode::{Esc, Tab};
use edtui::Index2;
use sqln::complete::{Completion, SuggestionKind, suggest};
use sqln::editor::KeyboardMode;
use sqln::sql::Dialect;

const DIALECTS: [Dialect; 3] = [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite];

fn run(marked: &str, dialect: Dialect) -> Completion {
    let cursor = marked.find('|').unwrap();
    suggest(&marked.replacen('|', "", 1), cursor, dialect, None)
}

fn labels(marked: &str, dialect: Dialect) -> Vec<String> {
    run(marked, dialect)
        .items
        .into_iter()
        .map(|item| item.label)
        .collect()
}

#[test]
fn column_types_follow_the_connection_without_a_catalog() {
    for dialect in DIALECTS {
        for sql in [
            "CREATE TABLE t (id |)",
            "CREATE TEMPORARY TABLE IF NOT EXISTS t (id |)",
            "CREATE TABLE public.t (name VARCHAR(20), id |)",
            "CREATE TABLE t (\"Big Name\" |)",
            "ALTER TABLE t ADD COLUMN id |",
            "ALTER TABLE t ADD id |",
        ] {
            let sql = if dialect == Dialect::MySql {
                sql.replace('"', "`")
            } else {
                sql.to_owned()
            };
            let found = run(&sql, dialect);
            assert!(
                found.items.iter().any(|item| item.label == "INTEGER"),
                "{sql}: {found:?}"
            );
            assert!(
                found
                    .items
                    .iter()
                    .all(|item| item.kind == SuggestionKind::DataType)
            );
            assert!(found.missing_columns.is_empty());
        }
    }
    assert!(labels("CREATE TABLE t (id JS|)", Dialect::Postgres).contains(&"JSONB".to_owned()));
    assert_eq!(
        labels("CREATE TABLE t (id DATET|)", Dialect::MySql),
        ["DATETIME"]
    );
    assert!(labels("CREATE TABLE t (id JS|)", Dialect::Sqlite).is_empty());
    assert!(labels("CREATE TABLE t (id DATET|)", Dialect::Postgres).is_empty());
}

#[test]
fn names_and_type_parameters_do_not_suggest_existing_objects() {
    for sql in [
        "CREATE TABLE |",
        "CREATE TABLE IF NOT EXISTS |",
        "CREATE TABLE t (|)",
        "CREATE TABLE t (first INTEGER, |)",
        "CREATE TABLE t (id VARCHAR(|))",
        "CREATE TABLE t (id NUMERIC(10, |))",
        "ALTER TABLE t ADD COLUMN |",
        "ALTER TABLE t ADD COLUMN id NUMERIC(10, |)",
    ] {
        for dialect in DIALECTS {
            assert!(labels(sql, dialect).is_empty(), "{sql}, {dialect:?}");
        }
    }
}

#[test]
fn altered_type_parameters_do_not_offer_expressions() {
    assert!(
        labels(
            "ALTER TABLE t ALTER COLUMN id TYPE NUMERIC(10, |)",
            Dialect::Postgres
        )
        .is_empty()
    );
}

#[test]
fn types_work_with_a_partial_word_and_a_cursor_in_the_word() {
    let found = run("CREATE TABLE t (id int|garbage)", Dialect::Postgres);
    let integer = found
        .items
        .iter()
        .find(|item| item.label == "integer")
        .unwrap();
    let mut sql = "CREATE TABLE t (id intgarbage)".to_owned();
    sql.replace_range(found.replace, &integer.insert);
    assert_eq!(sql, "CREATE TABLE t (id integer)");
}

#[test]
fn casts_use_types_that_the_dialect_accepts() {
    assert_eq!(
        labels("SELECT CAST(value AS INT|)", Dialect::Postgres),
        ["INTEGER", "INTERVAL"]
    );
    assert_eq!(
        labels("SELECT CAST(value AS SIG|)", Dialect::MySql),
        ["SIGNED"]
    );
    assert!(labels("SELECT CAST(value AS INT|)", Dialect::MySql).is_empty());
    assert!(labels("SELECT CAST(value AS SER|)", Dialect::Postgres).is_empty());
    for dialect in DIALECTS {
        assert!(
            run("SELECT CAST(value AS |)", dialect)
                .items
                .iter()
                .all(|item| item.kind == SuggestionKind::DataType)
        );
    }
    assert!(labels("SELECT value AS |", Dialect::Postgres).is_empty());
    assert_eq!(
        labels("SELECT CAST(value AS DOUBLE PRE|)", Dialect::Postgres),
        ["PRECISION"]
    );
    assert_eq!(
        labels("CREATE TABLE t (value DOUBLE PRE|)", Dialect::Postgres),
        ["PRECISION"]
    );
    assert!(labels("SELECT CAST(value AS INTEGER |)", Dialect::Postgres).is_empty());
}

#[test]
fn alter_operations_and_type_changes_follow_the_dialect() {
    let pg = labels("ALTER TABLE t |", Dialect::Postgres);
    assert!(pg.contains(&"ALTER COLUMN".to_owned()));
    assert!(!pg.contains(&"MODIFY COLUMN".to_owned()));
    let mysql = labels("ALTER TABLE t |", Dialect::MySql);
    assert!(mysql.contains(&"MODIFY COLUMN".to_owned()));
    let sqlite = labels("ALTER TABLE t |", Dialect::Sqlite);
    assert!(!sqlite.contains(&"ALTER COLUMN".to_owned()));
    for sql in [
        "ALTER TABLE t ALTER COLUMN id TYPE |",
        "ALTER TABLE t ALTER id TYPE |",
    ] {
        assert!(labels(sql, Dialect::Postgres).contains(&"INTEGER".to_owned()));
    }
    for sql in [
        "ALTER TABLE t MODIFY COLUMN id |",
        "ALTER TABLE t CHANGE COLUMN old_id id |",
    ] {
        assert!(labels(sql, Dialect::MySql).contains(&"INTEGER".to_owned()));
    }
}

#[test]
fn column_options_do_not_include_functions_or_query_clauses() {
    for dialect in DIALECTS {
        let found = labels("CREATE TABLE t (id INTEGER |)", dialect);
        for option in ["PRIMARY KEY", "NOT NULL", "DEFAULT", "UNIQUE", "REFERENCES"] {
            assert!(found.contains(&option.to_owned()), "{dialect:?}: {found:?}");
        }
        for invalid in ["SELECT", "WHERE", "COUNT", "INTEGER"] {
            assert!(!found.contains(&invalid.to_owned()));
        }
        assert_eq!(
            labels("CREATE TABLE t (id INTEGER NOT |)", dialect),
            ["NULL"]
        );
        assert_eq!(
            labels("CREATE TABLE t (id INTEGER PRIMARY |)", dialect),
            ["KEY"]
        );
        let after = labels("CREATE TABLE t (id INTEGER NOT NULL |)", dialect);
        assert!(!after.contains(&"NOT NULL".to_owned()));
        assert!(!after.contains(&"NULL".to_owned()));
    }
    assert_eq!(
        labels("CREATE TABLE t (id INTEGER AUTO_|)", Dialect::MySql),
        ["AUTO_INCREMENT"]
    );
    assert!(labels("CREATE TABLE t (id INTEGER AUTO_|)", Dialect::Postgres).is_empty());
    assert_eq!(
        labels(
            "CREATE TABLE t (id INTEGER PRIMARY KEY AUTO|)",
            Dialect::Sqlite
        ),
        ["AUTOINCREMENT"]
    );
    assert!(labels("CREATE TABLE t (name TEXT AUTO|)", Dialect::MySql).is_empty());
}

#[test]
fn defaults_allow_functions_and_nested_casts() {
    assert_eq!(
        labels("CREATE TABLE t (name TEXT DEFAULT LOW|)", Dialect::Postgres),
        ["LOWER"]
    );
    assert_eq!(
        labels(
            "CREATE TABLE t (name TEXT DEFAULT (LOW|))",
            Dialect::Postgres
        ),
        ["LOWER"]
    );
    assert_eq!(
        labels(
            "CREATE TABLE t (id INTEGER DEFAULT CAST(1 AS INT|))",
            Dialect::Postgres
        ),
        ["INTEGER", "INTERVAL"]
    );
}

#[test]
fn functions_show_signatures_and_insert_empty_parentheses() {
    for dialect in DIALECTS {
        for sql in [
            "SELECT COU|",
            "SELECT * FROM t WHERE COU|",
            "SELECT * FROM t HAVING COU|",
        ] {
            let found = run(sql, dialect);
            let count = found
                .items
                .iter()
                .find(|item| item.label == "COUNT")
                .unwrap();
            assert_eq!(count.insert, "COUNT()");
            assert_eq!(count.kind, SuggestionKind::Function);
            assert_eq!(count.detail.as_deref(), Some("COUNT(expression | *)"));
            assert_eq!(count.cursor_offset, Some(6));
        }
        let lower = run("SELECT cou|", dialect);
        assert_eq!(lower.items[0].label, "count");
        assert_eq!(lower.items[0].insert, "count()");
        for sql in [
            "|",
            "SELECT * FROM COU|",
            "SELECT id AS COU|",
            "SELECT \"COU|",
        ] {
            assert!(
                !run(sql, dialect)
                    .items
                    .iter()
                    .any(|item| item.kind == SuggestionKind::Function)
            );
        }
    }
}

#[test]
fn date_functions_follow_the_dialect() {
    assert_eq!(labels("SELECT DATE_T|", Dialect::Postgres), ["DATE_TRUNC"]);
    assert!(labels("SELECT DATE_T|", Dialect::MySql).is_empty());
    assert_eq!(labels("SELECT DATE_F|", Dialect::MySql), ["DATE_FORMAT"]);
    assert_eq!(labels("SELECT STRFT|", Dialect::Sqlite), ["STRFTIME"]);
    assert!(labels("SELECT STRFT|", Dialect::Postgres).is_empty());
}

#[test]
fn query_clauses_follow_the_cursor_and_do_not_offer_ddl() {
    for dialect in DIALECTS {
        for (sql, expected) in [
            ("SELECT * FROM t ORD|", "ORDER BY"),
            ("SELECT * FROM t GRO|", "GROUP BY"),
            ("SELECT * FROM t WHERE id = 1 GRO|", "GROUP BY"),
            ("SELECT city, COUNT(*) FROM t GROUP BY city HAV|", "HAVING"),
            ("SELECT * FROM t ORDER |", "BY"),
            ("SELECT * FROM t GROUP |", "BY"),
            ("SELECT * FROM t LIMIT 5 OFF|", "OFFSET"),
            ("SELECT * FROM t LEFT |", "JOIN"),
            ("SELECT * FROM t UNION |", "SELECT"),
            ("SELECT * FROM t UNION ALL |", "SELECT"),
        ] {
            assert!(
                labels(sql, dialect).contains(&expected.to_owned()),
                "{sql}, {dialect:?}"
            );
        }
        assert_eq!(
            labels("SELECT * FROM t JOIN other o |", dialect),
            ["ON", "USING"]
        );
        let nested = labels("SELECT * FROM t WHERE (name |)", dialect);
        assert!(nested.contains(&"LIKE".to_owned()));
        assert!(!nested.contains(&"CREATE".to_owned()));
        let after_from = labels("SELECT * FROM t |", dialect);
        assert!(after_from.contains(&"WHERE".to_owned()));
        for invalid in ["CREATE", "DROP", "COUNT", "WHEN"] {
            assert!(!after_from.contains(&invalid.to_owned()));
        }
        let after_order = labels("SELECT * FROM t ORDER BY id |", dialect);
        assert!(after_order.contains(&"ASC".to_owned()));
        assert!(after_order.contains(&"DESC".to_owned()));
        assert!(!after_order.contains(&"WHERE".to_owned()));
        assert!(!after_order.contains(&"GROUP BY".to_owned()));
    }
}

#[test]
fn predicates_and_null_ordering_have_contextual_continuations() {
    for dialect in DIALECTS {
        let found = labels("SELECT * FROM t WHERE name |", dialect);
        for word in ["LIKE", "IN", "IS NULL", "IS NOT NULL"] {
            assert!(found.contains(&word.to_owned()));
        }
        assert_eq!(
            labels("SELECT * FROM t WHERE name IS |", dialect),
            ["FALSE", "NOT NULL", "NULL", "TRUE"]
        );
        assert_eq!(
            labels("SELECT * FROM t WHERE name IS NOT |", dialect),
            ["FALSE", "NULL", "TRUE"]
        );
        assert!(labels("SELECT * FROM t WHERE name NOT |", dialect).contains(&"LIKE".to_owned()));
    }
    for dialect in [Dialect::Postgres, Dialect::Sqlite] {
        assert_eq!(
            labels("SELECT * FROM t ORDER BY name NULLS |", dialect),
            ["FIRST", "LAST"]
        );
        assert!(
            labels("SELECT * FROM t ORDER BY name DESC |", dialect)
                .contains(&"NULLS LAST".to_owned())
        );
    }
    assert!(
        !labels("SELECT * FROM t ORDER BY name |", Dialect::MySql)
            .contains(&"NULLS LAST".to_owned())
    );
}

#[test]
fn closed_subqueries_and_other_statements_do_not_change_the_query_context() {
    assert_eq!(
        labels(
            "SELECT (SELECT id FROM t ORDER BY id) FROM t GRO|",
            Dialect::Postgres
        ),
        ["GROUP BY"]
    );
    assert_eq!(
        labels(
            "SELECT * FROM (SELECT id FROM t GRO|) sub",
            Dialect::Postgres
        ),
        ["GROUP BY"]
    );
    assert_eq!(
        labels(
            "CREATE TABLE t (id INTEGER); SELECT COU|",
            Dialect::Postgres
        ),
        ["COUNT"]
    );
    assert_eq!(
        labels("SELECT 1; CREATE TABLE t (id INT|)", Dialect::Postgres),
        ["INTEGER", "INTERVAL"]
    );
}

#[test]
fn literals_and_comments_keep_the_list_closed_in_new_contexts() {
    for dialect in DIALECTS {
        for sql in [
            "CREATE TABLE t (id /* INT|",
            "SELECT 'COU|'",
            "CREATE TABLE t (id NUMERIC(12|))",
            "SELECT -- COU|",
        ] {
            assert!(labels(sql, dialect).is_empty());
        }
    }
    assert!(run("COU|", Dialect::Redis).items.is_empty());
}

#[test]
fn accepting_a_function_places_the_cursor_inside_and_keeps_one_undo_step() {
    for dialect in DIALECTS {
        let mut f = Fixture::with_dialect("SELECT ", KeyboardMode::Conventional, dialect);
        f.editor.set_cursor(Index2::new(0, 7));
        f.write("cou");
        f.press(key(Tab));
        assert_eq!(f.text(), "SELECT count()");
        assert_eq!(f.editor.cursor(), Index2::new(0, 13));
        f.write("*");
        assert_eq!(f.text(), "SELECT count(*)");
        f.press(ctrl('z'));
        assert_eq!(f.text(), "SELECT count()");
        f.press(ctrl('z'));
        assert_eq!(f.text(), "SELECT cou");
        f.press(ctrl('y'));
        assert_eq!(f.text(), "SELECT count()");
        assert_eq!(f.editor.cursor(), Index2::new(0, 13));
    }
}

#[test]
fn function_acceptance_reuses_parentheses_and_keeps_existing_arguments() {
    for tail in ["()", "(id)", " (id)", " /* note */ (id)", "\n(id)"] {
        let mut f = Fixture::conventional(&format!("SELECT {tail}"));
        f.editor.set_cursor(Index2::new(0, 7));
        f.write("cou");
        f.press(key(Tab));
        assert_eq!(f.text(), format!("SELECT count{tail}"));
        let at = f.text().find('(').unwrap() + 1;
        if tail.contains('\n') {
            assert_eq!(f.editor.cursor(), Index2::new(1, 1));
        } else {
            assert_eq!(f.editor.cursor(), Index2::new(0, at));
        }
        f.write("*");
        assert!(f.text().contains("(*"));
    }
}

#[test]
fn function_acceptance_works_with_unicode_a_later_line_and_vim() {
    let mut f = Fixture::conventional("SELECT 'ação';\nSELECT '日本', ");
    f.editor.set_cursor(Index2::new(1, 13));
    f.write("COU");
    f.press(key(Tab));
    assert_eq!(f.text(), "SELECT 'ação';\nSELECT '日本', COUNT()");
    assert_eq!(f.editor.cursor(), Index2::new(1, 19));

    let mut f = Fixture::vim("");
    f.write("iSELECT COU");
    f.press(key(Tab));
    assert_eq!(f.text(), "SELECT COUNT()");
    assert_eq!(f.editor.cursor(), Index2::new(0, 13));
    f.press(key(Esc));
    f.write("u");
    assert_eq!(f.text(), "SELECT COU");
}

#[test]
fn all_character_boundaries_accept_incomplete_grammar_without_panics() {
    for sql in [
        "CREATE TABLE IF NOT EXISTS t (\"日本\" NUMERIC(10, 2), id INTEGER DEFAULT (COALESCE(1, 2)))",
        "ALTER TABLE t ADD COLUMN id INTEGER",
        "SELECT CAST(COUNT(*) AS INTEGER), name FROM t GROUP BY name HAVING COUNT(*) > 1 ORDER BY name DESC NULLS LAST",
    ] {
        for (cursor, _) in sql.char_indices().chain([(sql.len(), ' ')]) {
            for dialect in DIALECTS {
                suggest(sql, cursor, dialect, None);
            }
        }
    }
}
