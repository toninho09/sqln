// The literals and quotes that SQLN writes for each dialect.

use sqln_core::catalog::RelationRef;
use sqln_sql::sql::literal::{binary_literal, plain_binary_literal, text_literal};
use sqln_sql::sql::{Dialect, from_clause, quote_identifier, quote_literal};
use sqln_sql::structure::name_literal;

#[test]
fn each_dialect_writes_its_own_literals_and_quotes() {
    // (dialect, identifier, literal, text literal, binary, plain binary, catalog name)
    let cases = [
        (
            Dialect::Postgres,
            "\"a\"\"b\"",
            "'a\\b'",
            "E'a\\\\b'",
            "E'\\\\x00ff'",
            "'\\x00ff'",
            "E'a\\\\b'",
        ),
        (
            Dialect::MySql,
            "`a\"b`",
            "'a\\\\b'",
            "_utf8mb4 0x615C62",
            "X'00ff'",
            "X'00ff'",
            "'a\\\\b'",
        ),
        (
            Dialect::Sqlite,
            "\"a\"\"b\"",
            "'a\\b'",
            "'a\\b'",
            "X'00ff'",
            "X'00ff'",
            "'a\\b'",
        ),
        (
            Dialect::DuckDb,
            "\"a\"\"b\"",
            "'a\\b'",
            "'a\\b'",
            "X'00ff'",
            "X'00ff'",
            "'a\\b'",
        ),
        (
            Dialect::Redis,
            "\"a\"\"b\"",
            "'a\\b'",
            "'a\\b'",
            "X'00ff'",
            "X'00ff'",
            "'a\\b'",
        ),
        (
            Dialect::Kafka,
            "\"a\"\"b\"",
            "'a\\b'",
            "'a\\\\b'",
            "X'00ff'",
            "X'00ff'",
            "'a\\b'",
        ),
    ];
    for (dialect, identifier, literal, text, binary, plain, name) in cases {
        assert_eq!(quote_identifier("a\"b", dialect), identifier, "{dialect:?}");
        assert_eq!(quote_literal("a\\b", dialect), literal, "{dialect:?}");
        assert_eq!(text_literal("a\\b", dialect), text, "{dialect:?}");
        assert_eq!(binary_literal("00ff", dialect), binary, "{dialect:?}");
        assert_eq!(plain_binary_literal("00ff", dialect), plain, "{dialect:?}");
        assert_eq!(name_literal("a\\b", dialect), name, "{dialect:?}");
    }
}

#[test]
fn only_duckdb_quotes_each_part_of_a_dotted_schema() {
    let relation = RelationRef {
        schema: "db.main".to_owned(),
        relation: "t".to_owned(),
    };
    assert_eq!(
        from_clause(&relation, Dialect::DuckDb),
        "\"db\".\"main\".\"t\""
    );
    assert_eq!(
        from_clause(&relation, Dialect::Postgres),
        "\"db.main\".\"t\""
    );
    assert_eq!(from_clause(&relation, Dialect::MySql), "`db.main`.`t`");
}
