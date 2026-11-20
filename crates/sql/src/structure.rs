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
    /// The sessions that wait for a lock, each with a session that holds it. The first
    /// column is the session that holds the lock, so a signal reaches it.
    Locks,
}

impl Scope {
    /// Returns true for a list of server sessions, where a signal can reach the session
    /// of the first column.
    pub fn takes_signals(self) -> bool {
        matches!(self, Self::Sessions | Self::Locks)
    }
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
/// `Statistics` needs a source with a statistics read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Columns,
    Indexes,
    ForeignKeys,
    Statistics,
}

impl Section {
    pub const ALL: [Self; 4] = [
        Self::Columns,
        Self::Indexes,
        Self::ForeignKeys,
        Self::Statistics,
    ];
    /// The sections of a source without a statistics read.
    pub const WITHOUT_STATISTICS: [Self; 3] = [Self::Columns, Self::Indexes, Self::ForeignKeys];

    /// Returns the position of the section in `ALL`.
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|section| *section == self)
            .unwrap_or(0)
    }

    /// Returns the sections that a source can show, in order. The list is a prefix of
    /// `ALL`, so the place of a section in it never changes.
    pub fn sections(dialect: Dialect) -> &'static [Self] {
        if has_statistics(dialect) {
            &Self::ALL
        } else {
            &Self::WITHOUT_STATISTICS
        }
    }

    /// Returns the section after this one in `list`, or before it, in a circle.
    pub fn step(self, list: &[Self], forward: bool) -> Self {
        if list.is_empty() {
            return self;
        }
        let count = list.len();
        let at = list
            .iter()
            .position(|section| *section == self)
            .unwrap_or(0);
        let distance = if forward { 1 } else { count - 1 };
        list[(at + distance) % count]
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

/// Returns true for a source with a list of the sessions that wait for a lock.
pub fn has_locks(dialect: Dialect) -> bool {
    structure_sql(
        dialect,
        Scope::Locks,
        &RelationRef::default(),
        Section::Columns,
    )
    .is_some()
}

/// Returns true for a source with a statistics read of a table.
pub fn has_statistics(dialect: Dialect) -> bool {
    structure_sql(
        dialect,
        Scope::Relation,
        &RelationRef::default(),
        Section::Statistics,
    )
    .is_some()
}
