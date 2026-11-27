//! The completion words of Databricks SQL.

use super::{Engine, Family, Features, Vocabulary};
use crate::catalog::Catalog;
use crate::complete::Completion;
use crate::complete::keywords::{COMMON, STARTERS};
use crate::complete::vocabulary::COMMON_FUNCTIONS;
use crate::plan::PlanSyntax;
use crate::sql::Statement;

pub struct Databricks;

const KEYWORDS: &[&str] = &[
    "CATALOG",
    "CATALOGS",
    "DESCRIBE",
    "EXPLODE",
    "LATERAL",
    "OPTIMIZE",
    "PIVOT",
    "QUALIFY",
    "RLIKE",
    "SHOW",
    "TABLESAMPLE",
    "UNPIVOT",
    "USING",
    "VOLUME",
];

const STARTERS_EXTRA: &[&str] = &["DESCRIBE", "SHOW", "USE"];

const TYPES: &[&str] = &[
    "ARRAY",
    "BIGINT",
    "BINARY",
    "BOOLEAN",
    "DATE",
    "DECIMAL",
    "DOUBLE",
    "FLOAT",
    "INT",
    "INTERVAL",
    "MAP",
    "SMALLINT",
    "STRING",
    "STRUCT",
    "TIMESTAMP",
    "TIMESTAMP_NTZ",
    "TINYINT",
    "VARIANT",
];

const VOCABULARY: Vocabulary = Vocabulary {
    keywords: &[COMMON, KEYWORDS],
    starters: &[STARTERS, STARTERS_EXTRA],
    types: TYPES,
    cast_types: None,
    functions: &[COMMON_FUNCTIONS],
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

impl Features for Databricks {
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
        "Write Databricks SQL. Name a table as catalog.schema.table. Quote an identifier \
         with backticks only when it needs quotes."
    }

    fn ai_comments_sql(&self, _: &str) -> Option<String> {
        None
    }

    fn engine(&self) -> Engine {
        Engine::Databricks
    }

    fn family(&self) -> Option<Family> {
        None
    }

    fn default_bound(&self, _: &str, _: u64) -> Option<String> {
        None
    }

    fn agent_name(&self) -> &'static str {
        "Databricks"
    }
}
