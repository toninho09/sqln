//! The structure of a table, a schema, or a connection: the catalog queries of a
//! structure tab. Each query has a stable order, so the pages repeat no row.

use crate::catalog::RelationRef;
use crate::sql::{Dialect, escape_literal, quote_literal};

/// What a structure tab shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Relation,
    Schema,
    Connection,
    /// The sessions of the server, not an object of the catalog.
    Sessions,
}

/// What a sessions tab asks the database to do to one session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionSignal {
    /// Stops the statement in progress. The session stays open.
    CancelQuery,
    /// Closes the session.
    EndSession,
}

/// One grid of a structure tab. Only the scope `Relation` has more than `Columns`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Columns,
    Indexes,
    ForeignKeys,
}

impl Section {
    pub const ALL: [Self; 3] = [Self::Columns, Self::Indexes, Self::ForeignKeys];

    /// Returns the position of the section in `ALL`.
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|section| *section == self)
            .unwrap_or(0)
    }

    /// Returns the section after this one, or before it, in a circle.
    pub fn step(self, forward: bool) -> Self {
        let count = Self::ALL.len();
        let distance = if forward { 1 } else { count - 1 };
        Self::ALL[(self.index() + distance) % count]
    }
}

/// The state of a structure tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Structure {
    pub scope: Scope,
    pub section: Section,
    /// The name of the table, the schema, or the connection.
    pub name: String,
}

impl Structure {
    pub fn new(scope: Scope, name: String) -> Self {
        Self {
            scope,
            section: Section::default(),
            name,
        }
    }

    pub fn has_sections(&self) -> bool {
        self.scope == Scope::Relation
    }
}

/// Writes a name of the catalog as a literal.
pub fn name_literal(name: &str, dialect: Dialect) -> String {
    if dialect.syntax().escaped_name_literal {
        escape_literal(name)
    } else {
        quote_literal(name, dialect)
    }
}

/// Returns the statement of a signal. The number of a session is a `u64`, so no text of
/// a cell reaches the SQL.
pub fn signal_sql(dialect: Dialect, signal: SessionSignal, session: u64) -> Option<String> {
    dialect.sql().signal_sql(signal, session)
}

/// Returns the query of a section. Redis has no structure, so it returns `None`. A schema
/// and a connection read only their columns. SQLite has no sessions to list.
pub fn structure_sql(
    dialect: Dialect,
    scope: Scope,
    relation: &RelationRef,
    section: Section,
) -> Option<String> {
    dialect.sql().structure_sql(scope, relation, section)
}

/// Returns true for a source with a list of sessions.
pub fn has_sessions(dialect: Dialect) -> bool {
    structure_sql(
        dialect,
        Scope::Sessions,
        &RelationRef::default(),
        Section::Columns,
    )
    .is_some()
}
