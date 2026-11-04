//! Structure compare and data compare: the reads of two sources, their models or rows,
//! the differences, and the script that makes the target equal to the source.

pub mod data;
pub mod ddl;
pub mod form;
pub mod schema;
pub mod sql;
pub mod tab;

use crate::exec::ConnectionId;

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
