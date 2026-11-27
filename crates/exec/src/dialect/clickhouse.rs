//! The completion words of ClickHouse SQL.

use super::{Engine, Family, Features, Vocabulary};
use crate::catalog::Catalog;
use crate::complete::Completion;
use crate::complete::keywords::{COMMON, STARTERS};
use crate::complete::vocabulary::COMMON_FUNCTIONS;
use crate::plan::PlanSyntax;
use crate::sql::Statement;

pub struct ClickHouse;

const KEYWORDS: &[&str] = &[
    "ARRAY",
    "DESCRIBE",
    "ENGINE",
    "EXPLAIN",
    "FILL",
    "FINAL",
    "FORMAT",
    "GLOBAL",
    "ILIKE",
    "OPTIMIZE",
    "PARTITION",
    "PREWHERE",
    "SAMPLE",
    "SETTINGS",
    "SHOW",
    "TTL",
];

const STARTERS_EXTRA: &[&str] = &["DESCRIBE", "EXPLAIN", "OPTIMIZE", "SHOW", "USE"];

const TYPES: &[&str] = &[
    "Array",
    "Bool",
    "Date",
    "Date32",
    "DateTime",
    "DateTime64",
    "Decimal",
    "Enum8",
    "FixedString",
    "Float32",
    "Float64",
    "Int16",
    "Int32",
    "Int64",
    "Int8",
    "IPv4",
    "IPv6",
    "JSON",
    "LowCardinality",
    "Map",
    "Nullable",
    "String",
    "Tuple",
    "UInt16",
    "UInt32",
    "UInt64",
    "UInt8",
    "UUID",
];

const FUNCTIONS: &[(&str, &str)] = &[
    ("any", "any(x)"),
    ("arrayJoin", "arrayJoin(array)"),
    ("countIf", "countIf(condition)"),
    ("formatReadableSize", "formatReadableSize(bytes)"),
    ("groupArray", "groupArray(x)"),
    ("if", "if(condition, then, else)"),
    ("now", "now()"),
    ("quantile", "quantile(level)(x)"),
    ("sumIf", "sumIf(x, condition)"),
    ("toDate", "toDate(x)"),
    ("toDateTime", "toDateTime(x)"),
    ("toStartOfDay", "toStartOfDay(time)"),
    ("toStartOfHour", "toStartOfHour(time)"),
    ("toString", "toString(x)"),
    ("toUInt64", "toUInt64(x)"),
    ("uniq", "uniq(x)"),
    ("uniqExact", "uniqExact(x)"),
];

const VOCABULARY: Vocabulary = Vocabulary {
    keywords: &[COMMON, KEYWORDS],
    starters: &[STARTERS, STARTERS_EXTRA],
    types: TYPES,
    cast_types: None,
    functions: &[COMMON_FUNCTIONS, FUNCTIONS],
    ilike: true,
    alter_table_actions: &[],
    modify_column: false,
    alter_column_type: false,
    column_options: &[],
    integer_options: &[],
    integer_key_autoincrement: false,
    nulls_order: true,
    join_extra: "FULL JOIN",
};

impl Features for ClickHouse {
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
        None
    }

    fn diagram_sql(&self, _: &str) -> Option<[String; 2]> {
        None
    }

    fn ai_language(&self) -> &'static str {
        "Write ClickHouse SQL. Name a table as database.table. Quote an identifier with \
         backticks only when it needs quotes."
    }

    fn ai_comments_sql(&self, _: &str) -> Option<String> {
        None
    }

    fn engine(&self) -> Engine {
        Engine::ClickHouse
    }

    fn family(&self) -> Option<Family> {
        None
    }

    fn default_bound(&self, _: &str, _: u64) -> Option<String> {
        None
    }

    fn agent_name(&self) -> &'static str {
        "ClickHouse"
    }
}
