//! Databricks SQL. A schema name of the catalog is always `catalog.schema`, because the
//! explorer shows the schemas of all catalogs of Unity Catalog.

use super::{BASE, Backslash, SqlDialect, Syntax};
use crate::catalog::RelationRef;
use crate::sql::Dialect;
use crate::sql::parameters::Parameter;
use crate::sql::quote_literal;
use crate::structure::{Scope, Section, SessionSignal, name_literal};

pub struct Databricks;

const SYNTAX: Syntax = Syntax {
    nested_block_comment: true,
    string_backslash: Backslash::Always,
    double_quote_string: true,
    question_marker: false,
    identifier_quote: '`',
    literal_backslash: true,
    backslash_quote: true,
    booleans: ("TRUE", "FALSE"),
    dotted_schema: true,
    folds_to_lower: true,
    ..BASE
};

impl SqlDialect for Databricks {
    fn syntax(&self) -> &'static Syntax {
        &SYNTAX
    }

    fn parser(&self) -> Option<&'static dyn sqlparser::dialect::Dialect> {
        Some(&sqlparser::dialect::DatabricksDialect {})
    }

    fn text_literal(&self, value: &str) -> String {
        quote_literal(value, Dialect::Databricks)
    }

    /// The connector does not bind parameters.
    fn parameter_marker(&self, _: usize, _: &mut Parameter) -> Result<String, String> {
        Err("Databricks connections do not support query parameters.".to_owned())
    }

    /// Unity Catalog has no indexes and no session list. Its keys are only informative,
    /// so the structure shows only the columns.
    fn structure_sql(
        &self,
        scope: Scope,
        relation: &RelationRef,
        section: Section,
    ) -> Option<String> {
        let table = name_literal(&relation.relation, Dialect::Databricks);
        match (scope, section) {
            (Scope::Relation, Section::Columns) => Some(format!(
                "SELECT column_name AS name, full_data_type AS type, \
                 CASE WHEN is_nullable = 'YES' THEN 'yes' ELSE 'no' END AS nullable, \
                 column_default AS `default` \
                 FROM system.information_schema.columns \
                 WHERE table_name = {table}{} \
                 ORDER BY ordinal_position",
                namespace(&relation.schema)
            )),
            (Scope::Relation, _) | (Scope::Sessions | Scope::Locks, _) => None,
            (Scope::Schema, _) => Some(all_columns(&namespace(&relation.schema))),
            (Scope::Connection, _) => Some(all_columns("")),
        }
    }

    fn signal_sql(&self, _: SessionSignal, _: u64) -> Option<String> {
        None
    }
}

/// The filters of a `catalog.schema` name, with a leading space.
fn namespace(schema: &str) -> String {
    let literal = |name: &str| name_literal(name, Dialect::Databricks);
    match schema.split_once('.') {
        Some((catalog, schema)) => format!(
            " AND table_catalog = {} AND table_schema = {}",
            literal(catalog),
            literal(schema)
        ),
        None => format!(" AND table_schema = {}", literal(schema)),
    }
}

fn all_columns(namespace: &str) -> String {
    format!(
        "SELECT table_catalog || '.' || table_schema AS `schema`, table_name AS relation, \
         column_name AS name, full_data_type AS type, \
         CASE WHEN is_nullable = 'YES' THEN 'yes' ELSE 'no' END AS nullable, \
         column_default AS `default` \
         FROM system.information_schema.columns \
         WHERE table_schema <> 'information_schema'{namespace} \
         ORDER BY 1, table_name, ordinal_position"
    )
}
