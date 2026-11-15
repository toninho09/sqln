//! The PostgreSQL connector. It depends on `sqln-session` and on no other driver.

mod postgres;

pub use postgres::*;

pub(crate) use sqln_core::{catalog, grid, limited, sink, text};
pub(crate) use sqln_session::connector::*;
pub(crate) use sqln_session::trace;
pub(crate) use sqln_sql::sql;
