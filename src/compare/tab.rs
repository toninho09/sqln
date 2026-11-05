//! A compare tab: the reads of the two sides, the differences, their grid, and the
//! script. Each side has its own task, so each read has the pool, the password, and the
//! stop of its connection.
//!
//! A structure compare reads both sides at once. A data compare reads in three steps: the
//! columns of both tables, then each row of the source, and then the rows of the target,
//! which meet the source rows as they arrive.

use super::data::{DataCompare, RowChange, plan};
use super::ddl::script;
use super::schema::{
    Change, ConstraintKind, Difference, Object, SchemaModel, Table, build, diff, summary,
};
use super::sql::{compare_sql, rows_sql, table_columns_sql};
use super::{Kind, Side};
use crate::catalog::RelationRef;
use crate::console::{Message, MessageLevel, Phase, RunClock};
use crate::exec::{ConsoleId, ExecEvent, ExecutionEnd, ExecutionId, ReadLimit, StatementStatus};
use crate::grid::{Budget, Reservation, Row, Value};
use crate::result::ResultTab;
use crate::sql::Dialect;
use crate::table::from_clause;
use crate::ui::text;

/// The read of one side.
pub struct SideRead {
    /// The identity of the task of this side. It shares the numbers of the consoles.
    pub id: ConsoleId,
    pub side: Side,
    pub dialect: Dialect,
    /// The name of the connection, for the title and the script.
    pub name: String,
    execution: Option<ExecutionId>,
    results: Vec<Vec<Row>>,
    end: Option<ExecutionEnd>,
}

impl SideRead {
    pub fn new(id: ConsoleId, side: Side, dialect: Dialect, name: String) -> Self {
        Self {
            id,
            side,
            dialect,
            name,
            execution: None,
            results: Vec::new(),
            end: None,
        }
    }

    /// Returns the name of the side for the title and the script.
    pub fn place(&self) -> String {
        let mut place = format!("{} · {}", self.name, self.side.schema);
        if let Some(table) = &self.side.table {
            place.push('.');
            place.push_str(table);
        }
        place
    }

    fn relation(&self) -> RelationRef {
        RelationRef {
            schema: self.side.schema.clone(),
            relation: self.side.table.clone().unwrap_or_default(),
        }
    }
}

/// The step of a data compare. A structure compare has only the first step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Columns,
    SourceRows,
    TargetRows,
}

pub struct CompareTab {
    /// True for a tab created by an assistant session.
    pub agent: bool,
    /// The place of the tab in the order of opening.
    pub opened: u64,
    pub kind: Kind,
    pub sides: [SideRead; 2],
    pub removals: bool,
    pub phase: Phase,
    pub run: RunClock,
    pub message: Option<Message>,
    pub result: ResultTab,
    /// The differences of a structure compare.
    pub differences: Vec<Difference>,
    /// The count of differences of the last compare.
    pub count: usize,
    /// The sync script. It is empty without differences and between two dialects.
    pub script: String,
    /// True after a compare that read both sides to the end.
    pub done: bool,
    step: Step,
    /// The side that the application reads next, in a data compare.
    next: Option<usize>,
    data: Option<DataCompare>,
    /// The key of the source table, in a data compare.
    key: Vec<String>,
    /// The columns that the target makes itself, so the script leaves them out.
    generated: Vec<String>,
    /// True when an `INSERT` into the target needs `OVERRIDING SYSTEM VALUE`.
    overriding: bool,
    /// The budget of the rows that the tab holds during the read.
    bytes: usize,
    /// What stopped a read at a limit, so the models or the rows can miss data.
    limit: ReadLimit,
}

impl CompareTab {
    pub fn new(opened: u64, kind: Kind, sides: [SideRead; 2], removals: bool) -> Self {
        Self {
            agent: false,
            opened,
            kind,
            sides,
            removals,
            phase: Phase::Idle,
            run: RunClock::default(),
            message: None,
            result: ResultTab::new(1, Vec::new()),
            differences: Vec::new(),
            count: 0,
            script: String::new(),
            done: false,
            step: Step::Columns,
            next: None,
            data: None,
            key: Vec::new(),
            generated: Vec::new(),
            overriding: false,
            bytes: 0,
            limit: ReadLimit::None,
        }
    }

    /// Returns true when the tab compares these two sides in this way.
    pub fn is(&self, kind: Kind, sides: &[Side; 2]) -> bool {
        self.kind == kind && self.sides[0].side == sides[0] && self.sides[1].side == sides[1]
    }

    /// Returns true when a task of the tab has this identity.
    pub fn owns(&self, id: ConsoleId) -> bool {
        self.sides.iter().any(|side| side.id == id)
    }

    pub fn label(&self) -> String {
        let side = &self.sides[0].side;
        let name = side.table.as_deref().unwrap_or(&side.schema);
        let label = match self.kind {
            Kind::Structure => text::compare_label(name),
            Kind::Data => text::data_compare_label(name),
        };
        if self.agent {
            format!("Agent: {label}")
        } else {
            label
        }
    }

    pub fn title(&self) -> String {
        let (source, target) = (self.sides[0].place(), self.sides[1].place());
        match self.kind {
            Kind::Structure => text::compare_title(&source, &target),
            Kind::Data => text::data_compare_title(&source, &target),
        }
    }

    pub fn is_busy(&self) -> bool {
        self.phase != Phase::Idle
    }

    pub fn same_dialect(&self) -> bool {
        self.sides[0].dialect == self.sides[1].dialect
    }

    /// Returns the queries of the first read of a side. Redis has no tables.
    pub fn statements(&self, side: usize) -> Option<Vec<String>> {
        let read = &self.sides[side];
        match self.kind {
            Kind::Structure => compare_sql(read.dialect, &read.side.schema),
            Kind::Data => {
                let table = read.side.table.as_deref()?;
                table_columns_sql(read.dialect, &read.side.schema, table).map(|sql| vec![sql])
            }
        }
    }

    /// Returns the side and the query of the next step of a data compare, once.
    pub fn take_next(&mut self) -> Option<(usize, Vec<String>)> {
        let side = self.next.take()?;
        let read = &self.sides[side];
        let table = read.side.table.as_deref()?;
        Some((side, vec![rows_sql(read.dialect, &read.side.schema, table)]))
    }

    /// Starts a new read of both sides. The grid of the last compare stays until the end.
    pub fn begin(&mut self, budget: &Budget) {
        self.release_rows(budget);
        for side in &mut self.sides {
            side.execution = None;
            side.end = None;
        }
        self.step = Step::Columns;
        self.next = None;
        self.phase = Phase::Waiting;
        self.message = None;
        self.limit = ReadLimit::None;
        self.run.start();
    }

    /// Records the execution that the manager accepted for the read of a side.
    pub fn started(&mut self, id: ConsoleId, execution: ExecutionId) {
        if let Some(side) = self.sides.iter_mut().find(|side| side.id == id) {
            side.execution = Some(execution);
            side.end = None;
        }
    }

    /// Ends the read of a side that never started, as a refused password does.
    pub fn canceled(&mut self, id: ConsoleId, budget: &Budget) {
        let Some(side) = self.sides.iter().position(|side| side.id == id) else {
            return;
        };
        if self.sides[side].end.is_none() && self.is_busy() {
            self.side_end(side, ExecutionEnd::CanceledBeforeStart, budget);
        }
    }

    fn side_of(&self, execution: ExecutionId) -> Option<usize> {
        self.sides
            .iter()
            .position(|side| side.execution == Some(execution))
    }

    pub fn on_event(&mut self, event: ExecEvent, budget: &Budget) {
        match event {
            ExecEvent::Started { execution, .. } if self.side_of(execution).is_some() => {
                if self.phase == Phase::Waiting {
                    self.phase = Phase::Running;
                }
            }
            ExecEvent::Columns { key, columns } if self.side_of(key.execution).is_some() => {
                self.on_columns(columns);
            }
            ExecEvent::Rows { key, rows, bytes } => {
                // An old execution has no place, so its data leaves the budget.
                let Some(side) = self.side_of(key.execution) else {
                    budget.release(bytes);
                    return;
                };
                self.on_rows(side, key.index, rows, bytes, budget);
            }
            ExecEvent::ResultDone { key, limit, .. } if self.side_of(key.execution).is_some() => {
                if limit != ReadLimit::None {
                    self.limit = limit;
                }
            }
            ExecEvent::StatementFinished {
                key,
                status: StatementStatus::Error(message),
                ..
            } if self.side_of(key.execution).is_some() => {
                self.message = Some(Message {
                    level: MessageLevel::Error,
                    text: message,
                });
            }
            ExecEvent::ExecutionFinished {
                execution,
                end,
                message,
                ..
            } => {
                let Some(side) = self.side_of(execution) else {
                    return;
                };
                if let Some(text) = message.filter(|_| end == ExecutionEnd::Failed) {
                    self.message.get_or_insert(Message {
                        level: MessageLevel::Error,
                        text,
                    });
                }
                self.side_end(side, end, budget);
            }
            _ => {}
        }
    }

    /// The columns of a read of rows give the places of the key, and on the target side,
    /// the place of each source column.
    fn on_columns(&mut self, columns: Vec<String>) {
        match self.step {
            Step::Columns => {}
            Step::SourceRows => {
                self.data = DataCompare::new(columns, &self.key);
                if self.data.is_none() {
                    self.fail(text::COMPARE_NO_KEY.to_owned());
                }
            }
            Step::TargetRows => {
                let Some(data) = &mut self.data else { return };
                if let Err(column) = data.set_target_columns(&columns, &self.generated) {
                    let reason = text::compare_missing_column(&column);
                    self.data = None;
                    self.fail(reason);
                }
            }
        }
    }

    fn on_rows(
        &mut self,
        side: usize,
        index: usize,
        rows: Vec<Row>,
        bytes: usize,
        budget: &Budget,
    ) {
        match (self.step, &mut self.data) {
            (Step::Columns, _) => {
                self.bytes += bytes;
                let results = &mut self.sides[side].results;
                if results.len() <= index {
                    results.resize_with(index + 1, Vec::new);
                }
                results[index].extend(rows);
            }
            (Step::SourceRows, Some(data)) => {
                self.bytes += bytes;
                data.add_source(rows);
            }
            (Step::TargetRows, Some(data)) => self.bytes += data.add_target(rows, budget),
            // A failed step keeps no rows.
            (_, None) => budget.release(bytes),
        }
    }

    /// Keeps the first reason of a failure. The read goes on to its end, and the end shows
    /// the reason.
    fn fail(&mut self, reason: String) {
        self.message.get_or_insert(Message {
            level: MessageLevel::Error,
            text: reason,
        });
    }

    /// Records the end of the read of a side, and goes to the next step when the step has
    /// its two ends, or its one end in a data compare.
    fn side_end(&mut self, side: usize, end: ExecutionEnd, budget: &Budget) {
        self.sides[side].end = Some(end);
        self.sides[side].execution = None;
        if self.step == Step::Columns && self.sides.iter().any(|side| side.end.is_none()) {
            return;
        }
        let ends = [self.sides[0].end, self.sides[1].end];
        let failed = ends
            .into_iter()
            .flatten()
            .find(|end| *end != ExecutionEnd::Completed);
        if let Some(end) = failed {
            self.stop_with(end, budget);
            return;
        }
        if self.limit != ReadLimit::None {
            let reason = if self.kind == Kind::Data && self.limit == ReadLimit::Budget {
                text::COMPARE_DATA_BUDGET
            } else {
                text::COMPARE_LIMITED
            };
            self.message = Some(Message {
                level: MessageLevel::Error,
                text: reason.to_owned(),
            });
            self.stop_with(ExecutionEnd::Completed, budget);
            return;
        }
        if self.message.is_some() {
            // A failure of a step already has its reason.
            self.stop_with(ExecutionEnd::Completed, budget);
            return;
        }
        let results = self
            .sides
            .each_mut()
            .map(|side| std::mem::take(&mut side.results));
        match (self.kind, self.step) {
            (Kind::Structure, _) => {
                self.show_structure(&results, budget);
                self.stop_with(ExecutionEnd::Completed, budget);
            }
            (Kind::Data, Step::Columns) => match self.check_tables(&results) {
                Ok(()) => {
                    self.release_rows(budget);
                    self.step = Step::SourceRows;
                    self.wait_for(0);
                }
                Err(reason) => {
                    self.fail(reason);
                    self.stop_with(ExecutionEnd::Completed, budget);
                }
            },
            (Kind::Data, Step::SourceRows) => {
                if self.data.is_none() {
                    self.fail(text::COMPARE_NO_KEY.to_owned());
                    self.stop_with(ExecutionEnd::Completed, budget);
                } else {
                    self.step = Step::TargetRows;
                    self.wait_for(1);
                }
            }
            (Kind::Data, Step::TargetRows) => {
                self.show_data(budget);
                self.stop_with(ExecutionEnd::Completed, budget);
            }
        }
    }

    /// Asks the application for the next read of a side. The side has no end until that
    /// read ends, so a canceled password request also ends the compare.
    fn wait_for(&mut self, side: usize) {
        self.sides[side].end = None;
        self.next = Some(side);
    }

    /// Ends the compare: the phase, the clock, the note of the end, and the rows of the
    /// read leave the budget.
    fn stop_with(&mut self, end: ExecutionEnd, budget: &Budget) {
        self.phase = Phase::Idle;
        self.next = None;
        self.run.finish(end);
        let note = |level, text: &str| {
            Some(Message {
                level,
                text: text.to_owned(),
            })
        };
        let note = match end {
            // The error of a statement is already in `message`.
            ExecutionEnd::Completed | ExecutionEnd::Refused | ExecutionEnd::Failed => None,
            ExecutionEnd::Stopped => note(MessageLevel::Info, text::COMPARE_STOPPED),
            ExecutionEnd::TimedOut => note(MessageLevel::Error, text::TABLE_TIMEOUT),
            ExecutionEnd::CanceledBeforeStart => note(MessageLevel::Info, text::TABLE_CANCELED),
            ExecutionEnd::ConnectionClosed => note(MessageLevel::Error, text::TABLE_CLOSED),
        };
        if note.is_some() {
            self.message = note;
        }
        self.release_rows(budget);
    }

    /// Checks the two tables of a data compare, and keeps the key and the columns of the
    /// target that the script leaves out.
    fn check_tables(&mut self, results: &[Vec<Vec<Row>>; 2]) -> Result<(), String> {
        let tables: Vec<Option<Table>> = self
            .sides
            .iter()
            .zip(results)
            .map(|(read, rows)| {
                let model = build(read.dialect, &read.side.schema, rows);
                let table = read.side.table.as_deref().unwrap_or_default();
                model.tables.get(table).cloned()
            })
            .collect();
        let [Some(source), Some(target)] = [&tables[0], &tables[1]] else {
            let (index, _) = tables
                .iter()
                .enumerate()
                .find(|(_, table)| table.is_none())
                .unwrap_or((0, &None));
            let read = &self.sides[index];
            let table = read.side.table.as_deref().unwrap_or_default();
            return Err(if index == 0 {
                text::compare_no_table(table)
            } else {
                text::compare_no_target_table(table)
            });
        };
        let plan = plan(source, target, self.sides[1].dialect)?;
        self.key = plan.key;
        self.generated = plan.skipped;
        self.overriding = plan.overriding;
        Ok(())
    }

    /// Builds the two models, compares them, and shows the differences.
    fn show_structure(&mut self, results: &[Vec<Vec<Row>>; 2], budget: &Budget) {
        let [source, target] = &self.sides;
        let mut from = build(source.dialect, &source.side.schema, &results[0]);
        let mut to = build(target.dialect, &target.side.schema, &results[1]);
        if let (Some(table), Some(other)) = (&source.side.table, &target.side.table) {
            if !from.tables.contains_key(table) {
                self.fail(text::compare_no_table(table));
                return;
            }
            // The source table takes the name of the target table, so the script acts on it.
            keep_table(&mut from, table, other);
            keep_table(&mut to, other, other);
        }
        let differences = diff(&from, &to);
        let rows: Vec<Row> = differences.iter().map(difference_row).collect();
        let columns = text::COMPARE_COLUMNS.map(str::to_owned).to_vec();
        if !self.show_rows(columns, rows, differences.len(), budget) {
            return;
        }
        self.script = if differences.is_empty() || !self.same_dialect() {
            String::new()
        } else {
            let body = script(&from, &to, &differences, self.removals);
            let header = text::compare_script_header(
                &self.sides[0].place(),
                &self.sides[1].place(),
                differences.len(),
            );
            format!("{header}\n{body}\n")
        };
        self.differences = differences;
    }

    /// Ends a data compare: each source row without a match is new, and the grid and the
    /// script show the changes.
    fn show_data(&mut self, budget: &Budget) {
        let Some(mut data) = self.data.take() else {
            return;
        };
        data.finish();
        let mut columns = vec![text::COMPARE_CHANGE.to_owned()];
        columns.extend(data.columns.iter().cloned());
        let mut rows = Vec::new();
        for change in &data.changes {
            match change {
                RowChange::Insert(place) => {
                    let values = data.source[*place].cells();
                    rows.push(data_row(text::COMPARE_INSERT, values));
                }
                RowChange::Update {
                    source,
                    target,
                    columns: changed,
                } => {
                    let names: Vec<&str> = changed
                        .iter()
                        .map(|&column| data.columns[column].as_str())
                        .collect();
                    let label = text::compare_update(&names.join(", "));
                    rows.push(data_row(&label, data.source[*source].cells()));
                    let values = data.target_values(target);
                    rows.push(data_row(text::COMPARE_TARGET_ROW, values.into_iter()));
                }
                RowChange::Delete(row) => {
                    let values = data.target_values(row);
                    rows.push(data_row(text::COMPARE_DELETE, values.into_iter()));
                }
            }
        }
        let count = data.changes.len();
        if !self.show_rows(columns, rows, count, budget) {
            return;
        }
        let target = &self.sides[1];
        self.script = if count == 0 {
            String::new()
        } else {
            let table = from_clause(&target.relation(), target.dialect);
            let body = data.script(&table, target.dialect, self.removals, self.overriding);
            let header = text::data_compare_header(&self.sides[0].place(), &target.place(), count);
            format!("{header}\n{body}\n")
        };
        self.differences = Vec::new();
    }

    /// Replaces the grid with the rows of the differences. The rows take a place in the
    /// budget first, so a full budget keeps the old grid and gives the reason.
    fn show_rows(
        &mut self,
        columns: Vec<String>,
        rows: Vec<Row>,
        count: usize,
        budget: &Budget,
    ) -> bool {
        let mut bytes = 0;
        for row in &rows {
            match budget.reserve(row) {
                Reservation::Accepted(cost) => bytes += cost,
                Reservation::BudgetReached | Reservation::ValueTooLarge => {
                    budget.release(bytes);
                    self.fail(text::COMPARE_BUDGET.to_owned());
                    return false;
                }
            }
        }
        budget.release(self.result.table.bytes);
        let mut result = ResultTab::new(1, columns);
        result.table.append(rows, bytes);
        result.summary = text::compare_summary(count);
        self.result = result;
        self.count = count;
        self.done = true;
        if count == 0 {
            self.message = Some(Message {
                level: MessageLevel::Info,
                text: text::COMPARE_EQUAL.to_owned(),
            });
        }
        true
    }

    /// Returns the rows of a read in progress and the grid to the budget.
    pub fn release(&mut self, budget: &Budget) {
        self.release_rows(budget);
        budget.release(std::mem::take(&mut self.result.table.bytes));
    }

    fn release_rows(&mut self, budget: &Budget) {
        for side in &mut self.sides {
            side.results.clear();
        }
        self.data = None;
        budget.release(std::mem::take(&mut self.bytes));
    }
}

fn value(text: Option<&str>) -> Value<'_> {
    text.map_or(Value::Null, Value::Text)
}

/// Writes one row of the grid of a data compare: the change, and the values in the order
/// of the source columns.
fn data_row<'a>(change: &'a str, values: impl Iterator<Item = Value<'a>>) -> Row {
    std::iter::once(Value::Text(change)).chain(values).collect()
}

/// Keeps only one table of a model, with a new name.
fn keep_table(model: &mut SchemaModel, table: &str, name: &str) {
    let kept = model.tables.remove(table);
    model.tables.clear();
    if let Some(mut kept) = kept {
        kept.name = name.to_owned();
        model.tables.insert(name.to_owned(), kept);
    }
}

/// Writes one row of the grid: the change, the object, the table, the name, and the
/// short text of each side.
fn difference_row(difference: &Difference) -> Row {
    let change = match difference.change {
        Change::Create => text::COMPARE_CREATE,
        Change::Drop => text::COMPARE_DROP,
        Change::Alter => text::COMPARE_ALTER,
    };
    let object = difference.object();
    let kind = match object {
        Object::Table(_) => text::COMPARE_TABLE,
        Object::Column(_) => text::COMPARE_COLUMN,
        Object::PrimaryKey { .. } => text::COMPARE_PRIMARY_KEY,
        Object::Constraint(constraint) => match constraint.kind {
            ConstraintKind::Unique => text::COMPARE_UNIQUE,
            ConstraintKind::Foreign => text::COMPARE_FOREIGN_KEY,
            ConstraintKind::Check => text::COMPARE_CHECK,
        },
        Object::Index(_) => text::COMPARE_INDEX,
    };
    let side = |object: &Option<Object>| object.as_ref().map(summary);
    let (source, target) = (side(&difference.source), side(&difference.target));
    [
        Value::Text(change),
        Value::Text(kind),
        Value::Text(&difference.table),
        Value::Text(object.name()),
        value(source.as_deref()),
        value(target.as_deref()),
    ]
    .into_iter()
    .collect()
}
