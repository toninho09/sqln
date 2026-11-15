//! The DuckDB connector. It depends on `sqln-session` and on no other driver.

mod duckdb;

pub use duckdb::*;

pub(crate) use sqln_core::{catalog, grid, sink, text};
pub(crate) use sqln_session::connector::*;
pub(crate) use sqln_session::{connector, trace};
pub(crate) use sqln_sql::sql;
