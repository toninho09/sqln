//! SQL text analysis. This module accepts incomplete text and never fails.

mod classify;
pub mod diagnostics;
mod format;
mod lexer;
pub mod literal;
pub mod parameters;
mod risk;
mod statement;

pub use classify::{
    changes_state, changes_without_result, counts_rows, first_keyword, is_known_redis_read,
    is_plain_read, is_secret, lacks_where, reads_or_writes_stdio, removes_prepared_statements,
    with_row_limit,
};
pub use format::format_statement;
pub use lexer::{Lexer, Token, TokenKind, tokenize};
pub use risk::run_risk;
pub use statement::{Statement, split, split_tokens, statement_at};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    Postgres,
    MySql,
    Sqlite,
    DuckDb,
    /// Redis and Valkey commands. This text is not SQL.
    Redis,
}

/// Shares parser dialect selection between assistant policy and local syntax checks.
pub(crate) fn parser_dialect(dialect: Dialect) -> Option<&'static dyn sqlparser::dialect::Dialect> {
    use sqlparser::dialect::{DuckDbDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
    Some(match dialect {
        Dialect::Postgres => &PostgreSqlDialect {},
        Dialect::MySql => &MySqlDialect {},
        Dialect::Sqlite => &SQLiteDialect {},
        Dialect::DuckDb => &DuckDbDialect {},
        Dialect::Redis => return None,
    })
}

/// Wraps a name in `open` quotes and doubles the quotes inside the name.
pub fn quote_with(name: &str, open: char) -> String {
    let mut quoted = String::with_capacity(name.len() + 2);
    quoted.push(open);
    for (index, part) in name.split(open).enumerate() {
        if index > 0 {
            quoted.push(open);
            quoted.push(open);
        }
        quoted.push_str(part);
    }
    quoted.push(open);
    quoted
}

/// Writes a name with the identifier quotes of the dialect. It always quotes.
pub fn quote_identifier(name: &str, dialect: Dialect) -> String {
    quote_with(name, if dialect == Dialect::MySql { '`' } else { '"' })
}

/// Writes a PostgreSQL literal in the `E` form, which does not change with
/// `standard_conforming_strings`. Other text cannot end this literal early.
pub fn escape_literal(text: &str) -> String {
    format!("E{}", quote_with(&text.replace('\\', "\\\\"), '\''))
}

/// Writes text as a string literal of the dialect. MySQL also reads a backslash as an escape.
pub fn quote_literal(text: &str, dialect: Dialect) -> String {
    let literal = quote_with(text, '\'');
    if dialect == Dialect::MySql {
        literal.replace('\\', "\\\\")
    } else {
        literal
    }
}
