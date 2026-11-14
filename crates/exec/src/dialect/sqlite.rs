//! The completion words of SQLite.

use super::{Features, Vocabulary};
use crate::complete::keywords::{COMMON, STARTERS};
use crate::complete::vocabulary::COMMON_FUNCTIONS;

pub struct Sqlite;

const KEYWORDS: &[&str] = &[
    "ABORT",
    "ATTACH",
    "AUTOINCREMENT",
    "CONFLICT",
    "DATABASE",
    "DEFERRED",
    "DETACH",
    "EXCLUSIVE",
    "GLOB",
    "IMMEDIATE",
    "INDEXED",
    "MATCH",
    "PRAGMA",
    "RAISE",
    "REGEXP",
    "REINDEX",
    "RENAME",
    "REPLACE",
    "RETURNING",
    "ROWID",
    "TEMP",
    "TEMPORARY",
    "VACUUM",
    "VIRTUAL",
    "WITHOUT",
];

const STARTERS_EXTRA: &[&str] = &["ATTACH", "DETACH", "PRAGMA", "REPLACE", "VACUUM"];

const TYPES: &[&str] = &["INTEGER", "REAL", "TEXT", "BLOB", "NUMERIC"];

const FUNCTIONS: &[(&str, &str)] = &[
    ("SUBSTR", "SUBSTR(text, start, length)"),
    ("GROUP_CONCAT", "GROUP_CONCAT(expression, separator)"),
    ("IFNULL", "IFNULL(value, fallback)"),
    ("DATE", "DATE(value, modifier, ...)"),
    ("TIME", "TIME(value, modifier, ...)"),
    ("DATETIME", "DATETIME(value, modifier, ...)"),
    ("STRFTIME", "STRFTIME(format, value, modifier, ...)"),
    ("JULIANDAY", "JULIANDAY(value, modifier, ...)"),
    ("JSON_EXTRACT", "JSON_EXTRACT(document, path, ...)"),
];

const VOCABULARY: Vocabulary = Vocabulary {
    keywords: &[COMMON, KEYWORDS],
    starters: &[STARTERS, STARTERS_EXTRA],
    types: TYPES,
    cast_types: None,
    functions: &[COMMON_FUNCTIONS, FUNCTIONS],
    ilike: false,
    alter_table_actions: &["ADD COLUMN", "DROP COLUMN", "RENAME COLUMN", "RENAME TO"],
    modify_column: false,
    alter_column_type: false,
    column_options: &[],
    integer_options: &[],
    integer_key_autoincrement: true,
    nulls_order: true,
    join_extra: "FULL JOIN",
};

impl Features for Sqlite {
    fn vocabulary(&self) -> &'static Vocabulary {
        &VOCABULARY
    }
}
