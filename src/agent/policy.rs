//! Conservative validation of commands submitted by an assistant.

use super::Error;
use crate::sql::{Dialect, TokenKind, is_plain_read, tokenize};
use sqlparser::ast::{Query, Select, Statement, Visit, Visitor};
use sqlparser::dialect::{DuckDbDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;
use std::ops::ControlFlow;

fn refused() -> Error {
    Error::new(
        "read_only_refused",
        "This command is not a supported read. Without full access, assistants run only known reads, without writes, session changes, locks, or unknown functions. Send other SQL with edit_console: the user reviews and runs it. The user can also give full access in SQLN.",
    )
}

/// Validate each statement before any statement reaches the database.
pub fn validate(statements: &[String], dialect: Dialect) -> Result<(), Error> {
    if statements.is_empty() {
        return Err(refused());
    }
    for sql in statements {
        if dialect == Dialect::Redis {
            // The connector also checks the server's readonly flag at execution time.
            if !crate::sql::is_known_redis_read(sql) {
                return Err(refused());
            }
            let words = crate::connector::redis::split_words(sql).map_err(|_| refused())?;
            if words.is_empty() {
                return Err(refused());
            }
            continue;
        }
        if !is_plain_read(sql, dialect) {
            return Err(refused());
        }
        for token in tokenize(sql, dialect) {
            if token.kind == TokenKind::Comment {
                let comment = &sql[token.start()..token.end()];
                if comment.starts_with("/*!")
                    || comment.starts_with("/*+")
                    || comment.starts_with("/*M!")
                {
                    return Err(refused());
                }
            }
        }
        let parsed = match dialect {
            Dialect::Postgres => Parser::parse_sql(&PostgreSqlDialect {}, sql),
            Dialect::MySql => Parser::parse_sql(&MySqlDialect {}, sql),
            Dialect::Sqlite => Parser::parse_sql(&SQLiteDialect {}, sql),
            Dialect::DuckDb => Parser::parse_sql(&DuckDbDialect {}, sql),
            Dialect::Redis => unreachable!(),
        }
        .map_err(|_| refused())?;
        if parsed.len() != 1 || parsed.visit(&mut Read).is_break() {
            return Err(refused());
        }
    }
    Ok(())
}

struct Read;
impl Visitor for Read {
    type Break = ();
    fn pre_visit_statement(&mut self, statement: &Statement) -> ControlFlow<()> {
        match statement {
            Statement::Query(_)
            | Statement::ShowVariable { .. }
            | Statement::ShowVariables { .. }
            | Statement::ShowColumns { .. }
            | Statement::ShowTables { .. }
            | Statement::ShowCreate { .. }
            | Statement::ShowSchemas { .. }
            | Statement::ShowDatabases { .. }
            | Statement::ExplainTable { .. } => ControlFlow::Continue(()),
            _ => ControlFlow::Break(()),
        }
    }
    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<()> {
        if !query.locks.is_empty() || query.settings.is_some() || query.format_clause.is_some() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<()> {
        if select.into.is_some() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}
