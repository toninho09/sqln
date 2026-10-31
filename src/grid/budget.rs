//! The global budget of grid data. It counts the bytes that the rows keep in memory.

use std::mem::size_of;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{Row, Value};

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

fn value_bytes(value: &Value) -> usize {
    match value {
        Value::Null => 0,
        Value::Text(text) | Value::Number(text) => text.len(),
        Value::Binary(bytes) => bytes.len(),
    }
}

/// Returns the bytes that one row keeps in memory, without allocator overhead.
pub fn row_cost(row: &[Value]) -> usize {
    size_of::<Row>() + std::mem::size_of_val(row) + row.iter().map(value_bytes).sum::<usize>()
}

impl Budget {
    pub fn new(limit: usize) -> Self {
        Self { limit: AtomicUsize::new(limit), used: AtomicUsize::new(0) }
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

    pub fn reserve(&self, row: &[Value]) -> Reservation {
        let limit = self.limit();
        if row.iter().any(|value| value_bytes(value) > limit) {
            return Reservation::ValueTooLarge;
        }
        let cost = row_cost(row);
        let reserved = self.used.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
            used.checked_add(cost).filter(|total| *total <= limit)
        });
        match reserved {
            Ok(_) => Reservation::Accepted(cost),
            Err(_) => Reservation::BudgetReached,
        }
    }

    pub fn release(&self, bytes: usize) {
        let _ = self.used.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
            Some(used.saturating_sub(bytes))
        });
    }
}
