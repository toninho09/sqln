// Properties of the lexer, the statement splitter, the formatter, and the literals, on
// generated text. Each failure that proptest finds becomes a fixed case below.

use proptest::prelude::*;
use rusqlite::Connection;
use rusqlite::types::ValueRef;
use sqln_sql::sql::literal::{bytes_literal, text_literal};
use sqln_sql::sql::{
    Dialect, format_statement, quote_identifier, quote_literal, split, statement_at, tokenize,
};

const SQL_DIALECTS: [Dialect; 5] = [
    Dialect::Postgres,
    Dialect::MySql,
    Dialect::Sqlite,
    Dialect::DuckDb,
    Dialect::Databricks,
];

fn dialect() -> impl Strategy<Value = Dialect> {
    proptest::sample::select(Dialect::ALL.to_vec())
}

fn sql_dialect() -> impl Strategy<Value = Dialect> {
    proptest::sample::select(SQL_DIALECTS.to_vec())
}

/// Text made of the pieces that change the state of a lexer, with other characters.
fn sqlish() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        Just("SELECT ".to_owned()),
        Just("FROM t ".to_owned()),
        Just("WHERE a = 1 AND b BETWEEN 1 AND 2 ".to_owned()),
        Just("(SELECT 1) ".to_owned()),
        Just("CASE WHEN x THEN 1 END ".to_owned()),
        Just("'".to_owned()),
        Just("\"".to_owned()),
        Just("`".to_owned()),
        Just("\\".to_owned()),
        Just("$$".to_owned()),
        Just("$tag$".to_owned()),
        Just("E'".to_owned()),
        Just("--".to_owned()),
        Just("#".to_owned()),
        Just("/*".to_owned()),
        Just("*/".to_owned()),
        Just(";".to_owned()),
        Just("\n".to_owned()),
        Just(", ".to_owned()),
        Just("é ".to_owned()),
        Just("名前".to_owned()),
        Just("😀".to_owned()),
        "[a-z0-9_ .()=<>+*-]{0,6}",
    ];
    proptest::collection::vec(piece, 0..24).prop_map(|pieces| pieces.concat())
}

fn any_sql() -> impl Strategy<Value = String> {
    prop_oneof![sqlish(), any::<String>()]
}

/// Text without NUL, which a SQLite statement cannot hold.
fn literal_text() -> impl Strategy<Value = String> {
    prop_oneof![
        "[^\u{0}]{0,40}",
        proptest::collection::vec(
            proptest::sample::select(vec!["'", "\"", "\\", "\n", "\r", "a", "é", "%", "_", "\\x"]),
            0..16,
        )
        .prop_map(|pieces| pieces.concat()),
    ]
}

/// Reads one value with SQLite, as SQLN writes it in a statement.
fn sqlite_value(connection: &Connection, literal: &str) -> rusqlite::Result<Vec<u8>> {
    connection.query_row(&format!("SELECT {literal}"), [], |row| {
        Ok(match row.get_ref(0)? {
            ValueRef::Text(bytes) | ValueRef::Blob(bytes) => bytes.to_vec(),
            other => format!("{other:?}").into_bytes(),
        })
    })
}

/// Text that holds `inner` and then a second statement. A literal or an identifier that
/// ends early gives another count of statements.
fn statement_count_with(inner: &str, dialect: Dialect) -> usize {
    split(&format!("SELECT {inner};\nSELECT 2;"), dialect).len()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn tokens_are_ordered_and_inside_the_text(sql in any_sql(), dialect in dialect()) {
        let tokens = tokenize(&sql, dialect);
        let mut previous_end = 0;
        for token in &tokens {
            prop_assert!(token.start() >= previous_end, "{token:?} overlaps in {sql:?}");
            prop_assert!(token.start() < token.end(), "{token:?} is empty in {sql:?}");
            prop_assert!(token.end() <= sql.len(), "{token:?} passes the end of {sql:?}");
            prop_assert!(sql.is_char_boundary(token.start()) && sql.is_char_boundary(token.end()));
            previous_end = token.end();
        }
    }

    #[test]
    fn statements_are_ordered_and_disjoint(sql in any_sql(), dialect in dialect()) {
        let statements = split(&sql, dialect);
        let mut previous_end = 0;
        for statement in &statements {
            prop_assert!(statement.start >= previous_end, "{statement:?} overlaps in {sql:?}");
            prop_assert!(statement.start <= statement.end);
            prop_assert!(statement.end <= statement.end_with_delimiter);
            prop_assert!(statement.end_with_delimiter <= sql.len());
            prop_assert!(sql.is_char_boundary(statement.start));
            prop_assert!(sql.is_char_boundary(statement.end));
            prop_assert!(sql.is_char_boundary(statement.end_with_delimiter));
            previous_end = statement.end_with_delimiter;
        }
    }

    #[test]
    fn the_statement_at_a_cursor_holds_the_cursor(
        sql in any_sql(),
        dialect in dialect(),
        at in any::<proptest::sample::Index>(),
    ) {
        let cursor = at.index(sql.len() + 1);
        if let Some(statement) = statement_at(&sql, dialect, cursor) {
            prop_assert!(statement.start <= cursor && cursor <= statement.end_with_delimiter);
        }
    }

    #[test]
    fn formatting_twice_changes_nothing(sql in any_sql(), dialect in dialect()) {
        if let Some(formatted) = format_statement(&sql, dialect) {
            let again = format_statement(&formatted, dialect);
            prop_assert_eq!(again.as_deref(), Some(formatted.as_str()), "input {:?}", sql);
        }
    }

    #[test]
    fn literals_and_identifiers_stay_one_value(
        text in literal_text(),
        dialect in sql_dialect(),
    ) {
        for inner in [
            quote_literal(&text, dialect),
            text_literal(&text, dialect),
            bytes_literal(text.as_bytes(), dialect),
            quote_identifier(&text, dialect),
        ] {
            prop_assert_eq!(statement_count_with(&inner, dialect), 2, "{:?} as {:?}", text, inner);
        }
    }

    #[test]
    fn sqlite_reads_back_each_literal(text in literal_text()) {
        let connection = Connection::open_in_memory().unwrap();
        for literal in [
            quote_literal(&text, Dialect::Sqlite),
            text_literal(&text, Dialect::Sqlite),
        ] {
            prop_assert_eq!(sqlite_value(&connection, &literal).unwrap(), text.as_bytes());
        }
        let bytes = sqlite_value(&connection, &bytes_literal(text.as_bytes(), Dialect::Sqlite));
        prop_assert_eq!(bytes.unwrap(), text.as_bytes());
        let name = quote_identifier(&text, Dialect::Sqlite);
        let read = connection.query_row(&format!("SELECT 1 AS {name}"), [], |row| {
            Ok(row.as_ref().column_name(0)?.to_owned())
        });
        prop_assert_eq!(read.unwrap(), text);
    }
}
