//! The global budget of grid data. It counts the bytes that the rows keep in memory.

use std::mem::size_of;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{Row, Value};
use crate::text;

pub const DEFAULT_LIMIT: usize = 268_435_456;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reservation {
    Accepted(usize),
    /// The row does not fit in the free part of the budget.
    BudgetReached,
    /// One value alone is larger than the complete budget.
    ValueTooLarge,
}

#[derive(Debug)]
pub struct Budget {
    limit: AtomicUsize,
    used: AtomicUsize,
}

fn value_bytes(value: Value<'_>) -> usize {
    match value {
        Value::Null => 0,
        Value::Text(text) | Value::Number(text) => text.len(),
        Value::Binary(bytes) => bytes.len(),
    }
}

/// Returns the bytes that one row keeps in memory, without allocator overhead.
pub fn row_cost(row: &Row) -> usize {
    size_of::<Row>() + row.bytes()
}

impl Budget {
    pub fn new(limit: usize) -> Self {
        Self {
            limit: AtomicUsize::new(limit),
            used: AtomicUsize::new(0),
        }
    }

    pub fn used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    /// Changes the limit. Rows that are already loaded stay.
    pub fn set_limit(&self, limit: usize) {
        self.limit.store(limit, Ordering::Relaxed);
    }

    pub fn limit(&self) -> usize {
        self.limit.load(Ordering::Relaxed)
    }

    pub fn reserve(&self, row: &Row) -> Reservation {
        let limit = self.limit();
        let cost = row_cost(row);
        if cost > limit && row.cells().any(|value| value_bytes(value) > limit) {
            return Reservation::ValueTooLarge;
        }
        let reserved = self
            .used
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(cost).filter(|total| *total <= limit)
            });
        match reserved {
            Ok(_) => Reservation::Accepted(cost),
            Err(_) => Reservation::BudgetReached,
        }
    }

    /// Reserves a bounded snapshot or generated text in the shared budget.
    pub fn reserve_bytes(&self, bytes: usize) -> bool {
        self.used
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(bytes)
                    .filter(|total| *total <= self.limit())
            })
            .is_ok()
    }

    pub fn release(&self, bytes: usize) {
        let _ = self
            .used
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                Some(used.saturating_sub(bytes))
            });
    }
}

/// A row whose memory reservation ends with its last owner.
#[derive(Debug)]
pub struct SavedRow {
    pub row: Row,
    bytes: usize,
    budget: Arc<Budget>,
}

impl SavedRow {
    pub fn reserve(row: Row, budget: &Arc<Budget>) -> Result<Arc<Self>, String> {
        let Reservation::Accepted(bytes) = budget.reserve(&row) else {
            return Err(text::EDIT_BUDGET.to_owned());
        };
        Ok(Arc::new(Self {
            row,
            bytes,
            budget: budget.clone(),
        }))
    }

    /// Takes a row whose bytes the budget already holds.
    pub fn adopt(row: Row, bytes: usize, budget: &Arc<Budget>) -> Arc<Self> {
        Arc::new(Self {
            row,
            bytes,
            budget: budget.clone(),
        })
    }

    /// Returns the row and its reserved bytes. The caller keeps the reservation.
    pub fn take(mut self) -> (Row, usize) {
        let bytes = std::mem::take(&mut self.bytes);
        (std::mem::take(&mut self.row), bytes)
    }
}

impl Drop for SavedRow {
    fn drop(&mut self) {
        self.budget.release(self.bytes);
    }
}
