//! The rules of each dialect. Each dialect has one file here, and `Dialect::sql` is the
//! only `match` that chooses it. Code outside this module reads these rules and does not
//! compare `Dialect` values, so a new dialect needs only a new file and a new arm.

mod duckdb;
mod kafka;
mod mysql;
mod postgres;
mod redis;
mod sqlite;

use super::Dialect;
use super::parameters::Parameter;

/// How a `'` string reads a backslash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backslash {
    Never,
    Always,
    /// Only in an `E'…'` string, as in PostgreSQL.
    AfterEscapePrefix,
}

/// How the dialect writes a literal of bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryLiteral {
    /// `X'00ff'`.
    XQuote,
    /// A text literal of `\x00ff`, as in PostgreSQL `bytea`.
    HexText,
}

/// The lexical rules of a dialect, and the rules that SQLN uses to write its SQL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Syntax {
    /// `--` starts a comment only before a space or the end, as in MySQL.
    pub dash_comment_needs_space: bool,
    /// `#` starts a line comment.
    pub hash_comment: bool,
    /// `/* /* */ */` is one comment.
    pub nested_block_comment: bool,
    pub string_backslash: Backslash,
    /// `"…"` is a string with backslash escapes, not an identifier.
    pub double_quote_string: bool,
    pub backtick_identifier: bool,
    pub bracket_identifier: bool,
    /// `$tag$…$tag$` is a string.
    pub dollar_quote: bool,
    /// In a routine body, `BEGIN` opens a block only in `BEGIN ATOMIC`.
    pub begin_needs_atomic: bool,
    /// The `DELIMITER` command changes the end of a statement.
    pub delimiter_command: bool,
    /// `?` is a native parameter marker.
    pub question_marker: bool,
    /// `@name` is a native parameter marker.
    pub at_marker: bool,
    /// The quote that SQLN writes around an identifier.
    pub identifier_quote: char,
    /// A string literal also reads a backslash as an escape, so SQLN doubles it.
    pub literal_backslash: bool,
    /// A name of the catalog uses the escape form of `escape_literal`.
    pub escaped_name_literal: bool,
    pub binary_literal: BinaryLiteral,
    /// The text of `true` and `false`.
    pub booleans: (&'static str, &'static str),
    /// A schema name can be `catalog.schema`, and each part needs its own quotes.
    pub dotted_schema: bool,
    /// The database folds a name without quotes to lower case.
    pub folds_to_lower: bool,
}

/// The behavior of one dialect. It has no default methods, so a new dialect must decide
/// each item.
pub trait SqlDialect: Sync {
    fn syntax(&self) -> &'static Syntax;
    /// The parser of `sqlparser`, or `None` for a source that is not SQL.
    fn parser(&self) -> Option<&'static dyn sqlparser::dialect::Dialect>;
    /// Writes text as a literal that does not change with the session.
    fn text_literal(&self, value: &str) -> String;
    /// Writes the native marker of a named parameter. `number` starts at 1. It can change
    /// the value to the form that the marker needs.
    fn parameter_marker(&self, number: usize, parameter: &mut Parameter) -> Result<String, String>;
}

impl Dialect {
    /// Returns the rules of the dialect.
    pub fn sql(self) -> &'static dyn SqlDialect {
        match self {
            Dialect::Postgres => &postgres::Postgres,
            Dialect::MySql => &mysql::MySql,
            Dialect::Sqlite => &sqlite::Sqlite,
            Dialect::DuckDb => &duckdb::DuckDb,
            Dialect::Redis => &redis::Redis,
            Dialect::Kafka => &kafka::Kafka,
        }
    }

    /// Returns the lexical rules of the dialect.
    pub fn syntax(self) -> &'static Syntax {
        self.sql().syntax()
    }
}

/// The rules of a SQL dialect without extra features. Each dialect file starts from this
/// value and changes only its own differences.
const BASE: Syntax = Syntax {
    dash_comment_needs_space: false,
    hash_comment: false,
    nested_block_comment: false,
    string_backslash: Backslash::Never,
    double_quote_string: false,
    backtick_identifier: true,
    bracket_identifier: false,
    dollar_quote: false,
    begin_needs_atomic: false,
    delimiter_command: false,
    question_marker: true,
    at_marker: false,
    identifier_quote: '"',
    literal_backslash: false,
    escaped_name_literal: false,
    binary_literal: BinaryLiteral::XQuote,
    booleans: ("1", "0"),
    dotted_schema: false,
    folds_to_lower: false,
};
