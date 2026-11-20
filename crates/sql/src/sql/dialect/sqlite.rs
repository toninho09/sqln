use super::{BASE, SqlDialect, Syntax};
use crate::catalog::RelationRef;
use crate::sql::Dialect;
use crate::sql::parameters::{Kind, Parameter};
use crate::sql::quote_literal;
use crate::structure::{Scope, Section, SessionSignal, name_literal};

pub struct Sqlite;

const SYNTAX: Syntax = Syntax {
    bracket_identifier: true,
    at_marker: true,
    ..BASE
};

impl SqlDialect for Sqlite {
    fn syntax(&self) -> &'static Syntax {
        &SYNTAX
    }

    fn parser(&self) -> Option<&'static dyn sqlparser::dialect::Dialect> {
        Some(&sqlparser::dialect::SQLiteDialect {})
    }

    fn text_literal(&self, value: &str) -> String {
        quote_literal(value, Dialect::Sqlite)
    }

    fn parameter_marker(&self, _: usize, parameter: &mut Parameter) -> Result<String, String> {
        Ok(match parameter.kind {
            Kind::Number => "CAST(? AS NUMERIC)",
            _ => "?",
        }
        .to_owned())
    }

    fn structure_sql(
        &self,
        scope: Scope,
        relation: &RelationRef,
        section: Section,
    ) -> Option<String> {
        let schema = name_literal(&relation.schema, Dialect::Sqlite);
        let table = name_literal(&relation.relation, Dialect::Sqlite);
        Some(match (scope, section) {
            (Scope::Relation, Section::Columns) => columns(&schema, &table),
            (Scope::Relation, Section::Indexes) => indexes(&schema, &table),
            (Scope::Relation, Section::ForeignKeys) => foreign_keys(&schema, &table),
            (Scope::Relation, Section::Statistics) => return None,
            (Scope::Schema, _) => all_columns(Some(&schema)),
            (Scope::Connection, _) => all_columns(None),
            (Scope::Sessions | Scope::Locks, _) => return None,
        })
    }

    fn signal_sql(&self, _: SessionSignal, _: u64) -> Option<String> {
        None
    }
}

const COLUMN_DATA: &str = "\
    c.name AS name, c.type AS type, \
    CASE WHEN c.\"notnull\" = 0 THEN 'yes' ELSE 'no' END AS nullable, \
    c.dflt_value AS \"default\"";

fn columns(schema: &str, table: &str) -> String {
    format!(
        "SELECT {COLUMN_DATA}, CASE WHEN c.pk > 0 THEN 'PK' END AS \"key\" \
         FROM pragma_table_xinfo({table}, {schema}) c \
         WHERE c.hidden <> 1 \
         ORDER BY c.cid"
    )
}

/// `pragma_table_list` also lists the tables of the system, and the catalog hides them.
fn all_columns(schema: Option<&str>) -> String {
    let filter = schema.map_or_else(String::new, |schema| format!(" AND t.schema = {schema}"));
    format!(
        "SELECT t.schema AS \"schema\", t.name AS relation, {COLUMN_DATA} \
         FROM pragma_table_list t \
         JOIN pragma_table_xinfo(t.name, t.schema) c \
         WHERE t.schema <> 'temp' AND t.name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
           AND c.hidden <> 1{filter} \
         ORDER BY t.schema, t.name, c.cid"
    )
}

fn indexes(schema: &str, table: &str) -> String {
    format!(
        "SELECT l.name AS name, \
         (SELECT group_concat(coalesce(i.name, 'expression'), ', ' ORDER BY i.seqno) \
          FROM pragma_index_info(l.name, {schema}) i) AS columns, \
         CASE WHEN l.\"unique\" = 1 THEN 'yes' ELSE 'no' END AS \"unique\" \
         FROM pragma_index_list({table}, {schema}) l \
         ORDER BY l.name"
    )
}

/// A key of SQLite has a number and no name. A key without target columns reads the
/// primary key of the other table.
fn foreign_keys(schema: &str, table: &str) -> String {
    format!(
        "SELECT f.id AS name, \
         group_concat(f.\"from\", ', ' ORDER BY f.seq) AS columns, \
         f.\"table\" || coalesce(' (' || group_concat(f.\"to\", ', ' ORDER BY f.seq) || ')', '') \
           AS \"references\", \
         f.on_update AS on_update, f.on_delete AS on_delete \
         FROM pragma_foreign_key_list({table}, {schema}) f \
         GROUP BY f.id \
         ORDER BY f.id"
    )
}
