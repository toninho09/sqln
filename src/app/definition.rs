//! The read of the DDL of an object: a temporary task of the pool, its rows, and a new
//! console with the text.

use super::App;
use super::auth::Use;
use crate::definition::{Target, definition_sql, definition_text};
use crate::exec::{
    ConnectionId, ConsoleId, ExecEvent, ExecutionEnd, ExecutionId, RunOptions, StatementStatus,
};
use crate::grid::Row as GridRow;
use crate::navigator::{Row, RowKind};
use crate::ui::text;

/// One read of a DDL. The task of `console` belongs to the read only.
pub(super) struct DefinitionRead {
    console: ConsoleId,
    connection: ConnectionId,
    target: Target,
    execution: Option<ExecutionId>,
    /// The rows of each statement.
    results: Vec<Vec<GridRow>>,
    /// The reason of a failed statement.
    failure: Option<String>,
}

impl App {
    /// Returns the object of the DDL of a navigator row: a table, a view, or an object.
    pub(super) fn row_definition(&self, row: &Row) -> Option<(ConnectionId, Target)> {
        let item = &self.connections[row.connection];
        let target = match row.kind {
            RowKind::Table | RowKind::View | RowKind::Column => {
                let relation = row.relation_ref(&self.connections)?;
                let (_, found) = item
                    .catalog
                    .as_ref()?
                    .relation(Some(&relation.schema), &relation.relation)?;
                Target::relation(&relation, found.kind)
            }
            RowKind::Object(_) => {
                let schema = row.schema_name(&self.connections)?;
                Target::object(schema, row.object(&self.connections)?)
            }
            _ => return None,
        };
        definition_sql(item.dialect, &target)?;
        Some((item.id, target))
    }

    /// Reads the DDL of the object of a navigator row.
    pub(super) fn open_row_definition(&mut self, row: &Row) {
        match self.row_definition(row) {
            Some((connection, target)) => self.start_definition(connection, target),
            None => self.status = Some(text::NOT_FOR_OBJECT.to_owned()),
        }
    }

    /// Reads the DDL of an object in a temporary task of the pool. The end opens a new
    /// console with the text.
    pub(super) fn start_definition(&mut self, connection: ConnectionId, target: Target) {
        if self.config_blocked() {
            return;
        }
        let Some(dialect) = self.connection(connection).map(|item| item.dialect) else {
            return;
        };
        let Some(statements) = definition_sql(dialect, &target) else {
            return;
        };
        let Some(console) = self
            .manager
            .as_mut()
            .and_then(|manager| manager.open_console(connection))
        else {
            return;
        };
        let schema = target.schema.clone();
        self.definitions.push(DefinitionRead {
            console,
            connection,
            target,
            execution: None,
            results: Vec::new(),
            failure: None,
        });
        self.authorize(
            connection,
            Use::Definition {
                console,
                connection,
                statements,
                schema,
            },
        );
    }

    /// Starts a read that the user authorized. On PostgreSQL, a `search_path` with only
    /// the schema keeps the names of the schema without the schema, as in the compare.
    pub(super) fn start_definition_read(
        &mut self,
        console: ConsoleId,
        connection: ConnectionId,
        statements: Vec<String>,
        schema: String,
    ) {
        let Some(manager) = &self.manager else { return };
        let Some(read) = self
            .definitions
            .iter_mut()
            .find(|read| read.console == console && read.connection == connection)
        else {
            return;
        };
        let options = RunOptions {
            row_cap: Some(usize::MAX),
            schema: Some(schema),
            only_schema: true,
            ..RunOptions::default()
        };
        read.execution = Some(manager.run(console, statements, options));
    }

    /// Ends a read without a console: a canceled password, or a connection that leaves.
    pub(super) fn cancel_definition(&mut self, console: ConsoleId) {
        if let Some(index) = self
            .definitions
            .iter()
            .position(|read| read.console == console)
        {
            let read = self.definitions.remove(index);
            if let Some(manager) = &mut self.manager {
                manager.close_console(read.console);
            }
        }
    }

    pub(super) fn cancel_definitions_of(&mut self, connection: ConnectionId) {
        let consoles: Vec<ConsoleId> = self
            .definitions
            .iter()
            .filter(|read| read.connection == connection)
            .map(|read| read.console)
            .collect();
        for console in consoles {
            self.cancel_definition(console);
        }
    }

    /// Takes an event of a read. It returns false for any other event.
    pub(super) fn on_definition_event(&mut self, event: &mut ExecEvent) -> bool {
        let Some(console) = event.console() else {
            return false;
        };
        let Some(index) = self
            .definitions
            .iter()
            .position(|read| read.console == console)
        else {
            return false;
        };
        let read = &mut self.definitions[index];
        let current = event.execution().is_some() && event.execution() == read.execution;
        match event {
            ExecEvent::Rows { key, rows, bytes } => {
                // The text is small, and the console holds it, so the rows leave the budget.
                self.budget.release(*bytes);
                if current {
                    if read.results.len() <= key.index {
                        read.results.resize_with(key.index + 1, Vec::new);
                    }
                    read.results[key.index].append(rows);
                }
            }
            ExecEvent::StatementFinished {
                status: StatementStatus::Error(message),
                ..
            } if current => {
                read.failure.get_or_insert_with(|| message.clone());
            }
            ExecEvent::ExecutionFinished { end, message, .. } if current => {
                let end = *end;
                let message = message.take();
                let read = self.definitions.remove(index);
                if let Some(manager) = &mut self.manager {
                    manager.close_console(read.console);
                }
                self.end_definition(read, end, message);
            }
            _ => {}
        }
        true
    }

    fn end_definition(&mut self, read: DefinitionRead, end: ExecutionEnd, message: Option<String>) {
        let Some(item) = self.connection(read.connection) else {
            return;
        };
        let (dialect, name) = (item.dialect, item.name.clone());
        if end != ExecutionEnd::Completed {
            let reason = read.failure.or(message);
            self.status = Some(reason.unwrap_or_else(|| text::DEFINITION_STOPPED.to_owned()));
            return;
        }
        match definition_text(dialect, &read.target, &name, &read.results) {
            Ok(ddl) => self.open_text_console(read.connection, Some(read.target.schema), &ddl),
            Err(reason) => self.status = Some(reason.to_owned()),
        }
    }
}
