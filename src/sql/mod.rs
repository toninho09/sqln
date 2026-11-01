//! SQL text analysis. This module accepts incomplete text and never fails.

mod classify;
mod lexer;
mod statement;

pub use classify::{counts_rows, is_plain_read, reads_or_writes_stdio};
pub use lexer::{Lexer, Token, TokenKind, tokenize};
pub use statement::{Statement, split, split_tokens, statement_at};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    Postgres,
    MySql,
    Sqlite,
    /// Redis and Valkey commands. This text is not SQL.
    Redis,
}

/// Wraps a name in `open` quotes and doubles the quotes inside the name.
pub fn quote_with(name: &str, open: char) -> String {
    format!(
        "{open}{}{open}",
        name.replace(open, &format!("{open}{open}"))
    )
}

/// Writes a name with the identifier quotes of the dialect. It always quotes.
pub fn quote_identifier(name: &str, dialect: Dialect) -> String {
    quote_with(name, if dialect == Dialect::MySql { '`' } else { '"' })
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
