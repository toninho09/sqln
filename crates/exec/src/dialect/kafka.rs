//! Kafka line commands and the SELECT subset. `complete::kafka` adds the words that
//! depend on the position.

use super::{Features, Vocabulary};
use crate::complete::keywords::{KAFKA_CLAUSES, KAFKA_COMMANDS, KAFKA_SELECT_WORDS};
use crate::plan::PlanSyntax;

pub struct Kafka;

const VOCABULARY: Vocabulary = Vocabulary {
    keywords: &[KAFKA_COMMANDS, KAFKA_CLAUSES, KAFKA_SELECT_WORDS],
    starters: &[KAFKA_COMMANDS],
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

impl Features for Kafka {
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
        crate::connector::kafka::GUIDE
    }

    fn ai_comments_sql(&self, _: &str) -> Option<String> {
        None
    }
}
