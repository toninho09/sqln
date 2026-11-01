//! The structure of a table, a schema, or a connection: the catalog queries of a
//! structure tab. Each query has a stable order, so the pages repeat no row.

use crate::catalog::RelationRef;
use crate::sql::{Dialect, quote_literal};

/// What a structure tab shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Relation,
    Schema,
    Connection,
}

/// One grid of a structure tab. Only the scope `Relation` has more than `Columns`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Columns,
    Indexes,
    ForeignKeys,
}

impl Section {
    pub const ALL: [Self; 3] = [Self::Columns, Self::Indexes, Self::ForeignKeys];

    /// Returns the position of the section in `ALL`.
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|section| *section == self)
            .unwrap_or(0)
    }

    /// Returns the section after this one, or before it, in a circle.
    pub fn step(self, forward: bool) -> Self {
        let count = Self::ALL.len();
        let distance = if forward { 1 } else { count - 1 };
        Self::ALL[(self.index() + distance) % count]
    }
}

/// The state of a structure tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Structure {
    pub scope: Scope,
    pub section: Section,
    /// The name of the table, the schema, or the connection.
    pub name: String,
}

impl Structure {
    pub fn new(scope: Scope, name: String) -> Self {
        Self {
            scope,
            section: Section::default(),
            name,
        }
    }

    pub fn has_sections(&self) -> bool {
        self.scope == Scope::Relation
    }
}

/// Writes a name of the catalog as a literal. The `E` form of PostgreSQL does not change
/// with `standard_conforming_strings`, so a name cannot end the literal early.
fn name_literal(name: &str, dialect: Dialect) -> String {
    if dialect == Dialect::Postgres {
        format!("E{}", quote_literal(&name.replace('\\', "\\\\"), dialect))
    } else {
        quote_literal(name, dialect)
    }
}

/// Returns the query of a section. Redis has no structure, so it returns `None`. A schema
/// and a connection read only their columns.
pub fn structure_sql(
    dialect: Dialect,
    scope: Scope,
    relation: &RelationRef,
    section: Section,
) -> Option<String> {
    if dialect == Dialect::Redis {
        return None;
    }
    let schema = name_literal(&relation.schema, dialect);
    let table = name_literal(&relation.relation, dialect);
    Some(match (dialect, scope, section) {
        (Dialect::Postgres, Scope::Relation, Section::Columns) => postgres_columns(&schema, &table),
        (Dialect::Postgres, Scope::Relation, Section::Indexes) => postgres_indexes(&schema, &table),
        (Dialect::Postgres, Scope::Relation, Section::ForeignKeys) => {
            postgres_foreign_keys(&schema, &table)
        }
        (Dialect::Postgres, Scope::Schema, _) => postgres_all_columns(Some(&schema)),
        (Dialect::Postgres, Scope::Connection, _) => postgres_all_columns(None),
        (Dialect::MySql, Scope::Relation, Section::Columns) => mysql_columns(&schema, &table),
        (Dialect::MySql, Scope::Relation, Section::Indexes) => mysql_indexes(&schema, &table),
        (Dialect::MySql, Scope::Relation, Section::ForeignKeys) => {
            mysql_foreign_keys(&schema, &table)
        }
        (Dialect::MySql, Scope::Schema, _) => mysql_all_columns(Some(&schema)),
        (Dialect::MySql, Scope::Connection, _) => mysql_all_columns(None),
        (_, Scope::Relation, Section::Columns) => sqlite_columns(&schema, &table),
        (_, Scope::Relation, Section::Indexes) => sqlite_indexes(&schema, &table),
        (_, Scope::Relation, Section::ForeignKeys) => sqlite_foreign_keys(&schema, &table),
        (_, Scope::Schema, _) => sqlite_all_columns(Some(&schema)),
        (_, Scope::Connection, _) => sqlite_all_columns(None),
    })
}

/// The columns that a table and a schema share, after the name.
const POSTGRES_COLUMN_DATA: &str = "\
    pg_catalog.format_type(a.atttypid, a.atttypmod) AS type, \
    CASE WHEN a.attnotnull THEN 'no' ELSE 'yes' END AS nullable, \
    pg_catalog.pg_get_expr(d.adbin, d.adrelid) AS \"default\"";
const POSTGRES_COLUMN_SOURCE: &str = "\
    FROM pg_catalog.pg_attribute a \
    JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
    JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
    LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
    WHERE a.attnum > 0 AND NOT a.attisdropped AND c.relkind IN ('r', 'p', 'f', 'v', 'm')";

fn postgres_columns(schema: &str, table: &str) -> String {
    format!(
        "SELECT a.attname::text AS name, {POSTGRES_COLUMN_DATA}, \
         CASE WHEN EXISTS (SELECT 1 FROM pg_catalog.pg_index i \
           WHERE i.indrelid = c.oid AND i.indisprimary AND a.attnum = ANY (i.indkey)) \
         THEN 'PK' END AS key \
         {POSTGRES_COLUMN_SOURCE} AND n.nspname = {schema} AND c.relname = {table} \
         ORDER BY a.attnum"
    )
}

fn postgres_all_columns(schema: Option<&str>) -> String {
    let filter = match schema {
        Some(schema) => format!("n.nspname = {schema}"),
        None => "n.nspname !~ '^pg_' AND n.nspname <> 'information_schema'".to_owned(),
    };
    format!(
        "SELECT n.nspname::text AS schema, c.relname::text AS relation, \
         a.attname::text AS name, {POSTGRES_COLUMN_DATA} \
         {POSTGRES_COLUMN_SOURCE} AND {filter} \
         ORDER BY n.nspname, c.relname, a.attnum"
    )
}

fn postgres_indexes(schema: &str, table: &str) -> String {
    format!(
        "SELECT i.relname::text AS name, \
         (SELECT pg_catalog.string_agg(pg_catalog.pg_get_indexdef(x.indexrelid, k, true), ', ' \
            ORDER BY k) \
          FROM pg_catalog.generate_series(1, x.indnatts) AS k) AS columns, \
         CASE WHEN x.indisunique THEN 'yes' ELSE 'no' END AS \"unique\" \
         FROM pg_catalog.pg_index x \
         JOIN pg_catalog.pg_class i ON i.oid = x.indexrelid \
         JOIN pg_catalog.pg_class c ON c.oid = x.indrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = {schema} AND c.relname = {table} \
         ORDER BY i.relname"
    )
}

/// Lists the names of the attributes of a key, in the order of the key.
fn postgres_key_names(key: &str, owner: &str) -> String {
    format!(
        "(SELECT pg_catalog.string_agg(a.attname::text, ', ' ORDER BY u.place) \
         FROM pg_catalog.unnest({key}) WITH ORDINALITY AS u(attnum, place) \
         JOIN pg_catalog.pg_attribute a ON a.attrelid = {owner} AND a.attnum = u.attnum)"
    )
}

fn postgres_action(code: &str) -> String {
    format!(
        "CASE {code} WHEN 'a' THEN 'NO ACTION' WHEN 'r' THEN 'RESTRICT' WHEN 'c' THEN 'CASCADE' \
         WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT' END"
    )
}

fn postgres_foreign_keys(schema: &str, table: &str) -> String {
    let columns = postgres_key_names("k.conkey", "k.conrelid");
    let targets = postgres_key_names("k.confkey", "k.confrelid");
    let on_update = postgres_action("k.confupdtype");
    let on_delete = postgres_action("k.confdeltype");
    format!(
        "SELECT k.conname::text AS name, {columns} AS columns, \
         rn.nspname::text || '.' || r.relname::text || ' (' || {targets} || ')' AS \"references\", \
         {on_update} AS on_update, {on_delete} AS on_delete \
         FROM pg_catalog.pg_constraint k \
         JOIN pg_catalog.pg_class c ON c.oid = k.conrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_catalog.pg_class r ON r.oid = k.confrelid \
         JOIN pg_catalog.pg_namespace rn ON rn.oid = r.relnamespace \
         WHERE k.contype = 'f' AND n.nspname = {schema} AND c.relname = {table} \
         ORDER BY k.conname"
    )
}

const MYSQL_COLUMN_DATA: &str = "\
    COLUMN_NAME AS name, COLUMN_TYPE AS type, LOWER(IS_NULLABLE) AS nullable, \
    COLUMN_DEFAULT AS `default`";

fn mysql_columns(schema: &str, table: &str) -> String {
    format!(
        "SELECT {MYSQL_COLUMN_DATA}, IF(COLUMN_KEY = 'PRI', 'PK', NULL) AS `key` \
         FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = {schema} AND TABLE_NAME = {table} \
         ORDER BY ORDINAL_POSITION"
    )
}

fn mysql_all_columns(schema: Option<&str>) -> String {
    let filter = match schema {
        Some(schema) => format!("TABLE_SCHEMA = {schema}"),
        None => "TABLE_SCHEMA NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')"
            .to_owned(),
    };
    format!(
        "SELECT TABLE_SCHEMA AS `schema`, TABLE_NAME AS relation, {MYSQL_COLUMN_DATA} \
         FROM information_schema.COLUMNS \
         WHERE {filter} \
         ORDER BY TABLE_SCHEMA, TABLE_NAME, ORDINAL_POSITION"
    )
}

fn mysql_indexes(schema: &str, table: &str) -> String {
    format!(
        "SELECT INDEX_NAME AS name, \
         GROUP_CONCAT(COALESCE(COLUMN_NAME, 'expression') ORDER BY SEQ_IN_INDEX SEPARATOR ', ') \
           AS columns, \
         IF(NON_UNIQUE = 0, 'yes', 'no') AS `unique` \
         FROM information_schema.STATISTICS \
         WHERE TABLE_SCHEMA = {schema} AND TABLE_NAME = {table} \
         GROUP BY INDEX_NAME, NON_UNIQUE \
         ORDER BY INDEX_NAME"
    )
}

fn mysql_foreign_keys(schema: &str, table: &str) -> String {
    format!(
        "SELECT k.CONSTRAINT_NAME AS name, \
         GROUP_CONCAT(k.COLUMN_NAME ORDER BY k.ORDINAL_POSITION SEPARATOR ', ') AS columns, \
         CONCAT(k.REFERENCED_TABLE_SCHEMA, '.', k.REFERENCED_TABLE_NAME, ' (', \
           GROUP_CONCAT(k.REFERENCED_COLUMN_NAME ORDER BY k.ORDINAL_POSITION SEPARATOR ', '), \
           ')') AS `references`, \
         r.UPDATE_RULE AS on_update, r.DELETE_RULE AS on_delete \
         FROM information_schema.KEY_COLUMN_USAGE k \
         JOIN information_schema.REFERENTIAL_CONSTRAINTS r \
           ON r.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA AND r.TABLE_NAME = k.TABLE_NAME \
           AND r.CONSTRAINT_NAME = k.CONSTRAINT_NAME \
         WHERE k.TABLE_SCHEMA = {schema} AND k.TABLE_NAME = {table} \
         GROUP BY k.CONSTRAINT_NAME, k.REFERENCED_TABLE_SCHEMA, k.REFERENCED_TABLE_NAME, \
           r.UPDATE_RULE, r.DELETE_RULE \
         ORDER BY k.CONSTRAINT_NAME"
    )
}

const SQLITE_COLUMN_DATA: &str = "\
    c.name AS name, c.type AS type, \
    CASE WHEN c.\"notnull\" = 0 THEN 'yes' ELSE 'no' END AS nullable, \
    c.dflt_value AS \"default\"";

fn sqlite_columns(schema: &str, table: &str) -> String {
    format!(
        "SELECT {SQLITE_COLUMN_DATA}, CASE WHEN c.pk > 0 THEN 'PK' END AS \"key\" \
         FROM pragma_table_xinfo({table}, {schema}) c \
         WHERE c.hidden <> 1 \
         ORDER BY c.cid"
    )
}

/// `pragma_table_list` also lists the tables of the system, and the catalog hides them.
fn sqlite_all_columns(schema: Option<&str>) -> String {
    let filter = schema.map_or_else(String::new, |schema| format!(" AND t.schema = {schema}"));
    format!(
        "SELECT t.schema AS \"schema\", t.name AS relation, {SQLITE_COLUMN_DATA} \
         FROM pragma_table_list t \
         JOIN pragma_table_xinfo(t.name, t.schema) c \
         WHERE t.schema <> 'temp' AND t.name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
           AND c.hidden <> 1{filter} \
         ORDER BY t.schema, t.name, c.cid"
    )
}

fn sqlite_indexes(schema: &str, table: &str) -> String {
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
fn sqlite_foreign_keys(schema: &str, table: &str) -> String {
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
