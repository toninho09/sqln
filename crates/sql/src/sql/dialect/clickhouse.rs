//! ClickHouse SQL. A database of ClickHouse is the schema of the explorer.

use super::{BASE, Backslash, SqlDialect, Syntax};
use crate::catalog::RelationRef;
use crate::sql::Dialect;
use crate::sql::parameters::Parameter;
use crate::sql::quote_literal;
use crate::structure::{Scope, Section, SessionSignal, name_literal};

pub struct ClickHouse;

/// The condition on a database column that keeps only the databases of the user.
#[macro_export]
macro_rules! clickhouse_user_databases {
    ($column:literal) => {
        concat!(
            $column,
            " NOT IN ('system', 'INFORMATION_SCHEMA', 'information_schema')"
        )
    };
}

const SYNTAX: Syntax = Syntax {
    hash_comment: true,
    string_backslash: Backslash::Always,
    question_marker: false,
    identifier_quote: '`',
    literal_backslash: true,
    booleans: ("true", "false"),
    ..BASE
};

impl SqlDialect for ClickHouse {
    fn syntax(&self) -> &'static Syntax {
        &SYNTAX
    }

    fn parser(&self) -> Option<&'static dyn sqlparser::dialect::Dialect> {
        Some(&sqlparser::dialect::ClickHouseDialect {})
    }

    fn text_literal(&self, value: &str) -> String {
        quote_literal(value, Dialect::ClickHouse)
    }

    /// The connector does not bind parameters.
    fn parameter_marker(&self, _: usize, _: &mut Parameter) -> Result<String, String> {
        Err("ClickHouse connections do not support query parameters.".to_owned())
    }

    /// The structure shows only the columns. ClickHouse has no foreign keys and no lock
    /// waits, and a query id is text, so the session list has no signals.
    fn structure_sql(
        &self,
        scope: Scope,
        relation: &RelationRef,
        section: Section,
    ) -> Option<String> {
        let database = name_literal(&relation.schema, Dialect::ClickHouse);
        let table = name_literal(&relation.relation, Dialect::ClickHouse);
        match (scope, section) {
            (Scope::Relation, Section::Columns) => Some(format!(
                "SELECT {COLUMN_DATA}, if(is_in_primary_key, 'PK', NULL) AS `key` \
                 FROM system.columns \
                 WHERE database = {database} AND table = {table} \
                 ORDER BY position"
            )),
            (Scope::Relation, _) | (Scope::Sessions | Scope::Locks, _) => None,
            (Scope::Schema, _) => Some(all_columns(&format!("database = {database}"))),
            (Scope::Connection, _) => Some(all_columns(clickhouse_user_databases!("database"))),
        }
    }

    fn signal_sql(&self, _: SessionSignal, _: u64) -> Option<String> {
        None
    }
}

const COLUMN_DATA: &str = "\
    name, type, if(startsWith(type, 'Nullable('), 'yes', 'no') AS nullable, \
    nullIf(default_expression, '') AS `default`";

fn all_columns(filter: &str) -> String {
    format!(
        "SELECT database AS `schema`, table AS relation, {COLUMN_DATA} \
         FROM system.columns \
         WHERE {filter} \
         ORDER BY database, table, position"
    )
}
