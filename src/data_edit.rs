//! Pending table changes and SQL that identifies rows by their original primary key.

pub mod form;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::catalog::RelationRef;
use crate::compare::schema::{self, Table};
use crate::grid::{Budget, Reservation, ResultTable, Row, Value, row_cost};
use crate::sql::literal::value_literal;
use crate::sql::{Dialect, quote_identifier};
use crate::table::from_clause;
use crate::ui::text;

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
}

impl Drop for SavedRow {
    fn drop(&mut self) {
        self.budget.release(self.bytes);
    }
}

#[derive(Debug)]
pub struct PendingRow {
    pub original: Arc<SavedRow>,
    pub current: Arc<SavedRow>,
    pub columns: BTreeSet<usize>,
}

#[derive(Debug, Default)]
pub struct Edits {
    pub rows: BTreeMap<usize, PendingRow>,
}

impl Edits {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn cells(&self) -> usize {
        self.rows.values().map(|row| row.columns.len()).sum()
    }

    pub fn changed(&self, row: usize, col: usize) -> bool {
        self.rows
            .get(&row)
            .is_some_and(|row| row.columns.contains(&col))
    }

    pub fn patches(&self) -> Vec<Patch> {
        self.rows
            .values()
            .map(|pending| Patch {
                original: pending.original.clone(),
                current: pending.current.clone(),
                columns: pending.columns.clone(),
            })
            .collect()
    }

    /// Replaces one visible value, but keeps the first database row unchanged.
    pub fn set(
        &mut self,
        table: &mut ResultTable,
        row: usize,
        col: usize,
        value: Value<'_>,
        budget: &Arc<Budget>,
    ) -> Result<(), String> {
        let old = table.rows.get(row).ok_or(text::EDIT_NO_CELL)?;
        if old.get(col).ok_or(text::EDIT_NO_CELL)? == value {
            return Ok(());
        }
        let next: Row = old
            .cells()
            .enumerate()
            .map(|(i, old)| if i == col { value } else { old })
            .collect();
        let snapshot = SavedRow::reserve(next.clone(), budget)?;
        let Reservation::Accepted(bytes) = budget.reserve(&next) else {
            return Err(text::EDIT_BUDGET.to_owned());
        };
        let old_bytes = row_cost(old);
        let old = std::mem::replace(&mut table.rows[row], next);
        table.bytes = table.bytes - old_bytes + bytes;
        let pending = match self.rows.entry(row) {
            std::collections::btree_map::Entry::Vacant(entry) => entry.insert(PendingRow {
                original: Arc::new(SavedRow {
                    row: old,
                    bytes: old_bytes,
                    budget: budget.clone(),
                }),
                current: snapshot.clone(),
                columns: BTreeSet::new(),
            }),
            std::collections::btree_map::Entry::Occupied(entry) => {
                budget.release(old_bytes);
                entry.into_mut()
            }
        };
        pending.current = snapshot;
        let current = &table.rows[row];
        if pending.original.row.get(col) == current.get(col) {
            pending.columns.remove(&col);
        } else {
            pending.columns.insert(col);
        }
        if pending.columns.is_empty() {
            self.rows.remove(&row);
        }
        table.refresh_row_width(row);
        Ok(())
    }

    pub fn discard(&mut self, table: &mut ResultTable, budget: &Arc<Budget>) -> Result<(), String> {
        if self
            .rows
            .values()
            .any(|pending| Arc::strong_count(&pending.original) != 1)
        {
            return Err(text::TABLE_BUSY.to_owned());
        }
        for (index, pending) in std::mem::take(&mut self.rows) {
            let mut saved = Arc::try_unwrap(pending.original).expect("the owner count is checked");
            let bytes = std::mem::take(&mut saved.bytes);
            let old = std::mem::replace(&mut table.rows[index], std::mem::take(&mut saved.row));
            let old_bytes = row_cost(&old);
            budget.release(old_bytes);
            table.bytes = table.bytes - old_bytes + bytes;
        }
        self.rows.clear();
        table.invalidate();
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub relation: RelationRef,
    pub dialect: Dialect,
    pub table: Table,
}

impl Metadata {
    pub fn from_rows(
        relation: RelationRef,
        dialect: Dialect,
        rows: Vec<Row>,
    ) -> Result<Self, String> {
        let mut model = schema::build(dialect, &relation.schema, &[rows]);
        let table = model
            .tables
            .remove(&relation.relation)
            .ok_or(text::EDIT_NO_KEY)?;
        if table.primary.is_empty() {
            return Err(text::EDIT_NO_KEY.to_owned());
        }
        Ok(Self {
            relation,
            dialect,
            table,
        })
    }

    pub fn editable(&self, columns: &[String], row: &Row, col: usize) -> Result<(), String> {
        if columns.len() != self.table.columns.len()
            || columns
                .iter()
                .zip(&self.table.columns)
                .any(|(name, column)| *name != column.name)
        {
            return Err(text::EDIT_SCHEMA_CHANGED.to_owned());
        }
        for key in &self.table.primary {
            let index = columns
                .iter()
                .position(|name| name == key)
                .ok_or(text::EDIT_NO_KEY)?;
            if row.get(index).is_none_or(|value| value == Value::Null) {
                return Err(text::EDIT_NO_KEY.to_owned());
            }
        }
        let column = self
            .table
            .column(columns.get(col).ok_or(text::EDIT_NO_CELL)?)
            .ok_or(text::EDIT_NO_CELL)?;
        if self.table.primary.contains(&column.name)
            || column.auto.is_some()
            || column.generated.is_some()
        {
            return Err(text::EDIT_PROTECTED.to_owned());
        }
        let ty = column.ty.to_ascii_lowercase();
        if matches!(row.get(col), Some(Value::Binary(_)))
            || ["blob", "binary", "bytea", "bit"]
                .iter()
                .any(|name| ty.contains(name))
        {
            return Err(text::EDIT_BINARY.to_owned());
        }
        Ok(())
    }

    pub fn key_where(&self, row: &Row) -> Result<String, String> {
        self.table
            .primary
            .iter()
            .map(|key| {
                let index = self
                    .table
                    .columns
                    .iter()
                    .position(|col| col.name == *key)
                    .ok_or(text::EDIT_NO_KEY)?;
                let value = row
                    .get(index)
                    .filter(|value| *value != Value::Null)
                    .ok_or(text::EDIT_NO_KEY)?;
                Ok(format!(
                    "{} = {}",
                    quote_identifier(key, self.dialect),
                    value_literal(value, self.dialect)
                ))
            })
            .collect::<Result<Vec<_>, String>>()
            .map(|parts| parts.join(" AND "))
    }

    pub fn lock_sql(&self, original: &Row) -> Result<String, String> {
        let lock = if self.dialect == Dialect::Sqlite {
            ""
        } else {
            " FOR UPDATE"
        };
        Ok(format!(
            "SELECT * FROM {} WHERE {}{lock}",
            from_clause(&self.relation, self.dialect),
            self.key_where(original)?
        ))
    }

    pub fn update_sql(&self, patch: &Patch) -> Result<String, String> {
        let names: Vec<_> = self
            .table
            .columns
            .iter()
            .map(|col| col.name.clone())
            .collect();
        let assignments = patch
            .columns
            .iter()
            .map(|&index| {
                self.editable(&names, &patch.original.row, index)?;
                let value = patch.current.row.get(index).ok_or(text::EDIT_NO_CELL)?;
                if value == Value::Null && !self.table.columns[index].nullable {
                    return Err(text::EDIT_NOT_NULL.to_owned());
                }
                Ok(format!(
                    "{} = {}",
                    quote_identifier(&names[index], self.dialect),
                    value_literal(value, self.dialect)
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        if assignments.is_empty() {
            return Err(text::EDIT_NO_CHANGES.to_owned());
        }
        Ok(format!(
            "UPDATE {} SET {} WHERE {};",
            from_clause(&self.relation, self.dialect),
            assignments.join(", "),
            self.key_where(&patch.original.row)?
        ))
    }
}

/// Cursor and selection identity across the reload after a confirmed commit.
pub struct Restore {
    pub view: crate::grid::GridView,
    pub cursor: Arc<SavedRow>,
    pub anchor: Option<Arc<SavedRow>>,
    pub columns: Vec<String>,
    pub primary: Vec<String>,
}

impl Restore {
    pub fn view_in(&self, table: &ResultTable) -> Option<crate::grid::GridView> {
        if table.columns != self.columns {
            return None;
        }
        let key: Vec<_> = self
            .primary
            .iter()
            .filter_map(|name| self.columns.iter().position(|column| column == name))
            .collect();
        if key.len() != self.primary.len() {
            return None;
        }
        let find = |value: &Row| {
            table
                .rows
                .iter()
                .position(|row| key.iter().all(|&col| row.get(col) == value.get(col)))
        };
        let mut view = self.view;
        view.row = find(&self.cursor.row)?;
        view.anchor = self.view.anchor.and_then(|(_, col)| {
            self.anchor
                .as_ref()
                .and_then(|row| find(&row.row))
                .map(|row| (row, col))
        });
        Some(view)
    }
}

#[derive(Clone, Debug)]
pub struct Patch {
    pub original: Arc<SavedRow>,
    pub current: Arc<SavedRow>,
    pub columns: BTreeSet<usize>,
}

#[derive(Clone, Debug)]
pub enum Job {
    Inspect {
        relation: RelationRef,
        dialect: Dialect,
    },
    Review {
        metadata: Metadata,
        patches: Vec<Patch>,
    },
    Apply {
        metadata: Metadata,
        patches: Vec<Patch>,
    },
}

#[derive(Debug)]
pub enum Outcome {
    Metadata(Metadata),
    Review(String),
    Applied,
}

pub fn review(metadata: &Metadata, patches: &[Patch]) -> Result<String, String> {
    let mut shown = vec![text::edit_pending(
        patches.len(),
        patches.iter().map(|patch| patch.columns.len()).sum(),
    )];
    for patch in patches {
        shown.push(format!(
            "\nWHERE {}",
            metadata.key_where(&patch.original.row)?
        ));
        for &col in &patch.columns {
            let name = metadata.table.columns.get(col).ok_or(text::EDIT_NO_CELL)?;
            shown.push(format!(
                "{}\n{}: {}\n{}: {}",
                crate::grid::printable(&name.name),
                text::EDIT_BEFORE,
                value_literal(
                    patch.original.row.get(col).ok_or(text::EDIT_NO_CELL)?,
                    metadata.dialect
                ),
                text::EDIT_AFTER,
                value_literal(
                    patch.current.row.get(col).ok_or(text::EDIT_NO_CELL)?,
                    metadata.dialect
                )
            ));
        }
        shown.push(metadata.update_sql(patch)?);
    }
    Ok(shown.join("\n"))
}

#[derive(Debug)]
pub struct Failure {
    pub message: String,
    pub uncertain: bool,
}
