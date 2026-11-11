//! Data compare: the rows of a source table against the rows of a target table, matched
//! by the primary key of the source. The source rows stay in memory, and the target rows
//! arrive as a stream: an equal row leaves at once.

use std::borrow::Cow;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use super::schema::{Column, Table};
use crate::grid::{Budget, Row, Value, row_cost};
use crate::sql::literal::value_literal;
use crate::sql::{Dialect, quote_identifier};
use crate::ui::text;

/// The largest power of ten that a number keeps in its decimal form. A larger exponent
/// compares as text.
const MAX_EXPONENT: i64 = 4096;

/// The identity that PostgreSQL makes and that refuses a value of a statement.
const ALWAYS_IDENTITY: &str = "GENERATED ALWAYS AS IDENTITY";

/// What a data compare takes from the two tables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The primary key of the source.
    pub key: Vec<String>,
    /// The columns of the target that make their own values, so the compare and the
    /// script leave them out. A key column never leaves.
    pub skipped: Vec<String>,
    /// True when an `INSERT` into the target needs `OVERRIDING SYSTEM VALUE`, because a
    /// key column of PostgreSQL is an identity that refuses a value.
    pub overriding: bool,
}

/// Checks two tables for a data compare: the source has a primary key, and the target has
/// each source column. It returns the reason of a refusal.
pub fn plan(source: &Table, target: &Table, dialect: Dialect) -> Result<Plan, String> {
    if source.primary.is_empty() {
        return Err(text::COMPARE_NO_KEY.to_owned());
    }
    if let Some(column) = source
        .columns
        .iter()
        .find(|column| target.column(&column.name).is_none())
    {
        return Err(text::compare_missing_column(&column.name));
    }
    let always = |column: &Column| {
        dialect == Dialect::Postgres && column.auto.as_deref() == Some(ALWAYS_IDENTITY)
    };
    let in_key = |column: &Column| source.primary.contains(&column.name);
    let skipped = target
        .columns
        .iter()
        .filter(|column| column.generated.is_some() || (always(column) && !in_key(column)))
        .map(|column| column.name.clone())
        .collect();
    let overriding = target
        .columns
        .iter()
        .any(|column| always(column) && in_key(column));
    Ok(Plan {
        key: source.primary.clone(),
        skipped,
        overriding,
    })
}

/// One difference of rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowChange {
    /// A row only in the source, by its place in the source rows.
    Insert(usize),
    /// A row on both sides with other values. The columns are places in the source.
    Update {
        source: usize,
        target: Row,
        columns: Vec<usize>,
    },
    /// A row only in the target.
    Delete(Row),
}

/// The rows of a data compare.
#[derive(Debug, Default)]
pub struct DataCompare {
    /// The columns of the source, in the order of its read.
    pub columns: Vec<String>,
    /// The places of the key in the source columns.
    key: Vec<usize>,
    /// The place of each source column in the target columns.
    map: Vec<usize>,
    /// The source columns that the compare and the script leave out, such as a
    /// generated column of the target.
    skipped: Vec<bool>,
    pub source: Vec<Row>,
    matched: Vec<bool>,
    index: HashMap<u64, Vec<usize>>,
    pub changes: Vec<RowChange>,
}

impl DataCompare {
    /// Starts a compare with the columns of the source read and the names of the key.
    /// It returns false when a key column is not in the read.
    pub fn new(columns: Vec<String>, key: &[String]) -> Option<Self> {
        let key = key
            .iter()
            .map(|name| columns.iter().position(|column| column == name))
            .collect::<Option<Vec<usize>>>()?;
        Some(Self {
            skipped: vec![false; columns.len()],
            columns,
            key,
            ..Self::default()
        })
    }

    /// Adds rows of the source to the index of keys.
    pub fn add_source(&mut self, rows: Vec<Row>) {
        for row in rows {
            let hash = key_hash(&row, self.key.iter().copied());
            self.index.entry(hash).or_default().push(self.source.len());
            self.source.push(row);
            self.matched.push(false);
        }
    }

    /// Gives the columns of the target read, and the source columns to leave out. It
    /// returns the first source column that the target does not have.
    pub fn set_target_columns(
        &mut self,
        columns: &[String],
        skipped: &[String],
    ) -> Result<(), String> {
        let mut map = Vec::with_capacity(self.columns.len());
        for (place, name) in self.columns.iter().enumerate() {
            let found = columns.iter().position(|column| column == name);
            map.push(found.ok_or_else(|| name.clone())?);
            self.skipped[place] = skipped.contains(name) && !self.key.contains(&place);
        }
        self.map = map;
        Ok(())
    }

    /// Compares rows of the target with the source. An equal row leaves the budget at
    /// once. It returns the bytes that the rows still hold.
    pub fn add_target(&mut self, rows: Vec<Row>, budget: &Budget) -> usize {
        let mut kept = 0;
        for row in rows {
            let target_key = self.key.iter().map(|place| self.map[*place]);
            let hash = key_hash(&row, target_key);
            let found = self.index.get(&hash).and_then(|places| {
                places.iter().copied().find(|&place| {
                    !self.matched[place]
                        && self.key.iter().all(|&column| {
                            same(
                                cell(&self.source[place], column),
                                cell(&row, self.map[column]),
                            )
                        })
                })
            });
            let Some(place) = found else {
                kept += row_cost(&row);
                self.changes.push(RowChange::Delete(row));
                continue;
            };
            self.matched[place] = true;
            let source = &self.source[place];
            let columns: Vec<usize> = (0..self.columns.len())
                .filter(|&column| !self.skipped[column])
                .filter(|&column| !same(cell(source, column), cell(&row, self.map[column])))
                .collect();
            if columns.is_empty() {
                budget.release(row_cost(&row));
            } else {
                kept += row_cost(&row);
                self.changes.push(RowChange::Update {
                    source: place,
                    target: row,
                    columns,
                });
            }
        }
        kept
    }

    /// Ends the compare: each source row without a match is a new row.
    pub fn finish(&mut self) {
        let missing = (0..self.source.len()).filter(|&place| !self.matched[place]);
        let inserts: Vec<RowChange> = missing.map(RowChange::Insert).collect();
        self.changes.extend(inserts);
        self.index = HashMap::new();
    }

    /// Returns the values of a target row in the order of the source columns.
    pub fn target_values<'a>(&self, row: &'a Row) -> Vec<Value<'a>> {
        self.map.iter().map(|&place| cell(row, place)).collect()
    }

    /// Returns the script that makes the target table equal to the source table, one
    /// statement per row, in one transaction. A removal stays a comment unless the user
    /// asks for it. `overriding` adds `OVERRIDING SYSTEM VALUE` to each `INSERT`, for a
    /// PostgreSQL identity that refuses a value.
    pub fn script(
        &self,
        table: &str,
        dialect: Dialect,
        removals: bool,
        overriding: bool,
    ) -> String {
        let name = |column: usize| quote_identifier(&self.columns[column], dialect);
        let condition = |values: &[Value]| {
            self.key
                .iter()
                .map(|&column| match values[column] {
                    Value::Null => format!("{} IS NULL", name(column)),
                    value => format!("{} = {}", name(column), value_literal(value, dialect)),
                })
                .collect::<Vec<_>>()
                .join(" AND ")
        };
        let mut lines = vec!["BEGIN;".to_owned()];
        for change in &self.changes {
            match change {
                RowChange::Insert(place) => {
                    let values: Vec<Value> = self.source[*place].cells().collect();
                    let columns: Vec<usize> = (0..self.columns.len())
                        .filter(|&column| !self.skipped[column])
                        .collect();
                    let names: Vec<String> = columns.iter().map(|&column| name(column)).collect();
                    let literals: Vec<String> = columns
                        .iter()
                        .map(|&column| value_literal(values[column], dialect))
                        .collect();
                    let overriding = if overriding {
                        " OVERRIDING SYSTEM VALUE"
                    } else {
                        ""
                    };
                    lines.push(format!(
                        "INSERT INTO {table} ({}){overriding} VALUES ({});",
                        names.join(", "),
                        literals.join(", ")
                    ));
                }
                RowChange::Update {
                    source, columns, ..
                } => {
                    let values: Vec<Value> = self.source[*source].cells().collect();
                    let set: Vec<String> = columns
                        .iter()
                        .map(|&column| {
                            format!(
                                "{} = {}",
                                name(column),
                                value_literal(values[column], dialect)
                            )
                        })
                        .collect();
                    lines.push(format!(
                        "UPDATE {table} SET {} WHERE {};",
                        set.join(", "),
                        condition(&values)
                    ));
                }
                RowChange::Delete(row) => {
                    let values = self.target_values(row);
                    let sql = format!("DELETE FROM {table} WHERE {};", condition(&values));
                    lines.push(if removals { sql } else { format!("-- {sql}") });
                }
            }
        }
        lines.push("COMMIT;".to_owned());
        lines.join("\n")
    }
}

fn cell(row: &Row, column: usize) -> Value<'_> {
    row.get(column).unwrap_or(Value::Null)
}

/// Returns the form of a value that a compare reads: a number in its decimal form, and
/// any other value as its bytes. `NULL` has no form.
pub(crate) fn canonical(value: Value<'_>) -> Option<Cow<'_, [u8]>> {
    match value {
        Value::Null => None,
        Value::Number(text) => Some(match decimal(text) {
            Some(form) => Cow::Owned(form.into_bytes()),
            None => Cow::Borrowed(text.as_bytes()),
        }),
        Value::Text(text) => Some(Cow::Borrowed(text.as_bytes())),
        Value::Binary(bytes) => Some(Cow::Borrowed(bytes.bytes())),
    }
}

/// Returns true when two values are equal for a compare: `NULL` equals only `NULL`, a
/// number compares by its decimal value, and other values compare by their bytes.
pub fn same(a: Value<'_>, b: Value<'_>) -> bool {
    match (canonical(a), canonical(b)) {
        (None, None) => true,
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

pub(crate) fn key_hash(row: &Row, columns: impl Iterator<Item = usize>) -> u64 {
    let mut hasher = DefaultHasher::new();
    for column in columns {
        canonical(cell(row, column)).hash(&mut hasher);
    }
    hasher.finish()
}

/// Writes a number in its plain decimal form, without extra zeros: `10.50` and `1.05e1`
/// give `10.5`. Text that is not a decimal number, such as `NaN`, returns `None`.
pub fn decimal(text: &str) -> Option<String> {
    let text = text.trim();
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (mantissa, exponent) = match rest.find(['e', 'E']) {
        Some(at) => (&rest[..at], rest[at + 1..].parse::<i64>().ok()?),
        None => (rest, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits_only = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    if (whole.is_empty() && fraction.is_empty()) || !digits_only(whole) || !digits_only(fraction) {
        return None;
    }
    let mut exponent = exponent.checked_sub(i64::try_from(fraction.len()).ok()?)?;
    let mut digits: String = format!("{whole}{fraction}")
        .trim_start_matches('0')
        .to_owned();
    while digits.ends_with('0') {
        digits.pop();
        exponent += 1;
    }
    if digits.is_empty() {
        return Some("0".to_owned());
    }
    if exponent.abs() > MAX_EXPONENT {
        return None;
    }
    let mut form = if exponent >= 0 {
        digits + &"0".repeat(exponent as usize)
    } else {
        let point = digits.len() as i64 + exponent;
        if point > 0 {
            let (left, right) = digits.split_at(point as usize);
            format!("{left}.{right}")
        } else {
            format!("0.{}{digits}", "0".repeat((-point) as usize))
        }
    };
    if negative {
        form.insert(0, '-');
    }
    Some(form)
}
