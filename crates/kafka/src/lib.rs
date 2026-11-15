//! The Kafka connector. It depends on `sqln-session` and on no other driver.

mod kafka;

pub use kafka::*;

pub(crate) use sqln_core::{catalog, grid, progress, sink, text};
pub(crate) use sqln_session::connector;
pub(crate) use sqln_session::connector::*;
pub(crate) use sqln_sql::sql;
