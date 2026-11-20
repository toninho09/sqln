//! Conservative validation of commands submitted by an assistant.

use super::Error;
use crate::sql::{Dialect, TokenKind, is_plain_read, tokenize};
use sqlparser::ast::{ObjectName, ObjectNamePart, Query, Select, Statement, Visit, Visitor};
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
        if dialect == Dialect::Kafka {
            // Each known Kafka command only reads. The runner refuses `TAIL` and
            // `EMIT CHANGES` outside a console, so an agent read always ends.
            if !crate::sql::is_known_kafka_read(sql) {
                return Err(refused());
            }
            crate::connector::kafka::command::parse(sql).map_err(|error| {
                let at = error
                    .offset
                    .map(|offset| format!(" Position: byte {offset} of the statement."))
                    .unwrap_or_default();
                Error::new(
                    "invalid_sql",
                    &format!(
                        "The Kafka statement does not parse: {}{at} Follow syntax.Kafka of get_context.",
                        error.message
                    ),
                )
            })?;
            continue;
        }
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
        let parsed = Parser::parse_sql(
            crate::sql::parser_dialect(dialect).ok_or_else(refused)?,
            sql,
        )
        .map_err(|_| refused())?;
        let mut read = Read {
            file_names: dialect.syntax().file_names,
        };
        if parsed.len() != 1 || parsed.visit(&mut read).is_break() {
            return Err(refused());
        }
    }
    Ok(())
}

/// Characters of a file path or a URL. A table name rarely has them.
const PATH_CHARACTERS: [char; 4] = ['.', '/', '\\', ':'];

struct Read {
    /// The database reads a quoted table name as a file or a URL.
    file_names: bool,
}
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
    /// A string as a table name reads a file in DuckDB, and a quoted path does so in
    /// DuckDB and Databricks. These sources have no native read-only context that stops
    /// such reads, so the policy refuses these names.
    fn pre_visit_relation(&mut self, relation: &ObjectName) -> ControlFlow<()> {
        for part in &relation.0 {
            let ObjectNamePart::Identifier(ident) = part else {
                return ControlFlow::Break(());
            };
            let quoted = ident.quote_style.is_some();
            if ident.quote_style == Some('\'')
                || (self.file_names && quoted && ident.value.contains(PATH_CHARACTERS))
            {
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    }
    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<()> {
        if select.into.is_some() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}
