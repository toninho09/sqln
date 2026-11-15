//! The completion words of PostgreSQL. DuckDB shares most of them.

use super::{Engine, Family, Features, Vocabulary};
use crate::catalog::Catalog;
use crate::complete::Completion;
use crate::complete::keywords::{COMMON, STARTERS};
use crate::complete::vocabulary::COMMON_FUNCTIONS;
use crate::diagram::sql::{postgres_columns, postgres_keys};
use crate::plan::{PlanFormat, PlanSyntax};
use crate::sql::Statement;

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

const PLAN: PlanSyntax = PlanSyntax {
    explain: "EXPLAIN (FORMAT JSON) ",
    analyze: Some("EXPLAIN (ANALYZE, FORMAT JSON) "),
    format: PlanFormat::PostgresJson,
};

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
        Some([postgres_columns(schema), postgres_keys(schema)])
    }

    fn ai_language(&self) -> &'static str {
        "Write PostgreSQL SQL. Quote an identifier with double quotes only when it needs quotes."
    }

    fn ai_comments_sql(&self, literal: &str) -> Option<String> {
        Some(format!(
            "SELECT c.relname::text AS relation, a.attname::text AS column_name, pg_catalog.col_description(c.oid,a.attnum) AS comment FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace LEFT JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped WHERE n.nspname={literal} AND c.relkind IN ('r','p','f','v','m') UNION ALL SELECT c.relname::text,NULL::text,pg_catalog.obj_description(c.oid,'pg_class') FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname={literal} AND c.relkind IN ('r','p','f','v','m')"
        ))
    }

    fn engine(&self) -> Engine {
        Engine::Postgres
    }

    fn family(&self) -> Option<Family> {
        Some(Family::Postgres)
    }
}
