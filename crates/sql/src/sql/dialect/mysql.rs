use super::{BASE, Backslash, SqlDialect, Syntax};
use crate::catalog::RelationRef;
use crate::grid::hex;
use crate::sql::Dialect;
use crate::sql::parameters::{Kind, Parameter, mysql_number};
use crate::sql::quote_literal;
use crate::structure::{Scope, Section, SessionSignal, name_literal};

pub struct MySql;

const SYNTAX: Syntax = Syntax {
    dash_comment_needs_space: true,
    hash_comment: true,
    string_backslash: Backslash::Always,
    double_quote_string: true,
    delimiter_command: true,
    identifier_quote: '`',
    literal_backslash: true,
    ..BASE
};

impl SqlDialect for MySql {
    fn syntax(&self) -> &'static Syntax {
        &SYNTAX
    }

    fn parser(&self) -> Option<&'static dyn sqlparser::dialect::Dialect> {
        Some(&sqlparser::dialect::MySqlDialect {})
    }

    /// The hexadecimal form does not change with `NO_BACKSLASH_ESCAPES`.
    fn text_literal(&self, value: &str) -> String {
        if value.contains('\\') {
            format!("_utf8mb4 {}", hex(value.as_bytes()))
        } else {
            quote_literal(value, Dialect::MySql)
        }
    }

    /// MySQL binds strings, so a typed cast keeps the precision of a number.
    fn parameter_marker(&self, _: usize, parameter: &mut Parameter) -> Result<String, String> {
        if parameter.kind != Kind::Number {
            return Ok("?".to_owned());
        }
        let (value, precision, scale) = mysql_number(&parameter.value)?;
        parameter.value = value;
        Ok(format!("CAST(? AS DECIMAL({precision},{scale}))"))
    }

    fn structure_sql(
        &self,
        scope: Scope,
        relation: &RelationRef,
        section: Section,
    ) -> Option<String> {
        let schema = name_literal(&relation.schema, Dialect::MySql);
        let table = name_literal(&relation.relation, Dialect::MySql);
        Some(match (scope, section) {
            (Scope::Relation, Section::Columns) => columns(&schema, &table),
            (Scope::Relation, Section::Indexes) => indexes(&schema, &table),
            (Scope::Relation, Section::ForeignKeys) => foreign_keys(&schema, &table),
            (Scope::Schema, _) => all_columns(Some(&schema)),
            (Scope::Connection, _) => all_columns(None),
            (Scope::Sessions, _) => SESSIONS.to_owned(),
        })
    }

    fn signal_sql(&self, signal: SessionSignal, session: u64) -> Option<String> {
        Some(match signal {
            SessionSignal::CancelQuery => format!("KILL QUERY {session}"),
            SessionSignal::EndSession => format!("KILL {session}"),
        })
    }
}

/// `SHOW PROCESSLIST` does not accept the `LIMIT` of a page, and this table does.
const SESSIONS: &str = "\
    SELECT ID AS id, USER AS `user`, HOST AS host, DB AS `database`, COMMAND AS command, \
    TIME AS age, STATE AS state, INFO AS query \
    FROM information_schema.PROCESSLIST \
    WHERE ID <> CONNECTION_ID() AND COMMAND <> 'Daemon' \
    ORDER BY COMMAND = 'Sleep', TIME DESC, ID";

const COLUMN_DATA: &str = "\
    COLUMN_NAME AS name, COLUMN_TYPE AS type, LOWER(IS_NULLABLE) AS nullable, \
    COLUMN_DEFAULT AS `default`";

fn columns(schema: &str, table: &str) -> String {
    format!(
        "SELECT {COLUMN_DATA}, IF(COLUMN_KEY = 'PRI', 'PK', NULL) AS `key` \
         FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = {schema} AND TABLE_NAME = {table} \
         ORDER BY ORDINAL_POSITION"
    )
}

fn all_columns(schema: Option<&str>) -> String {
    let filter = match schema {
        Some(schema) => format!("TABLE_SCHEMA = {schema}"),
        None => "TABLE_SCHEMA NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')"
            .to_owned(),
    };
    format!(
        "SELECT TABLE_SCHEMA AS `schema`, TABLE_NAME AS relation, {COLUMN_DATA} \
         FROM information_schema.COLUMNS \
         WHERE {filter} \
         ORDER BY TABLE_SCHEMA, TABLE_NAME, ORDINAL_POSITION"
    )
}

fn indexes(schema: &str, table: &str) -> String {
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

fn foreign_keys(schema: &str, table: &str) -> String {
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
