//! Kafka line commands and the SELECT subset. `complete::kafka` adds the words that
//! depend on the position.

use super::{Features, Vocabulary};
use crate::complete::keywords::{KAFKA_CLAUSES, KAFKA_COMMANDS, KAFKA_SELECT_WORDS};

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
}
