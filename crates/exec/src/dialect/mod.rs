//! The features of each dialect in executions: the completion words now, and the other
//! per-dialect SQL later. Each dialect has one file here, and `features` is the only
//! `match` that chooses it. The lexical rules are in `sqln_sql::sql::dialect`.

mod duckdb;
mod kafka;
mod mysql;
mod postgres;
mod redis;
mod sqlite;

use crate::plan::PlanSyntax;
use crate::sql::Dialect;

/// The SQL family whose catalog reads, DDL, and data edits a dialect follows: the schema
/// compare, the DDL of an object, and the edit of table rows. A dialect chooses one family
/// in its file, or `None` when it has none of these features. A new family needs a new arm
/// in each `match` on `Family`, and the compiler shows each one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Postgres,
    MySql,
    /// `ALTER TABLE` adds only simple columns, and a rebuild makes each other change.
    Sqlite,
}

impl Family {
    /// The script runs in one transaction, with the schema in `search_path`.
    pub fn transactional_ddl(self) -> bool {
        match self {
            Family::Postgres => true,
            Family::MySql | Family::Sqlite => false,
        }
    }

    /// Each key has a name of its own.
    pub fn named_keys(self) -> bool {
        match self {
            Family::Postgres | Family::MySql => true,
            Family::Sqlite => false,
        }
    }

    /// The clause after a read that locks its row until the end of a data edit.
    pub fn row_lock(self) -> &'static str {
        match self {
            Family::Postgres | Family::MySql => " FOR UPDATE",
            // SQLite locks the whole database with `BEGIN IMMEDIATE`.
            Family::Sqlite => "",
        }
    }

    /// The statement that starts the transaction of a data edit.
    pub fn edit_begin(self) -> &'static str {
        match self {
            Family::Postgres | Family::MySql => "BEGIN",
            Family::Sqlite => "BEGIN IMMEDIATE",
        }
    }

    /// An update that keeps each value reports zero changed rows, as in MySQL.
    pub fn counts_changed_values(self) -> bool {
        match self {
            Family::MySql => true,
            Family::Postgres | Family::Sqlite => false,
        }
    }

    /// A data edit needs a transactional table engine (InnoDB), as in MySQL.
    pub fn checks_table_engine(self) -> bool {
        match self {
            Family::MySql => true,
            Family::Postgres | Family::Sqlite => false,
        }
    }

    /// Writes an insert where every column takes its default.
    pub fn insert_defaults(self, table: &str) -> String {
        match self {
            Family::MySql => format!("INSERT INTO {table} () VALUES ();"),
            Family::Postgres | Family::Sqlite => format!("INSERT INTO {table} DEFAULT VALUES;"),
        }
    }

    /// A column can be `GENERATED ALWAYS AS IDENTITY`, which refuses an explicit value.
    pub fn always_identity(self) -> bool {
        match self {
            Family::Postgres => true,
            Family::MySql | Family::Sqlite => false,
        }
    }
}

/// Returns the family of a dialect, or `None` for a source without these features.
pub fn family(dialect: Dialect) -> Option<Family> {
    features(dialect).family()
}

/// The words that completion offers for a dialect. A slice of lists keeps shared lists
/// in one place.
#[derive(Clone, Copy, Debug)]
pub struct Vocabulary {
    pub keywords: &'static [&'static [&'static str]],
    /// The words that start a statement.
    pub starters: &'static [&'static [&'static str]],
    pub types: &'static [&'static str],
    /// The types after `CAST`, when they are not `types`.
    pub cast_types: Option<&'static [&'static str]>,
    /// The functions with their signatures.
    pub functions: &'static [&'static [(&'static str, &'static str)]],
    /// The source has `ILIKE`.
    pub ilike: bool,
    /// The actions after `ALTER TABLE name`.
    pub alter_table_actions: &'static [&'static str],
    /// `MODIFY` and `CHANGE` change a column, as in MySQL.
    pub modify_column: bool,
    /// `ALTER COLUMN … TYPE` changes the type of a column, as in PostgreSQL.
    pub alter_column_type: bool,
    /// The options of each column definition, after the common ones.
    pub column_options: &'static [&'static str],
    /// The options of an integer column, after `column_options`.
    pub integer_options: &'static [&'static str],
    /// `INTEGER PRIMARY KEY` accepts `AUTOINCREMENT`, as in SQLite.
    pub integer_key_autoincrement: bool,
    /// `ORDER BY` accepts `NULLS FIRST` and `NULLS LAST`.
    pub nulls_order: bool,
    /// The join that completion adds after the common joins.
    pub join_extra: &'static str,
}

/// The features of one dialect. It has no default methods, so a new dialect must decide
/// each item.
pub trait Features: Sync {
    fn vocabulary(&self) -> &'static Vocabulary;
    /// The plan of a statement, or `None` when the source has no plan.
    fn plan(&self) -> Option<&'static PlanSyntax>;
    /// The column query and the key query of a diagram. `schema` is a literal already.
    fn diagram_sql(&self, schema: &str) -> Option<[String; 2]>;
    /// The line of the AI system message that names the language.
    fn ai_language(&self) -> &'static str;
    /// The query of the comments of a schema for AI metadata. `schema` is a literal
    /// already.
    fn ai_comments_sql(&self, schema: &str) -> Option<String>;
    /// The family of the catalog reads, DDL, and data edits, or `None`.
    fn family(&self) -> Option<Family>;
}

/// Returns the features of a dialect.
pub fn features(dialect: Dialect) -> &'static dyn Features {
    match dialect {
        Dialect::Postgres => &postgres::Postgres,
        Dialect::MySql => &mysql::MySql,
        Dialect::Sqlite => &sqlite::Sqlite,
        Dialect::DuckDb => &duckdb::DuckDb,
        Dialect::Redis => &redis::Redis,
        Dialect::Kafka => &kafka::Kafka,
    }
}
