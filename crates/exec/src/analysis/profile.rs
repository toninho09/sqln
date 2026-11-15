use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hasher};

use super::{Output, number};
use crate::compare::data::{canonical, decimal};
use crate::grid::{ResultTable, Value, printable};

pub(super) struct Profile {
    column: usize,
    complete: bool,
    row: usize,
    histogram: bool,
    distinct: Distinct,
    nulls: usize,
    numeric: usize,
    ordered: usize,
    min: Option<usize>,
    max: Option<usize>,
    finite_min: f64,
    finite_max: f64,
    finite: usize,
    bins: [usize; 10],
}

impl Profile {
    pub fn new(column: usize, complete: bool) -> Self {
        Self {
            column,
            complete,
            row: 0,
            histogram: false,
            distinct: Distinct::default(),
            nulls: 0,
            numeric: 0,
            ordered: 0,
            min: None,
            max: None,
            finite_min: f64::INFINITY,
            finite_max: f64::NEG_INFINITY,
            finite: 0,
            bins: [0; 10],
        }
    }

    pub fn step(&mut self, table: &ResultTable, output: &mut Output) -> Result<bool, &'static str> {
        let value_at = |row: usize| table.rows[row].get(self.column).unwrap_or(Value::Null);
        if self.row < table.rows.len() {
            let value = value_at(self.row);
            if self.histogram {
                if let Some(value) = number(value) {
                    let bin = if self.finite_min == self.finite_max {
                        0
                    } else {
                        let scale = self
                            .finite_min
                            .abs()
                            .max(self.finite_max.abs())
                            .max(f64::MIN_POSITIVE);
                        let ratio = (value / scale - self.finite_min / scale)
                            / (self.finite_max / scale - self.finite_min / scale);
                        ((ratio * 10.0).floor() as usize).min(9)
                    };
                    self.bins[bin] += 1;
                }
            } else if value == Value::Null {
                self.nulls += 1;
            } else {
                if !self.distinct.step(table, self.column, self.row, output)? {
                    return Ok(false);
                }
                if let Value::Number(text) = value {
                    self.numeric += 1;
                    if decimal(text).is_some() {
                        self.ordered += 1;
                        if self.min.is_none_or(|row| {
                            numeric_cmp(value, value_at(row)) == Some(Ordering::Less)
                        }) {
                            self.min = Some(self.row);
                        }
                        if self.max.is_none_or(|row| {
                            numeric_cmp(value, value_at(row)) == Some(Ordering::Greater)
                        }) {
                            self.max = Some(self.row);
                        }
                    }
                }
                if let Some(value) = number(value) {
                    self.finite_min = self.finite_min.min(value);
                    self.finite_max = self.finite_max.max(value);
                    self.finite += 1;
                }
            }
            self.row += 1;
            return Ok(false);
        }
        if !self.histogram {
            self.histogram = true;
            self.row = 0;
            return Ok(false);
        }
        let name = table
            .columns
            .get(self.column)
            .ok_or("Column profile canceled: column not found.")?;
        output.line(&format!("Column: {}", printable(name)))?;
        output.line(if self.complete {
            "Scope: loaded rows (complete result)"
        } else {
            "Scope: loaded rows only (incomplete result)"
        })?;
        output.line(&format!("Rows: {}", table.rows.len()))?;
        let percent = if table.rows.is_empty() {
            0.0
        } else {
            self.nulls as f64 * 100.0 / table.rows.len() as f64
        };
        output.line(&format!("NULL: {} ({percent:.1}%)", self.nulls))?;
        output.line(&format!("Distinct (without NULL): {}", self.distinct.count))?;
        output.line(&format!("Numeric values: {}", self.numeric))?;
        if let Some(row) = self.min {
            output.line(&format!("Min: {}", value_at(row).display()))?;
        }
        if let Some(row) = self.max {
            output.line(&format!("Max: {}", value_at(row).display()))?;
        }
        if self.numeric > self.ordered {
            output.line(&format!(
                "Min/max exclude {} unsupported numeric forms.",
                self.numeric - self.ordered
            ))?;
        }
        if self.finite > 0 {
            output.line("Histogram: approximate numeric ranges")?;
            let largest = self.bins.iter().copied().max().unwrap_or(1).max(1);
            let count = if self.finite_min == self.finite_max {
                1
            } else {
                10
            };
            for bin in 0..count {
                let boundary = |part: usize| {
                    self.finite_min * (1.0 - part as f64 / 10.0)
                        + self.finite_max * (part as f64 / 10.0)
                };
                let label = if count == 1 {
                    format!("{:.4e}", self.finite_min)
                } else {
                    format!(
                        "{:.4e}..{:.4e}{}",
                        boundary(bin),
                        boundary(bin + 1),
                        if bin == 9 { "]" } else { ")" }
                    )
                };
                output.line(&format!(
                    "{label}: {} {}",
                    "#".repeat(self.bins[bin] * 20 / largest),
                    self.bins[bin]
                ))?;
            }
        }
        if self.numeric > self.finite {
            output.line(&format!(
                "Histogram excludes {} non-finite or out-of-range numbers.",
                self.numeric - self.finite
            ))?;
        }
        Ok(true)
    }
}

const CHUNK_BYTES: usize = 4096;

/// Keeps row indexes, not copies of complete values. Equal hashes still require a byte comparison.
#[derive(Default)]
struct Distinct {
    index: HashMap<u64, Vec<usize>>,
    pending: Option<DistinctValue>,
    count: usize,
    reserved: bool,
}

#[derive(Default)]
struct DistinctValue {
    hasher: DefaultHasher,
    offset: usize,
    hash: Option<u64>,
    candidate: usize,
}

impl Distinct {
    /// Hashes or compares at most one chunk before the job checks its frame deadline.
    fn step(
        &mut self,
        table: &ResultTable,
        column: usize,
        row: usize,
        output: &mut Output,
    ) -> Result<bool, &'static str> {
        if !self.reserved {
            output
                .memory
                .reserve(std::mem::size_of::<DistinctValue>())?;
            self.reserved = true;
        }
        let form_at = |row: usize| canonical(table.rows[row].get(column).unwrap()).unwrap();
        let form = form_at(row);
        let pending = self.pending.get_or_insert_with(DistinctValue::default);
        let Some(hash) = pending.hash else {
            let end = (pending.offset + CHUNK_BYTES).min(form.len());
            pending.hasher.write(&form[pending.offset..end]);
            pending.offset = end;
            if end == form.len() {
                pending.hasher.write_usize(form.len());
                pending.hash = Some(pending.hasher.finish());
                pending.offset = 0;
            }
            return Ok(false);
        };
        if let Some(&candidate) = self
            .index
            .get(&hash)
            .and_then(|rows| rows.get(pending.candidate))
        {
            let other = form_at(candidate);
            let end = (pending.offset + CHUNK_BYTES).min(form.len());
            if form.len() != other.len() || form[pending.offset..end] != other[pending.offset..end]
            {
                pending.candidate += 1;
                pending.offset = 0;
            } else if end == form.len() {
                self.pending = None;
                return Ok(true);
            } else {
                pending.offset = end;
            }
            return Ok(false);
        }
        output.memory.reserve(128)?;
        self.index.entry(hash).or_default().push(row);
        self.count += 1;
        self.pending = None;
        Ok(true)
    }
}

fn numeric_cmp(a: Value<'_>, b: Value<'_>) -> Option<Ordering> {
    let (Value::Number(a), Value::Number(b)) = (a, b) else {
        return None;
    };
    let (a, b) = (decimal(a)?, decimal(b)?);
    let negative_a = a.starts_with('-');
    let negative_b = b.starts_with('-');
    if negative_a != negative_b {
        return Some(if negative_a {
            Ordering::Less
        } else {
            Ordering::Greater
        });
    }
    let (a, b) = (a.trim_start_matches('-'), b.trim_start_matches('-'));
    let (aw, af) = a.split_once('.').unwrap_or((a, ""));
    let (bw, bf) = b.split_once('.').unwrap_or((b, ""));
    let fraction = || {
        af.bytes()
            .chain(std::iter::repeat(b'0'))
            .take(af.len().max(bf.len()))
            .cmp(
                bf.bytes()
                    .chain(std::iter::repeat(b'0'))
                    .take(af.len().max(bf.len())),
            )
    };
    let order = aw
        .len()
        .cmp(&bw.len())
        .then_with(|| aw.cmp(bw))
        .then_with(fraction);
    Some(if negative_a { order.reverse() } else { order })
}
