//! Transport-independent operations for a local assistant.

pub mod ipc;
pub mod policy;
pub mod protocol;

pub use protocol::*;

use crate::grid::Budget;
use std::sync::Arc;

/// Counts a retained assistant snapshot in the shared data budget.
pub struct Hold {
    budget: Arc<Budget>,
    bytes: usize,
}
impl Hold {
    pub fn new(bytes: usize, budget: &Arc<Budget>) -> Result<Self, Error> {
        if !budget.reserve_bytes(bytes) {
            return Err(Error::new(
                "budget_reached",
                "The assistant snapshot does not fit in the data budget. Close results and try again.",
            ));
        }
        Ok(Self {
            budget: budget.clone(),
            bytes,
        })
    }
}
impl Drop for Hold {
    fn drop(&mut self) {
        self.budget.release(self.bytes);
    }
}

/// Text artifacts retain their memory reservation until the last application owner releases them.
pub struct Text {
    pub value: String,
    _hold: Hold,
}
impl Text {
    pub fn new(value: String, budget: &Arc<Budget>) -> Result<Self, Error> {
        let hold = Hold::new(value.capacity(), budget)?;
        Ok(Self { value, _hold: hold })
    }
}
