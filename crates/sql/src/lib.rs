//! SQL analysis, formatting, literals, and the queries of the structure view.
//! This crate depends only on `sqln-core`.

pub mod sql;
pub mod structure;

pub(crate) use sqln_core::{catalog, grid, limited};
