// These tests keep the rules that `spikes/tests/splitter.rs` approved.

use sqln::sql::{
    Dialect, TokenKind, counts_rows, is_plain_read, reads_or_writes_stdio,
    removes_prepared_statements, split, split_tokens, statement_at, tokenize, with_row_limit,
};

fn texts(sql: &str, dialect: Dialect) -> Vec<&str> {
    split(sql, dialect)
        .into_iter()
        .map(|statement| statement.text(sql))
        .collect()
}

#[test]
fn semicolons_in_strings_and_comments_do_not_split() {
    let sql = "SELECT 'a;b'; -- x;y\nSELECT /* c;d */ 2;";
    for dialect in [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite] {
        assert_eq!(
            texts(sql, dialect),
            ["SELECT 'a;b'", "SELECT /* c;d */ 2"],
            "{dialect:?}"
        );
    }
}

#[test]
fn doubled_quotes_stay_in_the_string() {
    assert_eq!(
        texts("SELECT 'it''s;'; SELECT 2", Dialect::Sqlite),
        ["SELECT 'it''s;'", "SELECT 2"]
    );
}

#[test]
fn several_statements_on_one_line_and_one_statement_on_several_lines() {
    assert_eq!(
        texts("SELECT 1; SELECT 2;\nSELECT\n  3\n;", Dialect::Postgres),
        ["SELECT 1", "SELECT 2", "SELECT\n  3"]
    );
}

#[test]
fn postgres_dollar_quotes_keep_function_bodies() {
    let sql = "CREATE FUNCTION f() RETURNS int AS $body$ BEGIN; RETURN 1; END; $body$ LANGUAGE plpgsql; SELECT 1;";
    let found = texts(sql, Dialect::Postgres);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].ends_with("LANGUAGE plpgsql"));
}

#[test]
fn postgres_positional_parameter_is_not_a_dollar_quote() {
    assert_eq!(
        texts("SELECT $1; SELECT $2;", Dialect::Postgres),
        ["SELECT $1", "SELECT $2"]
    );
}

#[test]
fn postgres_block_comments_nest() {
    assert_eq!(
        texts("/* a /* b; */ c; */ SELECT 1;", Dialect::Postgres),
        ["SELECT 1"]
    );
}

#[test]
fn postgres_escape_string_uses_backslash() {
    assert_eq!(
        texts(r"SELECT E'a\';b'; SELECT 2;", Dialect::Postgres),
        [r"SELECT E'a\';b'", "SELECT 2"]
    );
    assert_eq!(
        texts(r"SELECT 'a\'; SELECT 2;", Dialect::Postgres),
        [r"SELECT 'a\'", "SELECT 2"]
    );
}

#[test]
fn postgres_begin_atomic_is_one_statement() {
    let sql = "CREATE FUNCTION f() RETURNS int LANGUAGE sql BEGIN ATOMIC SELECT 1; SELECT 2; END; SELECT 3;";
    assert_eq!(texts(sql, Dialect::Postgres).len(), 2);
}

#[test]
fn mysql_backslash_and_hash_comment() {
    assert_eq!(
        texts(r"SELECT 'a\';b'; # c;d", Dialect::MySql),
        [r"SELECT 'a\';b'"]
    );
}

#[test]
fn mysql_double_dash_needs_a_space() {
    assert_eq!(
        texts("SELECT 5--1; SELECT 2;", Dialect::MySql),
        ["SELECT 5--1", "SELECT 2"]
    );
}

#[test]
fn mysql_delimiter_changes_the_separator() {
    let sql = "DELIMITER $$\nCREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END$$\nDELIMITER ;\nSELECT 3;";
    let found = texts(sql, Dialect::MySql);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].ends_with("END"));
    assert_eq!(found[1], "SELECT 3");
}

#[test]
fn mysql_word_delimiter_keeps_a_whole_word() {
    let sql = "DELIMITER go\nSELECT * FROM goods\ngo\nSELECT 2\ngo";
    assert_eq!(
        texts(sql, Dialect::MySql),
        ["SELECT * FROM goods", "SELECT 2"],
        "a delimiter must not cut an identifier that starts with it"
    );
}

#[test]
fn mysql_word_delimiter_matches_at_the_end_of_the_text() {
    assert_eq!(
        texts("DELIMITER go\nSELECT 1 go", Dialect::MySql),
        ["SELECT 1"]
    );
}

#[test]
fn a_non_ascii_letter_keeps_a_compound_word_whole() {
    // `ATOMIC\u{e9}` is one identifier, so `BEGIN` opens no body and the first
    // semicolon ends the statement.
    let sql = "CREATE FUNCTION f() BEGIN ATOMIC\u{e9} SELECT 1; END; SELECT 2;";
    assert_eq!(
        texts(sql, Dialect::Postgres),
        [
            "CREATE FUNCTION f() BEGIN ATOMIC\u{e9} SELECT 1",
            "END",
            "SELECT 2"
        ]
    );
}

#[test]
fn mysql_compound_body_without_delimiter() {
    let sql = "CREATE PROCEDURE p() BEGIN IF 1 THEN SELECT 1; END IF; WHILE 0 DO SELECT 2; END WHILE; END; SELECT 3;";
    let found = texts(sql, Dialect::MySql);
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[1], "SELECT 3");
}

#[test]
fn sqlite_trigger_body_is_one_statement() {
    let sql = "CREATE TRIGGER t AFTER INSERT ON a BEGIN UPDATE a SET x = CASE WHEN x THEN 1 ELSE 2 END; DELETE FROM b; END; SELECT 1;";
    let found = texts(sql, Dialect::Sqlite);
    assert_eq!(found.len(), 2, "{found:?}");
}

#[test]
fn transaction_begin_is_not_a_compound_block() {
    for dialect in [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite] {
        assert_eq!(
            texts("BEGIN; UPDATE a SET x = 1; COMMIT;", dialect),
            ["BEGIN", "UPDATE a SET x = 1", "COMMIT"],
            "{dialect:?}"
        );
    }
}

#[test]
fn sqlite_bracket_identifier_keeps_semicolon() {
    assert_eq!(
        texts("SELECT [a;b] FROM t; SELECT 2;", Dialect::Sqlite),
        ["SELECT [a;b] FROM t", "SELECT 2"]
    );
}

#[test]
fn postgres_array_brackets_do_not_quote() {
    assert_eq!(
        texts("SELECT a[1]; SELECT 2;", Dialect::Postgres),
        ["SELECT a[1]", "SELECT 2"]
    );
}

#[test]
fn incomplete_text_does_not_fail() {
    for sql in [
        "SELECT 'open",
        "SELECT /* open",
        "SELECT $$ open",
        "SELECT \"open",
        "",
    ] {
        for dialect in [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite] {
            let found = split(sql, dialect);
            assert!(found.iter().all(|s| !s.terminated), "{sql:?} {dialect:?}");
        }
    }
}

#[test]
fn comment_only_text_has_no_statement() {
    assert!(split("-- only\n/* comment */\n", Dialect::Sqlite).is_empty());
    assert!(split(" ; ;; ", Dialect::Sqlite).is_empty());
}

#[test]
fn multibyte_text_keeps_byte_offsets() {
    let sql = "SELECT 'ação;'; SELECT '日本語';";
    let found = split(sql, Dialect::Postgres);
    assert_eq!(found.len(), 2);
    assert_eq!(&sql[found[1].start..found[1].end], "SELECT '日本語'");
}

#[test]
fn cursor_rules_follow_the_plan() {
    let sql = "SELECT 1;\n\n-- comment\nSELECT 2;";
    let inside = sql.find("SELECT 2").unwrap() + 3;
    assert_eq!(
        statement_at(sql, Dialect::Sqlite, inside)
            .unwrap()
            .text(sql),
        "SELECT 2"
    );

    let after_semicolon = sql.find(';').unwrap() + 1;
    assert_eq!(
        statement_at(sql, Dialect::Sqlite, after_semicolon)
            .unwrap()
            .text(sql),
        "SELECT 1"
    );

    let blank_line = after_semicolon + 1;
    assert!(statement_at(sql, Dialect::Sqlite, blank_line).is_none());

    let in_comment = sql.find("comment").unwrap();
    assert!(statement_at(sql, Dialect::Sqlite, in_comment).is_none());
}

#[test]
fn tokens_show_the_kind_and_the_closing_mark() {
    let kinds = |sql: &str, dialect| {
        tokenize(sql, dialect)
            .into_iter()
            .map(|t| (t.kind, t.closed))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        kinds("\"a\" 'b", Dialect::Postgres),
        [
            (TokenKind::QuotedIdentifier, true),
            (TokenKind::String, false)
        ]
    );
    assert_eq!(
        kinds("\"a\" `b`", Dialect::MySql),
        [
            (TokenKind::String, true),
            (TokenKind::QuotedIdentifier, true)
        ]
    );
    assert_eq!(
        kinds("[a 12 -- x", Dialect::Sqlite),
        [(TokenKind::QuotedIdentifier, false)]
    );
    assert_eq!(
        kinds("a 12 -- x", Dialect::Sqlite),
        [
            (TokenKind::Word, true),
            (TokenKind::Number, true),
            (TokenKind::Comment, false)
        ]
    );
}

#[test]
fn every_token_starts_and_ends_on_a_character_boundary() {
    let sql = "SELECT 'ação', \"日本\", açúcar /* ç */ -- é\n, 'x\\é";
    for dialect in [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite] {
        for token in tokenize(sql, dialect) {
            assert!(
                sql.get(token.start()..token.end()).is_some(),
                "{token:?} {dialect:?}"
            );
        }
    }
}

#[test]
fn reused_tokens_split_like_a_new_read() {
    let samples = [
        "SELECT 1; SELECT 2",
        "BEGIN; UPDATE a SET x = 1; COMMIT;",
        "CREATE TRIGGER t AFTER INSERT ON a BEGIN UPDATE a SET x = CASE WHEN x THEN 1 ELSE 2 END; DELETE FROM b; END; SELECT 1;",
        "CREATE FUNCTION f() RETURNS int LANGUAGE sql BEGIN ATOMIC SELECT 1; SELECT 2; END; SELECT 3;",
        "CREATE PROCEDURE p() BEGIN IF 1 THEN SELECT 1; END IF; WHILE 0 DO SELECT 2; END WHILE; END; SELECT 3;",
        "DELIMITER $$\nCREATE PROCEDURE p() BEGIN SELECT 1; END$$\nDELIMITER ;\nSELECT 3;",
        "SELECT 'ação;' /* ; */ ; -- x;\nSELECT $$a;b$$;",
        "SELECT 'open",
        " ; ;; ",
        "",
    ];
    for sql in samples {
        for dialect in [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite] {
            let tokens = tokenize(sql, dialect);
            assert_eq!(
                split_tokens(sql, dialect, &tokens),
                split(sql, dialect),
                "{sql:?} {dialect:?}"
            );
        }
    }
}

#[test]
fn large_script_splits_quickly() {
    let sql = "INSERT INTO t VALUES (1, 'text;value');\n".repeat(50_000);
    let started = std::time::Instant::now();
    let found = split(&sql, Dialect::Postgres);
    let elapsed = started.elapsed();
    println!("split {} bytes in {elapsed:?}", sql.len());
    assert_eq!(found.len(), 50_000);
    assert!(elapsed.as_millis() < 500, "{elapsed:?}");
}

#[test]
fn plain_reads_can_end_with_a_cancellation() {
    for sql in [
        "SELECT * FROM t",
        "select a, count(*) from t group by a",
        "SELECT lower(a), coalesce(b, 'x') FROM t WHERE a IN (1, 2)",
        "SELECT generate_series(1, 100)",
        "WITH c AS (SELECT 1) SELECT * FROM c",
        "VALUES (1), (2)",
        "TABLE t",
        "SHOW TABLES",
        "DESCRIBE t",
        "EXPLAIN SELECT * FROM t",
        "SELECT * FROM a JOIN (SELECT 1) b ON (a.x = b.y)",
        "-- comment\nSELECT 1",
        "SELECT 'INSERT INTO t' FROM t",
        "SELECT \"update\" FROM t",
    ] {
        assert!(is_plain_read(sql, Dialect::Postgres), "{sql}");
    }
}

#[test]
fn a_statement_with_a_possible_effect_consumes_its_rows() {
    for sql in [
        "INSERT INTO t VALUES (1) RETURNING id",
        "UPDATE t SET a = 1 RETURNING a",
        "DELETE FROM t RETURNING a",
        "WITH d AS (DELETE FROM t RETURNING a) SELECT * FROM d",
        "SELECT * INTO copy_of_t FROM t",
        "SELECT * FROM t FOR UPDATE",
        "EXPLAIN ANALYZE DELETE FROM t",
        "EXPLAIN ANALYZE SELECT 1",
        "SELECT write_row(id) FROM t",
        "SELECT public.count(id) FROM t",
        "SELECT \"count\"(id) FROM t",
        "SELECT nextval('s')",
        "CALL p()",
        "(SELECT 1)",
        "",
        "-- only a comment",
    ] {
        assert!(!is_plain_read(sql, Dialect::Postgres), "{sql}");
    }
}

#[test]
fn a_plain_read_without_its_own_limit_receives_the_limit_on_a_new_line() {
    for dialect in [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite] {
        for sql in [
            "SELECT * FROM t",
            "select a, count(*) from t group by a order by a",
            "WITH c AS (SELECT 1 LIMIT 1) SELECT * FROM c",
            "SELECT 1 UNION ALL SELECT 2",
            "VALUES (1), (2)",
            "TABLE t",
            "SELECT * FROM (SELECT * FROM t LIMIT 5) s",
            "SELECT * FROM t WHERE a IN (SELECT b FROM u OFFSET 1)",
            "-- comment\nSELECT 1",
            "SELECT 'LIMIT 1' AS \"limit\" FROM t",
        ] {
            let expected = format!("{sql}\nLIMIT 1001");
            assert_eq!(
                with_row_limit(sql, dialect, 1001).as_deref(),
                Some(expected.as_str()),
                "{sql} ({dialect:?})"
            );
        }
    }
}

#[test]
fn a_line_comment_at_the_end_stays_before_the_added_limit() {
    let sql = "SELECT * FROM t -- note  ";
    assert_eq!(
        with_row_limit(sql, Dialect::Postgres, 3).as_deref(),
        Some("SELECT * FROM t -- note\nLIMIT 3")
    );
}

#[test]
fn a_statement_with_its_own_limit_or_a_possible_effect_keeps_its_text() {
    for sql in [
        "SELECT * FROM t LIMIT 100000",
        "select * from t limit 5 offset 10",
        "SELECT * FROM t OFFSET 10",
        "SELECT * FROM t FETCH FIRST 5 ROWS ONLY",
        "SELECT * FROM t FOR SHARE",
        "SELECT * FROM t FOR UPDATE",
        "SELECT * FROM t LOCK IN SHARE MODE",
        "SELECT * INTO copy_of_t FROM t",
        "SHOW TABLES",
        "DESCRIBE t",
        "EXPLAIN SELECT * FROM t",
        "SELECT write_row(id) FROM t",
        "SELECT nextval('s')",
        "INSERT INTO t SELECT * FROM u",
        "(SELECT 1)",
        "",
        "-- only a comment",
    ] {
        assert_eq!(with_row_limit(sql, Dialect::Postgres, 1001), None, "{sql}");
    }
}

#[test]
fn a_redis_command_never_receives_a_limit() {
    assert_eq!(with_row_limit("SCAN 0", Dialect::Redis, 1001), None);
}

#[test]
fn row_counts_belong_to_changes() {
    for sql in [
        "INSERT INTO t VALUES (1)",
        "update t set a = 1",
        "/* c */ DELETE FROM t",
        "WITH c AS (SELECT 1) UPDATE t SET a = 1",
    ] {
        assert!(counts_rows(sql, Dialect::Postgres), "{sql}");
    }
    for sql in [
        "CREATE TABLE t (a int)",
        "DROP TABLE t",
        "BEGIN",
        "CALL p()",
        "",
    ] {
        assert!(!counts_rows(sql, Dialect::Postgres), "{sql}");
    }
}

#[test]
fn copy_with_a_client_stream_is_found() {
    for sql in [
        "COPY t FROM STDIN",
        "copy t (a) from stdin with csv",
        "COPY t TO STDOUT",
        "COPY (SELECT 1) TO stdout",
    ] {
        assert!(reads_or_writes_stdio(sql, Dialect::Postgres), "{sql}");
    }
    for sql in [
        "COPY t FROM '/tmp/stdin'",
        "COPY t TO '/tmp/file'",
        "SELECT stdin FROM t",
        "",
    ] {
        assert!(!reads_or_writes_stdio(sql, Dialect::Postgres), "{sql}");
    }
}

#[test]
fn the_removal_of_prepared_statements_is_found() {
    for sql in [
        "DEALLOCATE ALL",
        "deallocate prepare all",
        "DEALLOCATE s7",
        "/* x */ DISCARD ALL",
        "discard all",
    ] {
        assert!(removes_prepared_statements(sql, Dialect::Postgres), "{sql}");
    }
    for sql in [
        "DISCARD PLANS",
        "DISCARD TEMP",
        "SELECT 'DEALLOCATE ALL'",
        "-- DISCARD ALL\nSELECT 1",
        "",
    ] {
        assert!(
            !removes_prepared_statements(sql, Dialect::Postgres),
            "{sql}"
        );
    }
}
