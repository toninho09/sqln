//! Executions and their jobs: runs, data edits, imports, exports, comparisons,
//! diagrams, completion, AI metadata, and the assistant operations.
//! This crate never depends on the interface.

pub mod agent;
pub mod ai;
pub mod analysis;
pub mod command_log;
pub mod compare;
pub mod complete;
pub mod connection;
pub mod data_edit;
pub mod definition;
pub mod diagram;
pub mod dialect;
pub mod exec;
pub mod export;
pub mod import;
pub mod plan;

pub(crate) use sqln_core::{
    catalog, clipboard, filter_list, grid, limited, line_input, message, progress, sink, text, wrap,
};
pub(crate) use sqln_session::{connector, trace};
pub(crate) use sqln_sql::{sql, structure};
pub(crate) use sqln_store::{creds, store};
