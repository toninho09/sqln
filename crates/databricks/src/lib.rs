//! The Databricks SQL connector. It sends statements to a SQL warehouse through the SQL
//! Statement Execution API, and reads the results in the Arrow format.

mod databricks;

pub use databricks::*;

pub(crate) use sqln_core::{catalog, grid, sink, text};
pub(crate) use sqln_session::{connector, trace};
pub(crate) use sqln_sql::sql;
