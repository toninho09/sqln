//! Configuration, console scripts, credentials, trust, and the other saved files.
//! This crate depends on `sqln-core` and `sqln-connector`, and never on the interface.

pub mod creds;
pub mod script_file;
pub mod store;
pub mod trust;

pub(crate) use sqln_connector::{connector, vault};
pub(crate) use sqln_core::{grid, input, text};
