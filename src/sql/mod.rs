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
}
