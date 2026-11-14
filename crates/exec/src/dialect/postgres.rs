//! The completion words of PostgreSQL. DuckDB shares most of them.

use super::{Features, Vocabulary};
use crate::complete::keywords::{COMMON, STARTERS};
use crate::complete::vocabulary::COMMON_FUNCTIONS;

pub struct Postgres;

pub(super) const KEYWORDS: &[&str] = &[
    "ARRAY",
    "CONCURRENTLY",
    "COPY",
    "CTID",
    "DISCARD",
    "EXTENSION",
    "FETCH",
    "FILTER",
    "GRANT",
    "ILIKE",
    "LATERAL",
    "MATERIALIZED",
    "NOTIFY",
    "NULLS",
    "ONLY",
    "OVER",
    "PARTITION",
    "REINDEX",
    "RETURNING",
    "REVOKE",
    "SCHEMA",
    "SEQUENCE",
    "SHOW",
    "SIMILAR",
    "TRUNCATE",
    "VACUUM",
    "WINDOW",
];

pub(super) const STARTERS_EXTRA: &[&str] =
    &["COPY", "GRANT", "REVOKE", "SHOW", "TRUNCATE", "VACUUM"];

pub(super) const TYPES: &[&str] = &[
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
];

pub(super) const FUNCTIONS: &[(&str, &str)] = &[
    ("SUBSTRING", "SUBSTRING(text, start, count)"),
    ("CONCAT", "CONCAT(value, ...)"),
    ("STRING_AGG", "STRING_AGG(value, delimiter)"),
    ("NOW", "NOW()"),
    ("DATE_TRUNC", "DATE_TRUNC(unit, timestamp)"),
    ("TO_CHAR", "TO_CHAR(value, format)"),
    ("AGE", "AGE(timestamp, timestamp)"),
    ("JSONB_BUILD_OBJECT", "JSONB_BUILD_OBJECT(key, value, ...)"),
];

pub(super) const ALTER_TABLE_ACTIONS: &[&str] = &[
    "ADD COLUMN",
    "DROP COLUMN",
    "ALTER COLUMN",
    "RENAME COLUMN",
    "RENAME TO",
    "ADD CONSTRAINT",
    "DROP CONSTRAINT",
];

const VOCABULARY: Vocabulary = Vocabulary {
    keywords: &[COMMON, KEYWORDS],
    starters: &[STARTERS, STARTERS_EXTRA],
    types: TYPES,
    cast_types: None,
    functions: &[COMMON_FUNCTIONS, FUNCTIONS],
    ilike: true,
    alter_table_actions: ALTER_TABLE_ACTIONS,
    modify_column: false,
    alter_column_type: true,
    column_options: &[],
    integer_options: &["GENERATED ALWAYS AS IDENTITY"],
    integer_key_autoincrement: false,
    nulls_order: true,
    join_extra: "FULL JOIN",
};

impl Features for Postgres {
    fn vocabulary(&self) -> &'static Vocabulary {
        &VOCABULARY
    }
}
