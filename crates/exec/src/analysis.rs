//! Local result analysis. Jobs read loaded rows in bounded steps and keep a memory reservation.

mod chart;
mod diff;
mod profile;

use std::sync::Arc;
use std::time::Instant;

use crate::grid::{Budget, ResultTable};

pub use chart::ChartKind;

pub const MEMORY_ERROR: &str =
    "Analysis canceled: memory limit reached. Close a result and try again.";
pub const STALE_ERROR: &str = "Analysis canceled: the result changed. Open the analysis again.";

/// A reservation for analysis indexes, points, and report text.
#[derive(Debug)]
pub struct Memory {
    budget: Arc<Budget>,
    bytes: usize,
}

impl Memory {
    pub fn new(budget: &Arc<Budget>) -> Self {
        Self {
            budget: budget.clone(),
            bytes: 0,
        }
    }

    pub fn reserve(&mut self, bytes: usize) -> Result<(), &'static str> {
        if !self.budget.reserve_bytes(bytes) {
            return Err(MEMORY_ERROR);
        }
        self.bytes += bytes;
        Ok(())
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        self.budget.release(self.bytes);
    }
}

pub struct Report {
    pub title: String,
    pub text: String,
    pub memory: Memory,
}

struct Output {
    text: String,
    memory: Memory,
}

impl Output {
    fn line(&mut self, line: &str) -> Result<(), &'static str> {
        self.memory.reserve((line.len() + 1) * 2)?;
        self.text.push_str(line);
        self.text.push('\n');
        Ok(())
    }
}

enum Task {
    Profile(profile::Profile),
    Chart(chart::Chart),
    Diff(diff::Diff),
}

pub struct Job {
    pub generation: u64,
    previous_generation: Option<u64>,
    title: String,
    task: Task,
    output: Output,
}

impl Job {
    pub fn profile(
        table: &ResultTable,
        column: usize,
        complete: bool,
        budget: &Arc<Budget>,
    ) -> Self {
        Self::new(
            table,
            None,
            "Column profile",
            Task::Profile(profile::Profile::new(column, complete)),
            budget,
        )
    }

    pub fn chart(
        table: &ResultTable,
        x: usize,
        y: usize,
        kind: ChartKind,
        complete: bool,
        budget: &Arc<Budget>,
    ) -> Self {
        Self::new(
            table,
            None,
            "Text chart",
            Task::Chart(chart::Chart::new(x, y, kind, complete)),
            budget,
        )
    }

    pub fn diff(
        current: &ResultTable,
        previous: &ResultTable,
        key: Vec<usize>,
        budget: &Arc<Budget>,
    ) -> Result<Self, &'static str> {
        if current.columns != previous.columns {
            return Err(
                "Comparison canceled: column names or order changed. Run a result with the same columns.",
            );
        }
        if key.is_empty() || key.iter().any(|&column| column >= current.columns.len()) {
            return Err("Comparison canceled: select at least one key column.");
        }
        let mut job = Self::new(
            current,
            Some(previous.generation),
            "Execution differences",
            Task::Diff(diff::Diff::new(key)),
            budget,
        );
        job.output.line("Previous -> Current")?;
        Ok(job)
    }

    fn new(
        table: &ResultTable,
        previous_generation: Option<u64>,
        title: &str,
        task: Task,
        budget: &Arc<Budget>,
    ) -> Self {
        Self {
            generation: table.generation,
            previous_generation,
            title: title.to_owned(),
            task,
            output: Output {
                text: String::new(),
                memory: Memory::new(budget),
            },
        }
    }

    /// Runs at most 128 steps. No partial report leaves the job.
    pub fn step(
        &mut self,
        table: &ResultTable,
        previous: Option<&ResultTable>,
        deadline: Instant,
    ) -> Result<bool, &'static str> {
        if self.generation != table.generation
            || self.previous_generation.is_some_and(|generation| {
                previous.is_none_or(|table| table.generation != generation)
            })
        {
            return Err(STALE_ERROR);
        }
        for _ in 0..128 {
            let done = match &mut self.task {
                Task::Profile(profile) => profile.step(table, &mut self.output)?,
                Task::Chart(chart) => chart.step(table, &mut self.output)?,
                Task::Diff(diff) => {
                    diff.step(table, previous.ok_or(STALE_ERROR)?, &mut self.output)?
                }
            };
            if done {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                break;
            }
        }
        Ok(false)
    }

    pub fn finish(mut self) -> Report {
        let kept = self.output.text.capacity().min(self.output.memory.bytes);
        self.output
            .memory
            .budget
            .release(self.output.memory.bytes - kept);
        self.output.memory.bytes = kept;
        Report {
            title: self.title,
            text: self.output.text,
            memory: self.output.memory,
        }
    }
}

/// Maps finite numbers without overflowing on a range from -MAX to MAX.
fn position(value: f64, min: f64, max: f64, cells: usize) -> usize {
    if min == max || cells <= 1 {
        return 0;
    }
    let scale = min.abs().max(max.abs()).max(f64::MIN_POSITIVE);
    let ratio = ((value / scale - min / scale) / (max / scale - min / scale)).clamp(0.0, 1.0);
    (ratio * (cells - 1) as f64).round() as usize
}

fn number(value: crate::grid::Value<'_>) -> Option<f64> {
    let crate::grid::Value::Number(text) = value else {
        return None;
    };
    text.parse::<f64>().ok().filter(|value| value.is_finite())
}
