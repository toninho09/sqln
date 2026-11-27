//! Kafka line commands and the SELECT subset. `complete::kafka` adds the words that
//! depend on the position.

use super::{Engine, Family, Features, Vocabulary};
use crate::catalog::Catalog;
use crate::complete::Completion;
use crate::complete::keywords::{KAFKA_CLAUSES, KAFKA_COMMANDS, KAFKA_SELECT_WORDS};
use crate::plan::PlanSyntax;
use crate::sql::{Dialect, Statement};

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

    fn command_argument(&self, command: &str, catalog: Option<&Catalog>) -> Option<Vec<String>> {
        let schema = crate::connector::kafka::argument_schema(command)?;
        let names =
            catalog
                .and_then(|catalog| catalog.schema(schema))
                .map_or(Vec::new(), |schema| {
                    schema
                        .relations
                        .iter()
                        .map(|relation| relation.name.clone())
                        .collect()
                });
        Some(names)
    }

    fn command_line(
        &self,
        line: &str,
        cursor: usize,
        catalog: Option<&Catalog>,
    ) -> Option<Completion> {
        Some(crate::complete::kafka::suggest_line(line, cursor, catalog))
    }

    fn select_statement(
        &self,
        sql: &str,
        statement: &Statement,
        cursor: usize,
        catalog: Option<&Catalog>,
    ) -> Option<Completion> {
        let select = crate::sql::first_keyword(statement.text(sql), Dialect::Kafka)
            .is_some_and(|word| word.eq_ignore_ascii_case("SELECT"));
        select.then(|| {
            crate::complete::kafka::suggest(sql, (statement.start, statement.end), cursor, catalog)
        })
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

    fn engine(&self) -> Engine {
        Engine::Kafka
    }

    fn family(&self) -> Option<Family> {
        None
    }

    fn default_bound(&self, sql: &str, scan: u64) -> Option<String> {
        crate::connector::kafka::command::bounded_read(sql, scan)
    }

    fn agent_name(&self) -> &'static str {
        "Kafka"
    }
}
