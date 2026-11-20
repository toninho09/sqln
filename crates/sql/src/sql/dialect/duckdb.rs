use super::{BASE, SqlDialect, Syntax};
use crate::catalog::RelationRef;
use crate::sql::Dialect;
use crate::sql::parameters::Parameter;
use crate::sql::quote_literal;
use crate::structure::name_literal;
use crate::structure::{Scope, Section, SessionSignal};

pub struct DuckDb;

const SYNTAX: Syntax = Syntax {
    dotted_schema: true,
    file_names: true,
    ..BASE
};

impl SqlDialect for DuckDb {
    fn syntax(&self) -> &'static Syntax {
        &SYNTAX
    }

    fn parser(&self) -> Option<&'static dyn sqlparser::dialect::Dialect> {
        Some(&sqlparser::dialect::DuckDbDialect {})
    }

    fn text_literal(&self, value: &str) -> String {
        quote_literal(value, Dialect::DuckDb)
    }

    fn parameter_marker(&self, _: usize, _: &mut Parameter) -> Result<String, String> {
        Ok("?".to_owned())
    }

    /// DuckDB filters by the `catalog.schema` name of an attached database, which one
    /// literal cannot write.
    fn structure_sql(
        &self,
        scope: Scope,
        relation: &RelationRef,
        section: Section,
    ) -> Option<String> {
        let table = name_literal(&relation.relation, Dialect::DuckDb);
        let namespace = namespace(&relation.schema);
        Some(match (scope, section) {
            (Scope::Relation, Section::Columns) => columns(&table, &namespace),
            (Scope::Relation, Section::Indexes) => indexes(&table, &namespace),
            (Scope::Relation, Section::ForeignKeys) => foreign_keys(&table, &namespace),
            (Scope::Relation, Section::Statistics) => return None,
            (Scope::Schema, _) => all_columns(&namespace),
            (Scope::Connection, _) => all_columns(""),
            (Scope::Sessions | Scope::Locks, _) => return None,
        })
    }

    fn signal_sql(&self, _: SessionSignal, _: u64) -> Option<String> {
        None
    }
}

/// The columns of the DuckDB catalog, before the name of the relation.
const COLUMN_DATA: &str = "\
    column_name AS name, data_type AS type, \
    CASE WHEN is_nullable THEN 'yes' ELSE 'no' END AS nullable, \
    column_default AS \"default\"";

/// The filters of a DuckDB schema name, with a leading space. A name with a dot is the
/// `catalog.schema` name of a database the session attached; a plain name is a schema
/// of the connection, and it matches every catalog with that schema, because a session
/// can change the current one with `SET schema`.
fn namespace(schema: &str) -> String {
    match schema.split_once('.') {
        Some((database, schema)) => format!(
            " AND database_name = {} AND schema_name = {}",
            name_literal(database, Dialect::DuckDb),
            name_literal(schema, Dialect::DuckDb)
        ),
        None => format!(
            " AND schema_name = {}",
            name_literal(schema, Dialect::DuckDb)
        ),
    }
}

fn columns(table: &str, namespace: &str) -> String {
    format!(
        "SELECT {COLUMN_DATA}, \
         CASE WHEN EXISTS (SELECT 1 FROM duckdb_constraints() k \
           WHERE k.constraint_type = 'PRIMARY KEY' AND k.database_name = c.database_name \
             AND k.schema_name = c.schema_name AND k.table_name = c.table_name \
             AND list_contains(k.constraint_column_names, c.column_name)) THEN 'PK' END AS \"key\" \
         FROM duckdb_columns() c \
         WHERE c.table_name = {table}{namespace} \
         ORDER BY c.column_index"
    )
}

fn all_columns(namespace: &str) -> String {
    format!(
        "SELECT c.schema_name AS \"schema\", c.table_name AS relation, {COLUMN_DATA} \
         FROM duckdb_columns() c \
         WHERE TRUE{namespace} \
         ORDER BY c.schema_name, c.table_name, c.column_index"
    )
}

/// A DuckDB index reports its `CREATE INDEX` text, and the columns stay in that text.
fn indexes(table: &str, namespace: &str) -> String {
    format!(
        "SELECT index_name AS name, sql AS columns, \
         CASE WHEN is_unique THEN 'yes' ELSE 'no' END AS \"unique\" \
         FROM duckdb_indexes() \
         WHERE table_name = {table}{namespace} \
         ORDER BY index_name"
    )
}

fn foreign_keys(table: &str, namespace: &str) -> String {
    format!(
        "SELECT constraint_name AS name, \
         list_aggregate(constraint_column_names, 'string_agg', ', ') AS columns, \
         referenced_table || coalesce(' (' || \
           list_aggregate(referenced_column_names, 'string_agg', ', ') || ')', '') AS \"references\", \
         NULL AS on_update, NULL AS on_delete \
         FROM duckdb_constraints() \
         WHERE constraint_type = 'FOREIGN KEY' AND table_name = {table}{namespace} \
         GROUP BY constraint_name, referenced_table, referenced_column_names \
         ORDER BY constraint_name"
    )
}
