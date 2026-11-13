//! Cancellation and temporary memory for one Kafka read.

use std::io::{self, Write};
use std::sync::Arc;

use tokio_util::sync::{CancellationToken, DropGuard};

use crate::connector::ExecError;
use crate::grid::Budget;
use crate::ui::text;

#[derive(Clone)]
pub struct Evaluation {
    stop: CancellationToken,
    budget: Arc<Budget>,
}

impl Evaluation {
    pub fn new(stop: CancellationToken, bytes: usize) -> Self {
        Self {
            stop,
            budget: Arc::new(Budget::new(bytes)),
        }
    }

    pub fn check(&self) -> Result<(), ExecError> {
        if self.stop.is_cancelled() {
            Err(ExecError::new(text::KAFKA_EVAL_STOPPED))
        } else {
            Ok(())
        }
    }

    pub fn stopped(&self) -> bool {
        self.stop.is_cancelled()
    }

    /// Stops blocking work if its waiting future leaves scope before completion.
    pub fn cancel_on_drop(&self) -> DropGuard {
        self.stop.clone().drop_guard()
    }

    pub fn reserve(&self, bytes: usize) -> Result<Memory<'_>, ExecError> {
        self.check()?;
        if !self.budget.reserve_bytes(bytes) {
            return Err(ExecError::new(text::KAFKA_EVAL_MEMORY));
        }
        Ok(Memory {
            evaluation: self,
            bytes,
        })
    }

    /// Counts case conversion before it allocates its output.
    pub fn case(&self, value: &str, upper: bool) -> Result<(String, Memory<'_>), ExecError> {
        let bytes = value.chars().try_fold(0usize, |length, char| {
            self.check()?;
            let bytes = if upper {
                char.to_uppercase().map(char::len_utf8).sum::<usize>()
            } else {
                char.to_lowercase().map(char::len_utf8).sum::<usize>()
            };
            length
                .checked_add(bytes)
                .ok_or_else(|| ExecError::new(text::KAFKA_EVAL_MEMORY))
        })?;
        let memory = self.reserve(bytes)?;
        let text = if upper {
            value.to_uppercase()
        } else {
            value.to_lowercase()
        };
        Ok((text, memory))
    }

    #[cfg(test)]
    pub fn used(&self) -> usize {
        self.budget.used()
    }
}

impl Default for Evaluation {
    fn default() -> Self {
        Self::new(CancellationToken::new(), crate::grid::DEFAULT_LIMIT)
    }
}

/// Holds a reservation until its temporary value leaves scope.
pub struct Memory<'a> {
    evaluation: &'a Evaluation,
    bytes: usize,
}

impl Drop for Memory<'_> {
    fn drop(&mut self) {
        self.evaluation.budget.release(self.bytes);
    }
}

/// A UTF-8 buffer that checks its capacity before each allocation.
pub struct Text<'a> {
    data: Vec<u8>,
    memory: Memory<'a>,
}

impl<'a> Text<'a> {
    pub fn new(evaluation: &'a Evaluation) -> Result<Self, ExecError> {
        Ok(Self {
            data: Vec::new(),
            memory: evaluation.reserve(0)?,
        })
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<(), ExecError> {
        let evaluation = self.memory.evaluation;
        evaluation.check()?;
        let needed = self
            .data
            .len()
            .checked_add(bytes.len())
            .ok_or_else(|| ExecError::new(text::KAFKA_EVAL_MEMORY))?;
        if needed > self.data.capacity() {
            let available = evaluation
                .budget
                .limit()
                .saturating_sub(evaluation.budget.used())
                .saturating_add(self.memory.bytes);
            let capacity = needed
                .max(self.data.capacity().saturating_mul(2))
                .min(available);
            if capacity < needed {
                return Err(ExecError::new(text::KAFKA_EVAL_MEMORY));
            }
            let additional = capacity - self.memory.bytes;
            if !evaluation.budget.reserve_bytes(additional) {
                return Err(ExecError::new(text::KAFKA_EVAL_MEMORY));
            }
            self.memory.bytes = capacity;
            self.data
                .try_reserve_exact(capacity - self.data.len())
                .map_err(|_| ExecError::new(text::KAFKA_EVAL_MEMORY))?;
        }
        self.data.extend_from_slice(bytes);
        Ok(())
    }

    pub fn json(&mut self, value: &serde_json::Value) -> Result<(), ExecError> {
        serde_json::to_writer(&mut *self, value).map_err(|_| {
            if self.memory.evaluation.stopped() {
                ExecError::new(text::KAFKA_EVAL_STOPPED)
            } else {
                ExecError::new(text::KAFKA_EVAL_MEMORY)
            }
        })
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.data).expect("JSON and expression text are UTF-8")
    }

    pub fn into_parts(self) -> (String, Memory<'a>) {
        (
            String::from_utf8(self.data).expect("expression text is UTF-8"),
            self.memory,
        )
    }
}

impl Write for Text<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.push(bytes)
            .map_err(|_| io::Error::other("Kafka expression buffer stopped"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_canceled_buffer_releases_its_existing_allocation() {
        let stop = CancellationToken::new();
        let evaluation = Evaluation::new(stop.clone(), 32);
        let mut text = Text::new(&evaluation).unwrap();
        text.push(b"first").unwrap();
        assert!(evaluation.used() >= 5);
        stop.cancel();
        assert!(text.push(b"next").is_err());
        drop(text);
        assert_eq!(evaluation.used(), 0);
    }

    #[test]
    fn a_failed_buffer_releases_its_existing_allocation() {
        let evaluation = Evaluation::new(CancellationToken::new(), 8);
        let mut text = Text::new(&evaluation).unwrap();
        text.push(b"first").unwrap();
        assert!(text.json(&serde_json::json!({"too_long": 1})).is_err());
        drop(text);
        assert_eq!(evaluation.used(), 0);
    }

    #[test]
    fn dropping_a_waiting_future_stops_its_worker_but_completion_does_not() {
        let evaluation = Evaluation::default();
        evaluation.cancel_on_drop().disarm();
        assert!(!evaluation.stopped());
        drop(evaluation.cancel_on_drop());
        assert!(evaluation.stopped());
    }
}
