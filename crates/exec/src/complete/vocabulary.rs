//! Built-in types and function signatures. These lists do not use the connection.

use crate::sql::Dialect;

pub fn types(dialect: Dialect) -> &'static [&'static str] {
    match dialect {
        Dialect::Postgres | Dialect::DuckDb => &[
            "SMALLINT",
            "INTEGER",
            "BIGINT",
            "SMALLSERIAL",
            "SERIAL",
            "BIGSERIAL",
            "NUMERIC",
            "DECIMAL",
            "REAL",
            "DOUBLE PRECISION",
            "BOOLEAN",
            "CHAR",
            "VARCHAR",
            "TEXT",
            "BYTEA",
            "DATE",
            "TIME",
            "TIMESTAMP",
            "TIMESTAMPTZ",
            "INTERVAL",
            "UUID",
            "JSON",
            "JSONB",
            "INET",
            "CIDR",
            "MONEY",
        ],
        Dialect::MySql => &[
            "TINYINT",
            "SMALLINT",
            "MEDIUMINT",
            "INT",
            "INTEGER",
            "BIGINT",
            "DECIMAL",
            "NUMERIC",
            "FLOAT",
            "DOUBLE",
            "BIT",
            "BOOLEAN",
            "CHAR",
            "VARCHAR",
            "BINARY",
            "VARBINARY",
            "TINYTEXT",
            "TEXT",
            "MEDIUMTEXT",
            "LONGTEXT",
            "TINYBLOB",
            "BLOB",
            "MEDIUMBLOB",
            "LONGBLOB",
            "DATE",
            "TIME",
            "DATETIME",
            "TIMESTAMP",
            "YEAR",
            "JSON",
            "ENUM",
            "SET",
        ],
        Dialect::Sqlite => &["INTEGER", "REAL", "TEXT", "BLOB", "NUMERIC"],
        Dialect::Redis | Dialect::Kafka => &[],
    }
}

pub fn available_types(dialect: Dialect, cast: bool) -> impl Iterator<Item = &'static str> {
    let types = if cast && dialect == Dialect::MySql {
        &[
            "BINARY", "CHAR", "DATE", "DATETIME", "DECIMAL", "DOUBLE", "FLOAT", "JSON", "NCHAR",
            "REAL", "SIGNED", "TIME", "UNSIGNED", "YEAR",
        ][..]
    } else {
        types(dialect)
    };
    types
        .iter()
        .copied()
        .filter(move |word| !cast || !word.ends_with("SERIAL"))
}

const COMMON_FUNCTIONS: &[(&str, &str)] = &[
    ("COUNT", "COUNT(expression | *)"),
    ("SUM", "SUM(expression)"),
    ("AVG", "AVG(expression)"),
    ("MIN", "MIN(expression)"),
    ("MAX", "MAX(expression)"),
    ("LOWER", "LOWER(text)"),
    ("UPPER", "UPPER(text)"),
    ("TRIM", "TRIM(text)"),
    ("REPLACE", "REPLACE(text, from_text, to_text)"),
    ("COALESCE", "COALESCE(value, ...)"),
    ("NULLIF", "NULLIF(value, other)"),
    ("ABS", "ABS(number)"),
    ("ROUND", "ROUND(number, digits)"),
    ("LENGTH", "LENGTH(value)"),
];

pub fn functions(dialect: Dialect) -> impl Iterator<Item = (&'static str, &'static str)> {
    let specific: &[(&str, &str)] = match dialect {
        Dialect::Postgres | Dialect::DuckDb => &[
            ("SUBSTRING", "SUBSTRING(text, start, count)"),
            ("CONCAT", "CONCAT(value, ...)"),
            ("STRING_AGG", "STRING_AGG(value, delimiter)"),
            ("NOW", "NOW()"),
            ("DATE_TRUNC", "DATE_TRUNC(unit, timestamp)"),
            ("TO_CHAR", "TO_CHAR(value, format)"),
            ("AGE", "AGE(timestamp, timestamp)"),
            ("JSONB_BUILD_OBJECT", "JSONB_BUILD_OBJECT(key, value, ...)"),
        ],
        Dialect::MySql => &[
            ("SUBSTRING", "SUBSTRING(text, start, length)"),
            ("CONCAT", "CONCAT(value, ...)"),
            ("GROUP_CONCAT", "GROUP_CONCAT(expression)"),
            ("NOW", "NOW()"),
            ("DATE_FORMAT", "DATE_FORMAT(date, format)"),
            ("DATEDIFF", "DATEDIFF(date, other_date)"),
            ("IFNULL", "IFNULL(value, fallback)"),
            ("JSON_EXTRACT", "JSON_EXTRACT(document, path, ...)"),
        ],
        Dialect::Sqlite => &[
            ("SUBSTR", "SUBSTR(text, start, length)"),
            ("GROUP_CONCAT", "GROUP_CONCAT(expression, separator)"),
            ("IFNULL", "IFNULL(value, fallback)"),
            ("DATE", "DATE(value, modifier, ...)"),
            ("TIME", "TIME(value, modifier, ...)"),
            ("DATETIME", "DATETIME(value, modifier, ...)"),
            ("STRFTIME", "STRFTIME(format, value, modifier, ...)"),
            ("JULIANDAY", "JULIANDAY(value, modifier, ...)"),
            ("JSON_EXTRACT", "JSON_EXTRACT(document, path, ...)"),
        ],
        Dialect::Redis | Dialect::Kafka => &[],
    };
    let common = if dialect.is_command_line() {
        &[][..]
    } else {
        COMMON_FUNCTIONS
    };
    common.iter().chain(specific).copied()
}

pub const EXPRESSION_WORDS: &[&str] = &[
    "CASE",
    "CAST",
    "DISTINCT",
    "EXISTS",
    "FALSE",
    "NOT",
    "NULL",
    "TRUE",
    "CURRENT_DATE",
    "CURRENT_TIMESTAMP",
];
