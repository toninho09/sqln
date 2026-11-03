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

/// The main verbs of a statement. `WITH` comes before one of them.
const VERBS: &[&str] = &[
    "SELECT", "INSERT", "UPDATE", "DELETE", "MERGE", "REPLACE", "VALUES", "TABLE",
];

/// These statements control the session, and the question of a run skips them.
const SESSION_WORDS: &[&str] = &[
    "BEGIN",
    "START",
    "COMMIT",
    "END",
    "ROLLBACK",
    "SAVEPOINT",
    "RELEASE",
    "USE",
];

/// These Redis commands only read, and the question of a run skips them. A name out of
/// this list asks: the question must answer without the network, and a command of a
/// module then costs one key and no data.
const REDIS_READS: &[&str] = &[
    "BITCOUNT",
    "BITFIELD_RO",
    "BITPOS",
    "COMMAND",
    "DBSIZE",
    "DUMP",
    "ECHO",
    "EXISTS",
    "EXPIRETIME",
    "GEODIST",
    "GEOHASH",
    "GEOPOS",
    "GEORADIUSBYMEMBER_RO",
    "GEORADIUS_RO",
    "GEOSEARCH",
    "GET",
    "GETBIT",
    "GETRANGE",
    "HEXISTS",
    "HGET",
    "HGETALL",
    "HKEYS",
    "HLEN",
    "HMGET",
    "HRANDFIELD",
    "HSCAN",
    "HSTRLEN",
    "HVALS",
    "INFO",
    "KEYS",
    "LASTSAVE",
    "LCS",
    "LINDEX",
    "LLEN",
    "LPOS",
    "LRANGE",
    "MGET",
    "PEXPIRETIME",
    "PFCOUNT",
    "PING",
    "PTTL",
    "RANDOMKEY",
    "SCAN",
    "SCARD",
    "SDIFF",
    "SINTER",
    "SINTERCARD",
    "SISMEMBER",
    "SMEMBERS",
    "SMISMEMBER",
    "SORT_RO",
    "SRANDMEMBER",
    "SSCAN",
    "STRLEN",
    "SUBSTR",
    "SUNION",
    "TIME",
    "TTL",
    "TYPE",
    "XINFO",
    "XLEN",
    "XPENDING",
    "XRANGE",
    "XREAD",
    "XREVRANGE",
    "ZCARD",
    "ZCOUNT",
    "ZDIFF",
    "ZINTER",
    "ZINTERCARD",
    "ZLEXCOUNT",
    "ZMSCORE",
    "ZRANDMEMBER",
    "ZRANGE",
    "ZRANGEBYLEX",
    "ZRANGEBYSCORE",
    "ZRANK",
    "ZREVRANGE",
    "ZREVRANGEBYLEX",
    "ZREVRANGEBYSCORE",
    "ZREVRANK",
    "ZSCAN",
    "ZSCORE",
    "ZUNION",
];

/// These Redis commands control the session, as `BEGIN` and `USE` do in SQL. Each command
/// of a `MULTI` block has its own line, so it counts by itself.
const REDIS_SESSION: &[&str] = &["SELECT", "MULTI", "EXEC", "DISCARD", "WATCH", "UNWATCH"];

/// A statement with one of these words stays out of the query history.
const SECRET_WORDS: &[&str] = &[
    "PASSWORD",
    "PASSWD",
    "IDENTIFIED",
    "REQUIREPASS",
    "MASTERAUTH",
];

/// A Redis command with one of these names can carry a password.
const SECRET_COMMANDS: &[&str] = &["AUTH", "ACL", "HELLO"];

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

pub(super) fn is_any(word: &str, list: &[&str]) -> bool {
    list.iter().any(|item| word.eq_ignore_ascii_case(item))
}

/// Lists the words of the main level: outside parentheses and without comments.
fn top_level_words(sql: &str, dialect: Dialect) -> Vec<&str> {
    let mut depth = 0_usize;
    let mut words = Vec::new();
    for token in tokenize(sql, dialect) {
        let text = &sql[token.start()..token.end()];
        match token.kind {
            TokenKind::Punctuation if text == "(" => depth += 1,
            TokenKind::Punctuation if text == ")" => depth = depth.saturating_sub(1),
            TokenKind::Word if depth == 0 => words.push(text),
            _ => {}
        }
    }
    words
}

/// Returns the verb of a statement. After `WITH`, it is the first verb of the main level.
fn main_verb<'a>(words: &[&'a str]) -> Option<&'a str> {
    let first = *words.first()?;
    if first.eq_ignore_ascii_case("WITH") {
        words.iter().copied().find(|word| is_any(word, VERBS))
    } else {
        Some(first)
    }
}

/// Returns true for `UPDATE` or `DELETE` without `WHERE` in the main level.
/// A `WHERE` in a subquery does not limit the rows of the statement.
pub fn lacks_where(sql: &str, dialect: Dialect) -> bool {
    if dialect == Dialect::Redis {
        return false;
    }
    let words = top_level_words(sql, dialect);
    main_verb(&words).is_some_and(|verb| is_any(verb, &["UPDATE", "DELETE"]))
        && !words.iter().any(|word| word.eq_ignore_ascii_case("WHERE"))
}

/// Returns the first word of a statement, after its comments. A statement that starts
/// with another token, such as a name in quotes, has no first word.
pub fn first_keyword(sql: &str, dialect: Dialect) -> Option<&str> {
    let tokens = tokenize(sql, dialect);
    let first = tokens
        .iter()
        .find(|token| token.kind != TokenKind::Comment)?;
    (first.kind == TokenKind::Word).then(|| &sql[first.start()..first.end()])
}

/// Returns true for a Redis command out of the known reads. A name in quotes also asks,
/// because the server reads that name as the command.
fn redis_changes_state(sql: &str) -> bool {
    !first_keyword(sql, Dialect::Redis)
        .is_some_and(|word| is_any(word, REDIS_READS) || is_any(word, REDIS_SESSION))
}

/// Returns true for a statement that a production connection confirms: it is not a
/// plain read and it does not control the session.
pub fn changes_state(sql: &str, dialect: Dialect) -> bool {
    if dialect == Dialect::Redis {
        return redis_changes_state(sql);
    }
    let words = top_level_words(sql, dialect);
    !is_plain_read(sql, dialect)
        && !words
            .first()
            .is_some_and(|word| is_any(word, SESSION_WORDS))
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
            .map_or("", |token| &sql[token.start()..token.end()])
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

/// The statements that accept a `LIMIT` at their end.
const LIMIT_STARTS: &[&str] = &["SELECT", "WITH", "VALUES", "TABLE"];

/// A main-level word that already limits the rows, or that refuses a `LIMIT` after it.
const LIMIT_BLOCKERS: &[&str] = &["LIMIT", "OFFSET", "FETCH", "FOR", "LOCK"];

/// Returns the statement with `LIMIT rows` at its end, so the server plans for few rows.
/// Only a plain read without its own limit changes: a `LIMIT` would change the effect of
/// an unknown function, as a cancellation does.
pub fn with_row_limit(sql: &str, dialect: Dialect, rows: usize) -> Option<String> {
    if dialect == Dialect::Redis || !is_plain_read(sql, dialect) {
        return None;
    }
    let words = top_level_words(sql, dialect);
    let accepts = words.first().is_some_and(|word| is_any(word, LIMIT_STARTS))
        && !words.iter().any(|word| is_any(word, LIMIT_BLOCKERS));
    // The new line keeps the clause out of a line comment at the end.
    accepts.then(|| format!("{}\nLIMIT {rows}", sql.trim_end()))
}

/// Returns true for a statement that can hold a secret. The check reads words, so a
/// secret inside a string does not count.
pub fn is_secret(sql: &str, dialect: Dialect) -> bool {
    let tokens = tokenize(sql, dialect);
    let mut words = tokens
        .iter()
        .filter(|token| token.kind == TokenKind::Word)
        .map(|token| &sql[token.start()..token.end()]);
    let command = words.next().unwrap_or("");
    (dialect == Dialect::Redis && is_any(command, SECRET_COMMANDS))
        || is_any(command, SECRET_WORDS)
        || words.any(|word| is_any(word, SECRET_WORDS))
}

/// Returns true for a statement that reports the number of changed rows.
pub fn counts_rows(sql: &str, dialect: Dialect) -> bool {
    let tokens = tokenize(sql, dialect);
    let first = tokens.iter().find(|token| token.kind != TokenKind::Comment);
    first.is_some_and(|token| {
        let word = &sql[token.start()..token.end()];
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
        .map(|token| &sql[token.start()..token.end()]);
    words
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("COPY"))
        && words.any(|word| is_any(word, &["STDIN", "STDOUT"]))
}
