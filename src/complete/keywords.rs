use crate::sql::Dialect;

const COMMON: &[&str] = &[
    "ADD", "ALL", "ALTER", "ANALYZE", "AND", "AS", "ASC", "BEGIN", "BETWEEN", "BY", "CASCADE",
    "CASE", "CAST", "CHECK", "COLUMN", "COMMIT", "CONSTRAINT", "CREATE", "CROSS", "CURRENT_DATE",
    "CURRENT_TIMESTAMP", "DEFAULT", "DELETE", "DESC", "DISTINCT", "DROP", "ELSE", "END", "ESCAPE",
    "EXCEPT", "EXISTS", "EXPLAIN", "FALSE", "FOREIGN", "FROM", "FULL", "GROUP", "HAVING", "IF",
    "IN", "INDEX", "INNER", "INSERT", "INTERSECT", "INTO", "IS", "JOIN", "KEY", "LEFT", "LIKE",
    "LIMIT", "NOT", "NULL", "OFFSET", "ON", "OR", "ORDER", "OUTER", "PRIMARY", "REFERENCES",
    "RIGHT", "ROLLBACK", "SAVEPOINT", "SELECT", "SET", "TABLE", "THEN", "TRANSACTION", "TRIGGER",
    "TRUE", "UNION", "UNIQUE", "UPDATE", "USING", "VALUES", "VIEW", "WHEN", "WHERE", "WITH",
];

const POSTGRES: &[&str] = &[
    "ARRAY", "CONCURRENTLY", "COPY", "CTID", "DISCARD", "EXTENSION", "FETCH", "FILTER", "GRANT",
    "ILIKE", "LATERAL", "MATERIALIZED", "NOTIFY", "NULLS", "ONLY", "OVER", "PARTITION", "REINDEX",
    "RETURNING", "REVOKE", "SCHEMA", "SEQUENCE", "SHOW", "SIMILAR", "TRUNCATE", "VACUUM", "WINDOW",
];

const MYSQL: &[&str] = &[
    "AUTO_INCREMENT", "CALL", "CHANGE", "CHARSET", "COLLATE", "DATABASE", "DATABASES", "DELIMITER",
    "DESCRIBE", "DUPLICATE", "ENGINE", "EVENT", "FORCE", "GRANT", "IGNORE", "KILL", "LOCK",
    "MODIFY", "PROCEDURE", "REGEXP", "RENAME", "REPLACE", "RETURNS", "REVOKE", "RLIKE", "SHOW",
    "STATUS", "STRAIGHT_JOIN", "TABLES", "TRUNCATE", "UNLOCK", "UNSIGNED", "USE", "ZEROFILL",
];

const SQLITE: &[&str] = &[
    "ABORT", "ATTACH", "AUTOINCREMENT", "CONFLICT", "DATABASE", "DEFERRED", "DETACH", "EXCLUSIVE",
    "GLOB", "IMMEDIATE", "INDEXED", "MATCH", "PRAGMA", "RAISE", "REGEXP", "REINDEX", "RENAME",
    "REPLACE", "RETURNING", "ROWID", "TEMP", "TEMPORARY", "VACUUM", "VIRTUAL", "WITHOUT",
];

const STARTERS: &[&str] = &[
    "ALTER", "ANALYZE", "BEGIN", "COMMIT", "CREATE", "DELETE", "DROP", "EXPLAIN", "INSERT",
    "ROLLBACK", "SAVEPOINT", "SELECT", "SET", "UPDATE", "WITH",
];

const POSTGRES_STARTERS: &[&str] = &["COPY", "GRANT", "REVOKE", "SHOW", "TRUNCATE", "VACUUM"];
const MYSQL_STARTERS: &[&str] = &[
    "CALL", "DELIMITER", "DESCRIBE", "GRANT", "REPLACE", "REVOKE", "SHOW", "TRUNCATE", "USE",
];
const SQLITE_STARTERS: &[&str] = &["ATTACH", "DETACH", "PRAGMA", "REPLACE", "VACUUM"];

/// These words need quotes as identifiers. They do not act as table aliases.
const RESERVED: &[&str] = &[
    "ALL", "ALTER", "AND", "AS", "ASC", "BEGIN", "BETWEEN", "BY", "CASE", "CAST", "CHECK", "COLUMN",
    "COMMIT", "CONSTRAINT", "CREATE", "CROSS", "DEFAULT", "DELETE", "DESC", "DISTINCT", "DROP",
    "ELSE", "EXCEPT", "EXISTS", "FALSE", "FETCH", "FOR", "FOREIGN", "FROM", "FULL", "GRANT",
    "GROUP", "HAVING", "IF", "IN", "INDEX", "INNER", "INSERT", "INTERSECT", "INTO", "IS", "JOIN",
    "KEY", "LATERAL", "LEFT", "LIKE", "LIMIT", "NATURAL", "NOT", "NULL", "OFFSET", "ON", "ONLY",
    "OR", "ORDER", "OUTER", "PRIMARY", "REFERENCES", "RETURNING", "RIGHT", "ROLLBACK", "SELECT",
    "SET", "TABLE", "THEN", "TO", "TRIGGER", "TRUE", "UNION", "UNIQUE", "UPDATE", "USE", "USING",
    "VALUES", "VIEW", "WHEN", "WHERE", "WINDOW", "WITH",
];

fn specific(dialect: Dialect) -> &'static [&'static str] {
    match dialect {
        Dialect::Postgres => POSTGRES,
        Dialect::MySql => MYSQL,
        Dialect::Sqlite => SQLITE,
    }
}

pub fn all(dialect: Dialect) -> impl Iterator<Item = &'static str> {
    COMMON.iter().chain(specific(dialect)).copied()
}

pub fn starters(dialect: Dialect) -> impl Iterator<Item = &'static str> {
    let specific = match dialect {
        Dialect::Postgres => POSTGRES_STARTERS,
        Dialect::MySql => MYSQL_STARTERS,
        Dialect::Sqlite => SQLITE_STARTERS,
    };
    STARTERS.iter().chain(specific).copied()
}

pub fn is_keyword(word: &str, dialect: Dialect) -> bool {
    all(dialect).any(|keyword| keyword.eq_ignore_ascii_case(word))
}

pub fn is_reserved(word: &str) -> bool {
    RESERVED.iter().any(|keyword| keyword.eq_ignore_ascii_case(word))
}
