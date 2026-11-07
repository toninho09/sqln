//! Pending table changes and SQL that identifies rows by their original primary key.

pub mod form;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::catalog::RelationRef;
use crate::compare::schema::{self, Table};
use crate::grid::{Budget, Reservation, ResultTable, Row, RowWriter, Value, printable, row_cost};
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

/// One pending insert row: the submitted values and the columns the user provided.
/// The grid holds the same values, and the snapshot freezes what a review shows.
#[derive(Debug)]
pub struct InsertRow {
    pub current: Arc<SavedRow>,
    pub columns: BTreeSet<usize>,
}

/// The pending state of one whole row of the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowMark {
    Insert,
    Delete,
}

/// The answer of a delete toggle: the new state of the row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteToggle {
    Marked,
    Unmarked,
    /// A pending insert row left the grid, so no statement will run for it.
    RemovedInsert,
}

#[derive(Debug, Default)]
pub struct Edits {
    pub rows: BTreeMap<usize, PendingRow>,
    /// The rows marked for delete, with the original row that the executor verifies.
    pub deletes: BTreeMap<usize, Arc<SavedRow>>,
    /// The rows appended for insert. A column without a provided value uses the
    /// default of the table.
    pub inserts: BTreeMap<usize, InsertRow>,
}

/// Returns the row with one replaced cell, and reserves it in the budget of the grid.
/// It returns the row, its snapshot, and the reserved bytes.
fn replaced_row(
    old: &Row,
    col: usize,
    value: Value<'_>,
    budget: &Arc<Budget>,
) -> Result<(Row, Arc<SavedRow>, usize), String> {
    let next: Row = old
        .cells()
        .enumerate()
        .map(|(i, old)| if i == col { value } else { old })
        .collect();
    let snapshot = SavedRow::reserve(next.clone(), budget)?;
    let Reservation::Accepted(bytes) = budget.reserve(&next) else {
        return Err(text::EDIT_BUDGET.to_owned());
    };
    Ok((next, snapshot, bytes))
}

impl Edits {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.deletes.is_empty() && self.inserts.is_empty()
    }

    pub fn cells(&self) -> usize {
        self.rows
            .values()
            .map(|row| row.columns.len())
            .sum::<usize>()
            + self
                .inserts
                .values()
                .map(|row| row.columns.len())
                .sum::<usize>()
    }

    pub fn changed(&self, row: usize, col: usize) -> bool {
        self.rows
            .get(&row)
            .is_some_and(|row| row.columns.contains(&col))
            || self
                .inserts
                .get(&row)
                .is_some_and(|row| row.columns.contains(&col))
    }

    pub fn is_insert(&self, row: usize) -> bool {
        self.inserts.contains_key(&row)
    }

    /// The mark of one whole row: a pending insert or a pending delete.
    pub fn row_mark(&self, row: usize) -> Option<RowMark> {
        if self.inserts.contains_key(&row) {
            Some(RowMark::Insert)
        } else if self.deletes.contains_key(&row) {
            Some(RowMark::Delete)
        } else {
            None
        }
    }

    /// Collects the pending changes in apply order: updates, deletes, then inserts.
    pub fn changes(&self) -> Vec<Change> {
        let mut changes: Vec<_> = self
            .rows
            .values()
            .map(|pending| {
                Change::Update(Patch {
                    original: pending.original.clone(),
                    current: pending.current.clone(),
                    columns: pending.columns.clone(),
                })
            })
            .collect();
        changes.extend(self.deletes.values().map(|original| Change::Delete {
            original: original.clone(),
        }));
        changes.extend(self.inserts.values().map(|pending| Change::Insert {
            current: pending.current.clone(),
            columns: pending.columns.clone(),
        }));
        changes
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
        if self.deletes.contains_key(&row) {
            return Err(text::EDIT_ROW_DELETED.to_owned());
        }
        let old = table.rows.get(row).ok_or(text::EDIT_NO_CELL)?;
        if old.get(col).ok_or(text::EDIT_NO_CELL)? == value {
            return Ok(());
        }
        let (next, snapshot, bytes) = replaced_row(old, col, value, budget)?;
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

    /// Replaces one value of a pending insert row. An empty text leaves the provided
    /// set, so the column uses the default of the table.
    pub fn set_insert(
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
        let (next, snapshot, bytes) = replaced_row(old, col, value, budget)?;
        let old_bytes = row_cost(old);
        table.rows[row] = next;
        table.bytes = table.bytes - old_bytes + bytes;
        budget.release(old_bytes);
        let pending = self.inserts.entry(row).or_insert_with(|| InsertRow {
            current: snapshot.clone(),
            columns: BTreeSet::new(),
        });
        pending.current = snapshot;
        if matches!(value, Value::Text("")) {
            pending.columns.remove(&col);
        } else {
            pending.columns.insert(col);
        }
        table.refresh_row_width(row);
        Ok(())
    }

    /// Appends one pending insert row of empty cells. It returns the place of the row.
    pub fn new_row(
        &mut self,
        table: &mut ResultTable,
        budget: &Arc<Budget>,
    ) -> Result<usize, String> {
        let mut cells = RowWriter::default();
        for _ in 0..table.columns.len() {
            cells.push(Value::Text(""));
        }
        let row = cells.finish();
        let bytes = row_cost(&row);
        let snapshot = SavedRow::reserve(row.clone(), budget)?;
        let index = table.rows.len();
        table.append(vec![row], bytes);
        self.inserts.insert(
            index,
            InsertRow {
                current: snapshot,
                columns: BTreeSet::new(),
            },
        );
        Ok(index)
    }

    /// Toggles the delete mark of one row. A row with cell changes returns to its
    /// original values, because the delete wins. A pending insert row leaves the grid.
    pub fn toggle_delete(
        &mut self,
        table: &mut ResultTable,
        row: usize,
        budget: &Arc<Budget>,
    ) -> Result<DeleteToggle, String> {
        if self.inserts.contains_key(&row) {
            self.remove_insert(table, row, budget);
            // The rows after the removed one take its place.
            let later: Vec<usize> = self.inserts.range(row + 1..).map(|(key, _)| *key).collect();
            for key in later {
                let value = self
                    .inserts
                    .remove(&key)
                    .expect("the key comes from the map");
                self.inserts.insert(key - 1, value);
            }
            table.invalidate();
            return Ok(DeleteToggle::RemovedInsert);
        }
        if self.deletes.remove(&row).is_some() {
            return Ok(DeleteToggle::Unmarked);
        }
        if let Some(pending) = self.rows.remove(&row) {
            if Arc::strong_count(&pending.original) != 1 {
                self.rows.insert(row, pending);
                return Err(text::TABLE_BUSY.to_owned());
            }
            let mut saved = Arc::try_unwrap(pending.original).expect("the owner count is checked");
            let bytes = std::mem::take(&mut saved.bytes);
            let current = std::mem::replace(&mut table.rows[row], std::mem::take(&mut saved.row));
            let current_bytes = row_cost(&current);
            budget.release(current_bytes);
            table.bytes = table.bytes - current_bytes + bytes;
            table.refresh_row_width(row);
        }
        let original = table.rows.get(row).ok_or(text::EDIT_NO_CELL)?.clone();
        let original = SavedRow::reserve(original, budget)?;
        self.deletes.insert(row, original);
        table.invalidate();
        Ok(DeleteToggle::Marked)
    }

    /// Removes one pending insert row from the grid and frees its cost.
    fn remove_insert(&mut self, table: &mut ResultTable, row: usize, budget: &Arc<Budget>) {
        self.inserts.remove(&row);
        if let Some(current) = table.rows.get(row).cloned() {
            let bytes = row_cost(&current);
            table.rows.remove(row);
            table.bytes -= bytes;
            budget.release(bytes);
        }
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
        self.deletes.clear();
        // The insert rows leave from the end, so the places of the others stay.
        for index in self.inserts.keys().rev().copied().collect::<Vec<_>>() {
            self.remove_insert(table, index, budget);
        }
        self.inserts.clear();
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
    /// Returns true when the columns of the grid still match the table definition.
    pub fn matches(&self, columns: &[String]) -> bool {
        columns.len() == self.table.columns.len()
            && columns
                .iter()
                .zip(&self.table.columns)
                .all(|(name, column)| name == &column.name)
    }

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

    /// Checks one column of a pending insert row. A key column stays editable,
    /// because a table can have a natural key.
    pub fn editable_insert(&self, columns: &[String], col: usize) -> Result<(), String> {
        if !self.matches(columns) {
            return Err(text::EDIT_SCHEMA_CHANGED.to_owned());
        }
        let column = self
            .table
            .column(columns.get(col).ok_or(text::EDIT_NO_CELL)?)
            .ok_or(text::EDIT_NO_CELL)?;
        if column.auto.is_some() || column.generated.is_some() {
            return Err(text::EDIT_PROTECTED.to_owned());
        }
        let ty = column.ty.to_ascii_lowercase();
        if ["blob", "binary", "bytea", "bit"]
            .iter()
            .any(|name| ty.contains(name))
        {
            return Err(text::EDIT_BINARY.to_owned());
        }
        Ok(())
    }

    /// Returns true when the type of the column holds a number. A pending insert
    /// uses this to keep the number shape of a value the user writes.
    pub fn numeric(&self, col: usize) -> bool {
        self.table.columns.get(col).is_some_and(|column| {
            let ty = column.ty.to_ascii_lowercase();
            ["int", "num", "real", "floa", "doub", "dec"]
                .iter()
                .any(|part| ty.contains(part))
        })
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
    pub fn delete_sql(&self, original: &Row) -> Result<String, String> {
        Ok(format!(
            "DELETE FROM {} WHERE {};",
            from_clause(&self.relation, self.dialect),
            self.key_where(original)?
        ))
    }

    /// Writes the `INSERT` of one pending row. The columns outside `provided` use the
    /// default of the table, so they stay out of the statement.
    pub fn insert_sql(&self, row: &Row, provided: &BTreeSet<usize>) -> Result<String, String> {
        let names: Vec<_> = self
            .table
            .columns
            .iter()
            .map(|col| col.name.clone())
            .collect();
        if names.len() != row.columns() {
            return Err(text::EDIT_SCHEMA_CHANGED.to_owned());
        }
        let mut places = Vec::new();
        let mut values = Vec::new();
        for (index, name) in names.iter().enumerate() {
            let column = self.table.column(name).ok_or(text::EDIT_NO_CELL)?;
            if !provided.contains(&index) {
                if !column.nullable
                    && column.default.is_none()
                    && column.auto.is_none()
                    && column.generated.is_none()
                {
                    return Err(text::edit_insert_needs(&printable(name)));
                }
                continue;
            }
            self.editable_insert(&names, index)?;
            match row.get(index) {
                Some(Value::Null) if !column.nullable => {
                    return Err(text::EDIT_NOT_NULL.to_owned());
                }
                Some(value) => {
                    places.push(quote_identifier(name, self.dialect));
                    values.push(value_literal(value, self.dialect));
                }
                None => return Err(text::EDIT_NO_CELL.to_owned()),
            }
        }
        if places.is_empty() {
            // Every column uses its default. MySQL wants the empty column list, and
            // the other dialects have the `DEFAULT VALUES` form.
            return Ok(if self.dialect == Dialect::MySql {
                format!(
                    "INSERT INTO {} () VALUES ();",
                    from_clause(&self.relation, self.dialect)
                )
            } else {
                format!(
                    "INSERT INTO {} DEFAULT VALUES;",
                    from_clause(&self.relation, self.dialect)
                )
            });
        }
        Ok(format!(
            "INSERT INTO {} ({}) VALUES ({});",
            from_clause(&self.relation, self.dialect),
            places.join(", "),
            values.join(", ")
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

/// One pending change of the grid, ready for a review or an apply.
#[derive(Clone, Debug)]
pub enum Change {
    /// New values for the columns of one existing row.
    Update(Patch),
    /// The delete of one existing row, identified by its original primary key.
    Delete { original: Arc<SavedRow> },
    /// One new row. `columns` are the places the user provided; the other columns use
    /// the default of the table.
    Insert {
        current: Arc<SavedRow>,
        columns: BTreeSet<usize>,
    },
}

impl Change {
    /// The original row that identifies an existing row. An insert has none.
    pub fn original(&self) -> Option<&Arc<SavedRow>> {
        match self {
            Change::Update(patch) => Some(&patch.original),
            Change::Delete { original } => Some(original),
            Change::Insert { .. } => None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Job {
    Inspect {
        relation: RelationRef,
        dialect: Dialect,
    },
    Review {
        metadata: Metadata,
        changes: Vec<Change>,
    },
    Apply {
        metadata: Metadata,
        changes: Vec<Change>,
    },
}

#[derive(Debug)]
pub enum Outcome {
    Metadata(Metadata),
    Review(String),
    Applied,
}

pub fn review(metadata: &Metadata, changes: &[Change]) -> Result<String, String> {
    let updates = changes
        .iter()
        .filter(|change| matches!(change, Change::Update(_)))
        .count();
    let deletes = changes
        .iter()
        .filter(|change| matches!(change, Change::Delete { .. }))
        .count();
    let inserts = changes
        .iter()
        .filter(|change| matches!(change, Change::Insert { .. }))
        .count();
    let mut shown = vec![text::edit_pending(updates, deletes, inserts)];
    for change in changes {
        match change {
            Change::Update(patch) => {
                shown.push(format!(
                    "\n{}\nWHERE {}",
                    text::EDIT_UPDATE_ROW,
                    metadata.key_where(&patch.original.row)?
                ));
                for &col in &patch.columns {
                    let name = metadata.table.columns.get(col).ok_or(text::EDIT_NO_CELL)?;
                    shown.push(format!(
                        "{}\n{}: {}\n{}: {}",
                        printable(&name.name),
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
            Change::Delete { original } => {
                shown.push(format!(
                    "\n{}\nWHERE {}",
                    text::EDIT_DELETE_ROW,
                    metadata.key_where(&original.row)?
                ));
                shown.push(metadata.delete_sql(&original.row)?);
            }
            Change::Insert { current, columns } => {
                shown.push(format!("\n{}", text::EDIT_INSERT_ROW));
                shown.push(metadata.insert_sql(&current.row, columns)?);
            }
        }
    }
    Ok(shown.join("\n"))
}

#[derive(Debug)]
pub struct Failure {
    pub message: String,
    pub uncertain: bool,
}
