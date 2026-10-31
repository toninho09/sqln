//! Statement classes that change how a connector reads a result.

use super::Dialect;
use super::lexer::{TokenKind, tokenize};

const READ_STARTS: &[&str] = &[
    "SELECT", "WITH", "VALUES", "TABLE", "SHOW", "DESC", "DESCRIBE", "EXPLAIN",
];

/// A statement with one of these words can change data.
const WRITE_WORDS: &[&str] = &[
    "INTO", "INSERT", "UPDATE", "DELETE", "MERGE", "ANALYZE", "ANALYSE",
];

/// These words can stand before a parenthesis, and they are not function calls.
const SYNTAX_WORDS: &[&str] = &[
    "ALL",
    "AND",
    "ANY",
    "AS",
    "BETWEEN",
    "BY",
    "CASE",
    "CAST",
    "DISTINCT",
    "ELSE",
    "EXCEPT",
    "EXISTS",
    "FILTER",
    "FROM",
    "HAVING",
    "ILIKE",
    "IN",
    "INTERSECT",
    "INTERVAL",
    "JOIN",
    "LATERAL",
    "LIKE",
    "NOT",
    "ON",
    "OR",
    "OVER",
    "SELECT",
    "SOME",
    "THEN",
    "UNION",
    "USING",
    "VALUES",
    "WHEN",
    "WHERE",
    "WITHIN",
];

/// Common functions of the two network databases that change no data.
const PURE_FUNCTIONS: &[&str] = &[
    "ABS",
    "AGE",
    "ARRAY_AGG",
    "ARRAY_LENGTH",
    "AVG",
    "BTRIM",
    "CEIL",
    "CEILING",
    "CHAR_LENGTH",
    "CHARACTER_LENGTH",
    "COALESCE",
    "CONCAT",
    "CONCAT_WS",
    "COUNT",
    "CUME_DIST",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "DATE",
    "DATE_FORMAT",
    "DATE_PART",
    "DATE_TRUNC",
    "DAY",
    "DENSE_RANK",
    "EXP",
    "EXTRACT",
    "FIRST_VALUE",
    "FLOOR",
    "FORMAT",
    "GENERATE_SERIES",
    "GREATEST",
    "GROUP_CONCAT",
    "HOUR",
    "IF",
    "IFNULL",
    "INITCAP",
    "JSON_AGG",
    "JSON_ARRAYAGG",
    "JSON_EXTRACT",
    "JSON_OBJECTAGG",
    "JSONB_AGG",
    "LAG",
    "LAST_VALUE",
    "LEAD",
    "LEAST",
    "LEFT",
    "LENGTH",
    "LN",
    "LOCALTIME",
    "LOCALTIMESTAMP",
    "LOG",
    "LOWER",
    "LPAD",
    "LTRIM",
    "MAX",
    "MD5",
    "MIN",
    "MINUTE",
    "MOD",
    "MONTH",
    "NOW",
    "NTH_VALUE",
    "NTILE",
    "NULLIF",
    "OCTET_LENGTH",
    "PERCENT_RANK",
    "POSITION",
    "POWER",
    "RANK",
    "REGEXP_REPLACE",
    "REPEAT",
    "REPLACE",
    "REVERSE",
    "RIGHT",
    "ROUND",
    "ROW_NUMBER",
    "RPAD",
    "RTRIM",
    "SECOND",
    "SIGN",
    "SPLIT_PART",
    "SQRT",
    "STDDEV",
    "STRING_AGG",
    "STRPOS",
    "SUBSTR",
    "SUBSTRING",
    "SUM",
    "TIME",
    "TIMESTAMP",
    "TO_CHAR",
    "TO_DATE",
    "TO_NUMBER",
    "TO_TIMESTAMP",
    "TRIM",
    "TRUNC",
    "TRUNCATE",
    "UNNEST",
    "UPPER",
    "VARIANCE",
    "YEAR",
];

fn is_any(word: &str, list: &[&str]) -> bool {
    list.iter().any(|item| word.eq_ignore_ascii_case(item))
}

/// Returns true when the end of the statement at the row cap can use a cancellation.
/// A cancellation undoes the writes of a statement, so an unknown function call
/// returns false: the reader then consumes the rows, and this only costs time.
pub fn is_plain_read(sql: &str, dialect: Dialect) -> bool {
    let tokens = tokenize(sql, dialect);
    let code: Vec<_> = tokens
        .iter()
        .filter(|token| token.kind != TokenKind::Comment)
        .collect();
    let word = |index: usize| {
        code.get(index)
            .map_or("", |token| &sql[token.start..token.end])
    };
    if code
        .first()
        .is_none_or(|token| token.kind != TokenKind::Word)
        || !is_any(word(0), READ_STARTS)
    {
        return false;
    }
    for (index, token) in code.iter().enumerate() {
        let text = word(index);
        if token.kind == TokenKind::Word && is_any(text, WRITE_WORDS) {
            return false;
        }
        if word(index + 1) != "(" {
            continue;
        }
        let qualified = index > 0 && word(index - 1) == ".";
        let known = is_any(text, SYNTAX_WORDS) || (is_any(text, PURE_FUNCTIONS) && !qualified);
        let name = matches!(token.kind, TokenKind::Word | TokenKind::QuotedIdentifier);
        if name && (token.kind == TokenKind::QuotedIdentifier || !known) {
            return false;
        }
    }
    true
}

/// Returns true for a statement that reports the number of changed rows.
pub fn counts_rows(sql: &str, dialect: Dialect) -> bool {
    let tokens = tokenize(sql, dialect);
    let first = tokens.iter().find(|token| token.kind != TokenKind::Comment);
    first.is_some_and(|token| {
        let word = &sql[token.start..token.end];
        is_any(
            word,
            &[
                "INSERT", "UPDATE", "DELETE", "MERGE", "REPLACE", "COPY", "WITH", "LOAD",
            ],
        )
    })
}

/// Returns true for `COPY` that reads `STDIN` or writes `STDOUT`.
/// The protocol of these statements has no place in a grid.
pub fn reads_or_writes_stdio(sql: &str, dialect: Dialect) -> bool {
    let tokens = tokenize(sql, dialect);
    let mut words = tokens
        .iter()
        .filter(|token| token.kind == TokenKind::Word)
        .map(|token| &sql[token.start..token.end]);
    words
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("COPY"))
        && words.any(|word| is_any(word, &["STDIN", "STDOUT"]))
}
