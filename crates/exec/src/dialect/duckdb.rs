//! The completion words of DuckDB. It uses the PostgreSQL lists.

use super::postgres::{ALTER_TABLE_ACTIONS, FUNCTIONS, KEYWORDS, STARTERS_EXTRA, TYPES};
use super::{Features, Vocabulary};
use crate::compare::Style;
use crate::complete::keywords::{COMMON, STARTERS};
use crate::complete::vocabulary::COMMON_FUNCTIONS;
use crate::plan::PlanSyntax;

pub struct DuckDb;

const VOCABULARY: Vocabulary = Vocabulary {
    keywords: &[COMMON, KEYWORDS],
    starters: &[STARTERS, STARTERS_EXTRA],
    types: TYPES,
    cast_types: None,
    functions: &[COMMON_FUNCTIONS, FUNCTIONS],
    ilike: false,
    alter_table_actions: ALTER_TABLE_ACTIONS,
    modify_column: false,
    alter_column_type: false,
    column_options: &[],
    integer_options: &[],
    integer_key_autoincrement: false,
    nulls_order: true,
    join_extra: "FULL JOIN",
};

impl Features for DuckDb {
    fn vocabulary(&self) -> &'static Vocabulary {
        &VOCABULARY
    }

    fn plan(&self) -> Option<&'static PlanSyntax> {
        None
    }

    fn diagram_sql(&self, _: &str) -> Option<[String; 2]> {
        None
    }

    fn ai_language(&self) -> &'static str {
        "Write DuckDB SQL. Quote an identifier with double quotes only when it needs quotes."
    }

    fn ai_comments_sql(&self, _: &str) -> Option<String> {
        None
    }

    fn compare(&self) -> Option<Style> {
        None
    }
}
