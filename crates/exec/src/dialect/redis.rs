//! Redis and Valkey commands. Only the command words apply.

use super::{Features, Vocabulary};
use crate::complete::keywords::REDIS;
use crate::plan::PlanSyntax;

pub struct Redis;

const VOCABULARY: Vocabulary = Vocabulary {
    keywords: &[REDIS],
    starters: &[REDIS],
    types: &[],
    cast_types: None,
    functions: &[],
    ilike: false,
    alter_table_actions: &[],
    modify_column: false,
    alter_column_type: false,
    column_options: &[],
    integer_options: &[],
    integer_key_autoincrement: false,
    nulls_order: true,
    join_extra: "FULL JOIN",
};

impl Features for Redis {
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
        "Write Redis commands, one command on each line."
    }

    fn ai_comments_sql(&self, _: &str) -> Option<String> {
        None
    }
}
