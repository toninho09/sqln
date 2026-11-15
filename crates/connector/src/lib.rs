//! The connectors of SQLN in one place: the contract of `sqln-session` and the driver
//! crates. Only the `sqln` crate depends on this crate, so a change in one driver
//! recompiles that driver, this crate, and `sqln`.

pub mod connector;

pub use sqln_session::{pool, trace};
pub use sqln_vault as vault;
