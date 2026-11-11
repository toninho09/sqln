//! The two catalog queries of a diagram and the readers of their rows. The first query
//! returns the columns of each table of a schema. The second returns each pair of columns
//! of the foreign keys that leave or reach the schema.

use crate::catalog::RelationRef;
use crate::grid::Row;
use crate::sql::Dialect;
use crate::structure::name_literal;

use super::{ColumnRow, KeyRow};

/// Returns the column query and the key query of a schema. Redis has no tables.
pub fn diagram_sql(dialect: Dialect, schema: &str) -> Option<[String; 2]> {
    let schema = name_literal(schema, dialect);
    match dialect {
        Dialect::Postgres => Some([postgres_columns(&schema), postgres_keys(&schema)]),
        Dialect::MySql => Some([mysql_columns(&schema), mysql_keys(&schema)]),
        Dialect::Sqlite => Some([sqlite_columns(&schema), sqlite_keys(&schema)]),
        Dialect::DuckDb | Dialect::Redis | Dialect::Kafka => None,
    }
}

/// The partitions of a table stay out, and the key of a partitioned table appears once.
fn postgres_columns(schema: &str) -> String {
    format!(
        "SELECT n.nspname::text AS schema, c.relname::text AS relation, \
         a.attname::text AS name, pg_catalog.format_type(a.atttypid, a.atttypmod) AS type, \
         CASE WHEN a.attnotnull THEN 'no' ELSE 'yes' END AS nullable, \
         (SELECT k.place FROM pg_catalog.pg_index i \
            CROSS JOIN LATERAL pg_catalog.unnest(i.indkey::pg_catalog.int2[]) \
              WITH ORDINALITY AS k(attnum, place) \
          WHERE i.indrelid = c.oid AND i.indisprimary AND k.attnum = a.attnum) AS primary_key \
         FROM pg_catalog.pg_attribute a \
         JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE a.attnum > 0 AND NOT a.attisdropped AND c.relkind IN ('r', 'p', 'f') \
           AND NOT c.relispartition AND n.nspname = {schema} \
         ORDER BY c.relname, a.attnum"
    )
}

fn postgres_keys(schema: &str) -> String {
    format!(
        "SELECT n.nspname::text AS schema, c.relname::text AS relation, \
         k.conname::text AS key_name, a.attname::text AS column_name, \
         rn.nspname::text AS target_schema, r.relname::text AS target_relation, \
         ra.attname::text AS target_column \
         FROM pg_catalog.pg_constraint k \
         JOIN pg_catalog.pg_class c ON c.oid = k.conrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_catalog.pg_class r ON r.oid = k.confrelid \
         JOIN pg_catalog.pg_namespace rn ON rn.oid = r.relnamespace \
         CROSS JOIN ROWS FROM (pg_catalog.unnest(k.conkey), pg_catalog.unnest(k.confkey)) \
           WITH ORDINALITY AS u(attnum, target_attnum, place) \
         JOIN pg_catalog.pg_attribute a ON a.attrelid = k.conrelid AND a.attnum = u.attnum \
         JOIN pg_catalog.pg_attribute ra \
           ON ra.attrelid = k.confrelid AND ra.attnum = u.target_attnum \
         WHERE k.contype = 'f' AND k.conparentid = 0 AND NOT c.relispartition \
           AND (n.nspname = {schema} OR rn.nspname = {schema}) \
         ORDER BY n.nspname, c.relname, k.conname, u.place"
    )
}

fn mysql_columns(schema: &str) -> String {
    format!(
        "SELECT c.TABLE_SCHEMA AS `schema`, c.TABLE_NAME AS relation, c.COLUMN_NAME AS name, \
         c.COLUMN_TYPE AS type, LOWER(c.IS_NULLABLE) AS nullable, \
         k.ORDINAL_POSITION AS primary_key \
         FROM information_schema.COLUMNS c \
         JOIN information_schema.TABLES t \
           ON t.TABLE_SCHEMA = c.TABLE_SCHEMA AND t.TABLE_NAME = c.TABLE_NAME \
         LEFT JOIN information_schema.KEY_COLUMN_USAGE k \
           ON k.TABLE_SCHEMA = c.TABLE_SCHEMA AND k.TABLE_NAME = c.TABLE_NAME \
           AND k.COLUMN_NAME = c.COLUMN_NAME AND k.CONSTRAINT_NAME = 'PRIMARY' \
         WHERE c.TABLE_SCHEMA = {schema} AND t.TABLE_TYPE = 'BASE TABLE' \
         ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION"
    )
}

fn mysql_keys(schema: &str) -> String {
    format!(
        "SELECT TABLE_SCHEMA AS `schema`, TABLE_NAME AS relation, CONSTRAINT_NAME AS key_name, \
         COLUMN_NAME AS column_name, REFERENCED_TABLE_SCHEMA AS target_schema, \
         REFERENCED_TABLE_NAME AS target_relation, REFERENCED_COLUMN_NAME AS target_column \
         FROM information_schema.KEY_COLUMN_USAGE \
         WHERE REFERENCED_TABLE_NAME IS NOT NULL \
           AND (TABLE_SCHEMA = {schema} OR REFERENCED_TABLE_SCHEMA = {schema}) \
         ORDER BY TABLE_SCHEMA, TABLE_NAME, CONSTRAINT_NAME, ORDINAL_POSITION"
    )
}

/// The tables of the system, the views, and the shadow tables stay out.
const SQLITE_TABLES: &str = "\
    t.type = 'table' AND t.name NOT LIKE 'sqlite\\_%' ESCAPE '\\'";

fn sqlite_columns(schema: &str) -> String {
    format!(
        "SELECT t.schema AS \"schema\", t.name AS relation, c.name AS name, c.type AS type, \
         CASE WHEN c.\"notnull\" = 0 THEN 'yes' ELSE 'no' END AS nullable, \
         NULLIF(c.pk, 0) AS primary_key \
         FROM pragma_table_list t \
         JOIN pragma_table_xinfo(t.name, t.schema) c \
         WHERE t.schema = {schema} AND {SQLITE_TABLES} AND c.hidden <> 1 \
         ORDER BY t.name, c.cid"
    )
}

/// A key of SQLite reads a table of its own file. A key without target columns reads
/// the primary key of the target, so its target column is `NULL`.
fn sqlite_keys(schema: &str) -> String {
    format!(
        "SELECT t.schema AS \"schema\", t.name AS relation, f.id AS key_name, \
         f.\"from\" AS column_name, t.schema AS target_schema, f.\"table\" AS target_relation, \
         f.\"to\" AS target_column \
         FROM pragma_table_list t \
         JOIN pragma_foreign_key_list(t.name, t.schema) f \
         WHERE t.schema = {schema} AND {SQLITE_TABLES} \
         ORDER BY t.name, f.id, f.seq"
    )
}

/// Reads a row of the column query. A row without a name returns `None`.
pub fn parse_column(row: &Row) -> Option<ColumnRow> {
    Some(ColumnRow {
        relation: RelationRef {
            schema: row.text(0)?,
            relation: row.text(1)?,
        },
        name: row.text(2)?,
        ty: row.text(3).unwrap_or_default(),
        nullable: row.text(4).is_none_or(|nullable| nullable == "yes"),
        primary: row.text(5).and_then(|place| place.parse().ok()),
    })
}

/// Reads a row of the key query. A row without a name returns `None`.
pub fn parse_key(row: &Row) -> Option<KeyRow> {
    Some(KeyRow {
        relation: RelationRef {
            schema: row.text(0)?,
            relation: row.text(1)?,
        },
        key: row.text(2)?,
        column: row.text(3)?,
        target: RelationRef {
            schema: row.text(4)?,
            relation: row.text(5)?,
        },
        target_column: row.text(6),
    })
}
