//! Redis and Valkey commands. Only the command words apply.

use super::{Engine, Family, Features, Vocabulary};
use crate::catalog::Catalog;
use crate::complete::Completion;
use crate::complete::keywords::REDIS;
use crate::plan::PlanSyntax;
use crate::sql::Statement;

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
        "Write Redis commands, one command on each line."
    }

    fn ai_comments_sql(&self, _: &str) -> Option<String> {
        None
    }

    fn engine(&self) -> Engine {
        Engine::Redis
    }

    fn family(&self) -> Option<Family> {
        None
    }

    fn default_bound(&self, _: &str, _: u64) -> Option<String> {
        None
    }

    fn agent_name(&self) -> &'static str {
        "Redis"
    }
}
