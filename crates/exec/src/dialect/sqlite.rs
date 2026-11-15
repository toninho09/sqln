//! The completion words of SQLite.

use super::{Engine, Family, Features, Vocabulary};
use crate::catalog::Catalog;
use crate::complete::Completion;
use crate::complete::keywords::{COMMON, STARTERS};
use crate::complete::vocabulary::COMMON_FUNCTIONS;
use crate::diagram::sql::{sqlite_columns, sqlite_keys};
use crate::plan::{PlanFormat, PlanSyntax};
use crate::sql::Statement;

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

/// SQLite cannot run the statement in a plan.
const PLAN: PlanSyntax = PlanSyntax {
    explain: "EXPLAIN QUERY PLAN ",
    analyze: None,
    format: PlanFormat::SqliteRows,
};

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

    fn command_argument(&self, _: &str, _: Option<&Catalog>) -> Option<Vec<String>> {
        None
    }

    fn command_line(&self, _: &str, _: usize, _: Option<&Catalog>) -> Option<Completion> {
        None
    }

    fn select_statement(
        &self,
        _: &str,
        _: &Statement,
        _: usize,
        _: Option<&Catalog>,
    ) -> Option<Completion> {
        None
    }

    fn plan(&self) -> Option<&'static PlanSyntax> {
        Some(&PLAN)
    }

    fn diagram_sql(&self, schema: &str) -> Option<[String; 2]> {
        Some([sqlite_columns(schema), sqlite_keys(schema)])
    }

    fn ai_language(&self) -> &'static str {
        "Write SQLite SQL. Quote an identifier with double quotes only when it needs quotes."
    }

    /// SQLite has no comment catalog.
    fn ai_comments_sql(&self, _: &str) -> Option<String> {
        None
    }

    fn engine(&self) -> Engine {
        Engine::Sqlite
    }

    fn family(&self) -> Option<Family> {
        Some(Family::Sqlite)
    }
}
