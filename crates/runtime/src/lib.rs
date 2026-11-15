//! The execution manager: the console tasks, the pools, and the jobs. Only the `sqln`
//! crate depends on this crate, so a change here does not recompile `sqln-ui`.

pub mod exec;

pub(crate) use sqln_core::{catalog, grid, sink, text};
pub(crate) use sqln_exec::{agent, ai, command_log, compare, data_edit, dialect, export, import};
pub(crate) use sqln_session::{connector, pool};
pub(crate) use sqln_sql::{sql, structure};
