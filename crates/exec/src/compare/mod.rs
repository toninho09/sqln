//! Structure compare and data compare: the reads of two sources, their models or rows,
//! the differences, and the script that makes the target equal to the source.

pub mod data;
pub mod ddl;
pub mod form;
pub mod schema;
pub mod sql;

use crate::dialect::features;
use crate::exec::ConnectionId;
use crate::sql::Dialect;

/// How a source reads and writes its schema in a compare. A dialect chooses one style in
/// its file of `crate::dialect`. A new style needs a new arm in each `match` of this
/// module, and the compiler shows each one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Postgres,
    MySql,
    /// `ALTER TABLE` adds only simple columns, and a rebuild makes each other change.
    Sqlite,
}

impl Style {
    /// The script runs in one transaction, with the schema in `search_path`.
    pub fn transactional_ddl(self) -> bool {
        match self {
            Style::Postgres => true,
            Style::MySql | Style::Sqlite => false,
        }
    }

    /// Each key has a name of its own.
    pub fn named_keys(self) -> bool {
        match self {
            Style::Postgres | Style::MySql => true,
            Style::Sqlite => false,
        }
    }

    /// A column can be `GENERATED ALWAYS AS IDENTITY`, which refuses an explicit value.
    pub fn always_identity(self) -> bool {
        match self {
            Style::Postgres => true,
            Style::MySql | Style::Sqlite => false,
        }
    }
}

/// Returns the compare style of a dialect, or `None` for a source without a compare.
pub fn style(dialect: Dialect) -> Option<Style> {
    features(dialect).compare()
}

/// What a compare reads: the structure of a schema or a table, or the rows of a table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Structure,
    Data,
}

/// One side of a compare: a connection and one of its schemas. A table limits the
/// compare to that table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Side {
    pub connection: ConnectionId,
    pub schema: String,
    pub table: Option<String>,
}
