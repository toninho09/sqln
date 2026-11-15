//! The connector contract, the connection targets, session pools, and command traces.
//! It needs no driver library, so the crates above it do not recompile when a driver
//! changes. This crate depends only on `sqln-core` and `sqln-sql`.

pub mod connector;
pub mod pool;
pub mod trace;

pub(crate) use sqln_core::{catalog, grid, limited, message, sink, text};
pub(crate) use sqln_sql::sql;
