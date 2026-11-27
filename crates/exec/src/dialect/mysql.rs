//! The completion words of MySQL.

use super::{Engine, Family, Features, Vocabulary};
use crate::catalog::Catalog;
use crate::complete::Completion;
use crate::complete::keywords::{COMMON, STARTERS};
use crate::complete::vocabulary::COMMON_FUNCTIONS;
use crate::diagram::sql::{mysql_columns, mysql_keys};
use crate::plan::{PlanFormat, PlanSyntax};
use crate::sql::Statement;

pub struct MySql;

const KEYWORDS: &[&str] = &[
    "AUTO_INCREMENT",
    "CALL",
    "CHANGE",
    "CHARSET",
    "COLLATE",
    "DATABASE",
    "DATABASES",
    "DELIMITER",
    "DESCRIBE",
    "DUPLICATE",
    "ENGINE",
    "EVENT",
    "FORCE",
    "GRANT",
    "IGNORE",
    "KILL",
    "LOCK",
    "MODIFY",
    "PROCEDURE",
    "REGEXP",
    "RENAME",
    "REPLACE",
    "RETURNS",
    "REVOKE",
    "RLIKE",
    "SHOW",
    "STATUS",
    "STRAIGHT_JOIN",
    "TABLES",
    "TRUNCATE",
    "UNLOCK",
    "UNSIGNED",
    "USE",
    "ZEROFILL",
];

const STARTERS_EXTRA: &[&str] = &[
    "CALL",
    "DELIMITER",
    "DESCRIBE",
    "GRANT",
    "REPLACE",
    "REVOKE",
    "SHOW",
    "TRUNCATE",
    "USE",
];

const TYPES: &[&str] = &[
    "TINYINT",
    "SMALLINT",
    "MEDIUMINT",
    "INT",
    "INTEGER",
    "BIGINT",
    "DECIMAL",
    "NUMERIC",
    "FLOAT",
    "DOUBLE",
    "BIT",
    "BOOLEAN",
    "CHAR",
    "VARCHAR",
    "BINARY",
    "VARBINARY",
    "TINYTEXT",
    "TEXT",
    "MEDIUMTEXT",
    "LONGTEXT",
    "TINYBLOB",
    "BLOB",
    "MEDIUMBLOB",
    "LONGBLOB",
    "DATE",
    "TIME",
    "DATETIME",
    "TIMESTAMP",
    "YEAR",
    "JSON",
    "ENUM",
    "SET",
];

/// The types that `CAST` accepts.
const CAST_TYPES: &[&str] = &[
    "BINARY", "CHAR", "DATE", "DATETIME", "DECIMAL", "DOUBLE", "FLOAT", "JSON", "NCHAR", "REAL",
    "SIGNED", "TIME", "UNSIGNED", "YEAR",
];

const FUNCTIONS: &[(&str, &str)] = &[
    ("SUBSTRING", "SUBSTRING(text, start, length)"),
    ("CONCAT", "CONCAT(value, ...)"),
    ("GROUP_CONCAT", "GROUP_CONCAT(expression)"),
    ("NOW", "NOW()"),
    ("DATE_FORMAT", "DATE_FORMAT(date, format)"),
    ("DATEDIFF", "DATEDIFF(date, other_date)"),
    ("IFNULL", "IFNULL(value, fallback)"),
    ("JSON_EXTRACT", "JSON_EXTRACT(document, path, ...)"),
];

const ALTER_TABLE_ACTIONS: &[&str] = &[
    "ADD COLUMN",
    "DROP COLUMN",
    "MODIFY COLUMN",
    "CHANGE COLUMN",
    "RENAME COLUMN",
    "RENAME TO",
    "ADD CONSTRAINT",
    "DROP FOREIGN KEY",
];

const PLAN: PlanSyntax = PlanSyntax {
    explain: "EXPLAIN FORMAT=TREE ",
    analyze: Some("EXPLAIN ANALYZE "),
    format: PlanFormat::MySqlTree,
};

const VOCABULARY: Vocabulary = Vocabulary {
    keywords: &[COMMON, KEYWORDS],
    starters: &[STARTERS, STARTERS_EXTRA],
    types: TYPES,
    cast_types: Some(CAST_TYPES),
    functions: &[COMMON_FUNCTIONS, FUNCTIONS],
    ilike: false,
    alter_table_actions: ALTER_TABLE_ACTIONS,
    modify_column: true,
    alter_column_type: false,
    column_options: &["COMMENT"],
    integer_options: &["AUTO_INCREMENT", "UNSIGNED"],
    integer_key_autoincrement: false,
    nulls_order: false,
    join_extra: "STRAIGHT_JOIN",
};

impl Features for MySql {
    fn vocabulary(&self) -> &'static Vocabulary {
        &VOCABULARY
    }

    fn command_argument(&self, _: &str, _: Option<&Catalog>) -> Option<Vec<String>> {
        None
    }

    fn command_line(&self, _: &str, _: usize, _: Option<&Catalog>) -> Option<Completion> {
        None
    }

    fn select_statement(
        &self,
        _: &str,
        _: &Statement,
        _: usize,
        _: Option<&Catalog>,
    ) -> Option<Completion> {
        None
    }

    fn plan(&self) -> Option<&'static PlanSyntax> {
        Some(&PLAN)
    }

    fn diagram_sql(&self, schema: &str) -> Option<[String; 2]> {
        Some([mysql_columns(schema), mysql_keys(schema)])
    }

    fn ai_language(&self) -> &'static str {
        "Write MySQL SQL. Quote an identifier with backticks only when it needs quotes."
    }

    fn ai_comments_sql(&self, literal: &str) -> Option<String> {
        Some(format!(
            "SELECT TABLE_NAME AS relation,COLUMN_NAME AS column_name,COLUMN_COMMENT AS comment FROM information_schema.COLUMNS WHERE TABLE_SCHEMA={literal} UNION ALL SELECT TABLE_NAME,NULL,TABLE_COMMENT FROM information_schema.TABLES WHERE TABLE_SCHEMA={literal}"
        ))
    }

    fn engine(&self) -> Engine {
        Engine::MySql
    }

    fn family(&self) -> Option<Family> {
        Some(Family::MySql)
    }

    fn default_bound(&self, _: &str, _: u64) -> Option<String> {
        None
    }

    fn agent_name(&self) -> &'static str {
        "MySql"
    }
}
