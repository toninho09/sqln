//! A table tab: the rows of one table or view, loaded one page at a time.
//!
//! Each page is a new query, and a page keeps no read open. The tab asks for one row
//! more than a page, so it knows that a next page exists without a count.

use std::ops::Range;

use crossterm::event::{KeyCode, KeyEvent};

use crate::catalog::{Catalog, RelationRef};
use crate::complete::{quote_name, suggest};
use crate::console::{Message, MessageLevel, Phase};
use crate::editor::SuggestionList;
use crate::exec::{
    ConnectionId, ConsoleId, ExecEvent, ExecutionEnd, ExecutionId, ReadLimit, StatementStatus,
};
use crate::grid::{Budget, Row, row_cost};
use crate::line_input::{Edit, LineInput};
use crate::result::ResultTab;
use crate::sql::{Dialect, TokenKind, quote_identifier, split, tokenize};
use crate::ui::text;

/// The part of a table tab that receives the keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableFocus {
    #[default]
    Grid,
    Filter,
    Order,
}

/// One page that the application must ask the database for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageRequest {
    pub sql: String,
    /// True for the first page of a query, which replaces the loaded rows.
    pub first: bool,
    /// The cap of the sink: a page and the row that proves a next page.
    pub row_cap: usize,
}

/// The page in progress.
struct Load {
    execution: ExecutionId,
    first: bool,
    /// The rows of this page that the grid kept.
    received: usize,
    /// True when the database sent the row after the page.
    extra: bool,
    /// True when the database finished the page.
    done: bool,
}

pub struct TableTab {
    /// The place of the tab in the order of opening.
    pub opened: u64,
    /// The identity of the task of this tab. It shares the numbers of the consoles.
    pub id: ConsoleId,
    pub connection: ConnectionId,
    pub relation: RelationRef,
    pub is_view: bool,
    pub result: ResultTab,
    pub focus: TableFocus,
    pub filter: LineInput,
    pub order: LineInput,
    pub list: Option<SuggestionList>,
    pub phase: Phase,
    pub message: Option<Message>,
    /// True when a next page can exist.
    pub has_more: bool,
    applied_filter: String,
    applied_order: String,
    page_size: usize,
    load: Option<Load>,
    /// What stopped the last page: `None`, `Budget`, or `ValueTooLarge`.
    pub limit: ReadLimit,
    /// The data in use when a page stopped for lack of space. A load that finds no
    /// freed space does not ask the database.
    blocked_at: usize,
    /// True when the list waits for columns that the catalog does not hold yet.
    list_waits: bool,
}

/// Returns the relation as the `FROM` clause writes it.
fn from_clause(relation: &RelationRef, dialect: Dialect) -> String {
    format!(
        "{}.{}",
        quote_identifier(&relation.schema, dialect),
        quote_identifier(&relation.relation, dialect)
    )
}

/// Returns true when the text is one statement without an open token. A semicolon,
/// an open quote, or an open block comment would change the query that follows.
fn is_one_statement(sql: &str, dialect: Dialect) -> bool {
    let open = tokenize(sql, dialect).iter().any(|token| {
        if token.kind == TokenKind::Comment {
            sql[token.start..].starts_with("/*") && !token.closed
        } else {
            !token.closed
        }
    });
    !open && split(sql, dialect).len() == 1
}

/// Builds the query of one page. Each clause starts a line, so a line comment in
/// the filter cannot remove the next clause.
pub fn page_sql(
    dialect: Dialect,
    relation: &RelationRef,
    filter: &str,
    order: &str,
    page_size: usize,
    offset: usize,
) -> Result<String, String> {
    let mut sql = format!("SELECT * FROM {}", from_clause(relation, dialect));
    if !filter.is_empty() {
        sql.push_str("\nWHERE ");
        sql.push_str(filter);
    }
    if !order.is_empty() {
        sql.push_str("\nORDER BY ");
        sql.push_str(order);
    }
    sql.push_str(&format!("\nLIMIT {} OFFSET {offset}", page_size.saturating_add(1)));
    if is_one_statement(&sql, dialect) { Ok(sql) } else { Err(text::TABLE_TEXT_REFUSED.to_owned()) }
}

/// Compares two `ORDER BY` texts without case and without extra blanks.
fn same_order(a: &str, b: &str) -> bool {
    let words = |text: &str| text.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>();
    words(a) == words(b)
}

/// Returns the text that a click on a column header gives: ascending, then
/// descending, then no order.
pub fn clicked_order(current: &str, column: &str, dialect: Dialect) -> String {
    let name = quote_name(column, dialect, None);
    let ascending = format!("{name} ASC");
    let descending = format!("{name} DESC");
    if same_order(current, &ascending) {
        descending
    } else if same_order(current, &descending) {
        String::new()
    } else {
        ascending
    }
}

impl TableTab {
    pub fn new(
        id: ConsoleId,
        connection: ConnectionId,
        relation: RelationRef,
        is_view: bool,
        page_size: usize,
    ) -> Self {
        Self {
            opened: 0,
            id,
            connection,
            relation,
            is_view,
            result: ResultTab::new(0, Vec::new()),
            focus: TableFocus::Grid,
            filter: LineInput::default(),
            order: LineInput::default(),
            list: None,
            phase: Phase::Idle,
            message: None,
            has_more: false,
            applied_filter: String::new(),
            applied_order: String::new(),
            page_size,
            load: None,
            limit: ReadLimit::None,
            blocked_at: 0,
            list_waits: false,
        }
    }

    pub fn is_busy(&self) -> bool {
        self.phase != Phase::Idle
    }

    pub fn loaded(&self) -> usize {
        self.result.table.rows.len()
    }

    pub fn applied_order(&self) -> &str {
        &self.applied_order
    }

    pub fn applied_filter(&self) -> &str {
        &self.applied_filter
    }

    /// Returns true when the data budget stopped the last page, and no space is free.
    /// A value larger than the budget never fits, so it blocks for good.
    pub fn is_blocked(&self, budget: &Budget) -> bool {
        match self.limit {
            ReadLimit::Budget => budget.used() >= self.blocked_at,
            ReadLimit::ValueTooLarge => true,
            ReadLimit::None | ReadLimit::RowCap => false,
        }
    }

    pub fn label(&self) -> String {
        text::table_label(&self.relation.relation, self.is_view)
    }

    /// Returns true for the same relation of the same connection.
    pub fn is(&self, connection: ConnectionId, relation: &RelationRef) -> bool {
        self.connection == connection && self.relation == *relation
    }

    /// Frees the rows of the grid and their budget.
    pub fn clear_rows(&mut self, budget: &Budget) {
        budget.release(self.result.table.bytes);
        self.result = ResultTab::new(0, Vec::new());
    }

    /// Starts a query from the first page with the texts of the fields. A text that
    /// cannot make one statement leaves the grid empty, with the reason.
    pub fn apply(&mut self, dialect: Dialect, budget: &Budget) -> Option<PageRequest> {
        let filter = self.filter.text().trim().to_owned();
        let order = self.order.text().trim().to_owned();
        self.restart(filter, order, dialect, budget)
    }

    /// Clears both fields, and it starts a query without a filter and an order.
    pub fn clear(&mut self, dialect: Dialect, budget: &Budget) -> Option<PageRequest> {
        self.filter.set("");
        self.order.set("");
        self.apply(dialect, budget)
    }

    /// Sorts by a column, as a click on its header does. The applied filter stays,
    /// and a draft in the filter field stays a draft.
    pub fn sort_by(&mut self, column: usize, dialect: Dialect, budget: &Budget) -> Option<PageRequest> {
        let name = self.result.table.columns.get(column)?.clone();
        let order = clicked_order(&self.applied_order, &name, dialect);
        self.order.set(&order);
        let filter = self.applied_filter.clone();
        self.restart(filter, order, dialect, budget)
    }

    fn restart(
        &mut self,
        filter: String,
        order: String,
        dialect: Dialect,
        budget: &Budget,
    ) -> Option<PageRequest> {
        self.clear_rows(budget);
        self.applied_filter = filter;
        self.applied_order = order;
        self.message = None;
        self.has_more = false;
        self.limit = ReadLimit::None;
        self.load = None;
        self.list = None;
        let sql = page_sql(dialect, &self.relation, &self.applied_filter, &self.applied_order, self.page_size, 0);
        match sql {
            Ok(sql) => Some(PageRequest { sql, first: true, row_cap: self.page_size.saturating_add(1) }),
            Err(reason) => {
                self.message = Some(Message { level: MessageLevel::Error, text: reason });
                None
            }
        }
    }

    /// Returns the next page. The error is the reason that the page cannot start.
    pub fn next_page(&mut self, dialect: Dialect, budget: &Budget) -> Result<PageRequest, &'static str> {
        if self.is_busy() {
            return Err(text::TABLE_BUSY);
        }
        if !self.has_more {
            return Err(text::TABLE_NO_MORE);
        }
        if self.is_blocked(budget) {
            return Err(text::TABLE_BLOCKED);
        }
        let offset = self.loaded();
        let sql = page_sql(dialect, &self.relation, &self.applied_filter, &self.applied_order, self.page_size, offset)
            .map_err(|_| text::TABLE_TEXT_REFUSED)?;
        self.message = None;
        Ok(PageRequest { sql, first: false, row_cap: self.page_size.saturating_add(1) })
    }

    /// Records the execution that the manager accepted for a page.
    pub fn started(&mut self, execution: ExecutionId, first: bool) {
        self.load = Some(Load { execution, first, received: 0, extra: false, done: false });
        self.limit = ReadLimit::None;
        self.phase = Phase::Waiting;
    }

    fn is_current(&self, execution: ExecutionId) -> bool {
        self.load.as_ref().is_some_and(|load| load.execution == execution)
    }

    pub fn on_event(&mut self, event: ExecEvent, budget: &Budget) {
        match event {
            ExecEvent::Started { execution, .. } if self.is_current(execution) => {
                if self.phase == Phase::Waiting {
                    self.phase = Phase::Running;
                }
            }
            ExecEvent::Columns { key, columns } if self.is_current(key.execution) => {
                if self.load.as_ref().is_some_and(|load| load.first) {
                    self.result = ResultTab::new(0, columns);
                }
            }
            ExecEvent::Rows { key, rows, bytes } => self.on_rows(key.execution, rows, bytes, budget),
            ExecEvent::ResultDone { key, limit, .. } if self.is_current(key.execution) => {
                self.on_page_done(limit, budget);
            }
            ExecEvent::StatementFinished { key, status: StatementStatus::Error(message), .. }
                if self.is_current(key.execution) =>
            {
                self.message = Some(Message { level: MessageLevel::Error, text: message });
            }
            ExecEvent::ExecutionFinished { execution, end, message, .. }
                if self.is_current(execution) =>
            {
                self.on_finished(end, message);
            }
            _ => {}
        }
    }

    fn on_rows(&mut self, execution: ExecutionId, mut rows: Vec<Row>, mut bytes: usize, budget: &Budget) {
        // An old execution has no grid, so its data leaves the budget.
        let Some(load) = self.load.as_mut().filter(|load| load.execution == execution) else {
            budget.release(bytes);
            return;
        };
        let room = self.page_size.saturating_sub(load.received);
        if rows.len() > room {
            let extra = rows.split_off(room);
            let cost: usize = extra.iter().map(|row| row_cost(row)).sum();
            budget.release(cost);
            bytes = bytes.saturating_sub(cost);
            load.extra = true;
        }
        load.received += rows.len();
        self.result.table.append(rows, bytes);
    }

    fn on_page_done(&mut self, limit: ReadLimit, budget: &Budget) {
        let Some(load) = &mut self.load else { return };
        load.done = true;
        self.has_more = load.extra || limit != ReadLimit::None;
        self.limit = limit;
        self.blocked_at = budget.used();
    }

    fn on_finished(&mut self, end: ExecutionEnd, message: Option<String>) {
        let done = self.load.as_ref().is_some_and(|load| load.done);
        self.phase = Phase::Idle;
        self.load = None;
        // A page that ended early leaves rows that the next page continues.
        if !done && self.loaded() > 0 {
            self.has_more = true;
        }
        let info = |text: &str| Message { level: MessageLevel::Info, text: text.to_owned() };
        let error = |text: &str| Message { level: MessageLevel::Error, text: text.to_owned() };
        let note = match end {
            // The error of a statement is already in `message`.
            ExecutionEnd::Completed | ExecutionEnd::Refused => None,
            ExecutionEnd::Failed => message.map(|text| Message { level: MessageLevel::Error, text }),
            ExecutionEnd::Stopped => Some(info(text::TABLE_STOPPED)),
            ExecutionEnd::TimedOut => Some(error(text::TABLE_TIMEOUT)),
            ExecutionEnd::CanceledBeforeStart => Some(info(text::TABLE_CANCELED)),
            ExecutionEnd::ConnectionClosed => Some(error(text::TABLE_CLOSED)),
        };
        if note.is_some() {
            self.message = note;
        }
    }

    /// Returns the field that has the focus.
    pub fn focused_field(&mut self) -> Option<&mut LineInput> {
        match self.focus {
            TableFocus::Grid => None,
            TableFocus::Filter => Some(&mut self.filter),
            TableFocus::Order => Some(&mut self.order),
        }
    }

    /// Moves the focus to the other field.
    pub fn switch_field(&mut self) {
        self.list = None;
        self.focus = if self.focus == TableFocus::Filter { TableFocus::Order } else { TableFocus::Filter };
    }

    /// Handles a key for the open list. It returns false when other handlers must act.
    pub fn on_list_key(&mut self, key: KeyEvent) -> bool {
        let Some(list) = &mut self.list else { return false };
        let count = list.completion.items.len();
        let plain = key.modifiers.is_empty();
        match key.code {
            KeyCode::Tab if plain => self.accept_suggestion(),
            KeyCode::Esc => self.list = None,
            KeyCode::Up if plain => list.selected = (list.selected + count - 1) % count,
            KeyCode::Down if plain => list.selected = (list.selected + 1) % count,
            _ => return false,
        }
        true
    }

    fn accept_suggestion(&mut self) {
        let Some(list) = self.list.take() else { return };
        let Some(item) = list.completion.items.get(list.selected) else { return };
        let range = list.completion.replace.clone();
        let insert = item.insert.clone();
        if let Some(field) = self.focused_field() {
            field.replace(range, &insert);
        }
    }

    /// Applies an editing key to the focused field. It returns the relations whose
    /// columns the list needs.
    pub fn on_field_key(
        &mut self,
        key: KeyEvent,
        dialect: Dialect,
        catalog: Option<&Catalog>,
    ) -> Vec<RelationRef> {
        let Some(field) = self.focused_field() else { return Vec::new() };
        let edit = field.on_key(key);
        self.list_waits = false;
        match edit {
            Edit::Wrote => self.update_list(dialect, catalog),
            Edit::Removed if self.list.is_some() => self.update_list(dialect, catalog),
            _ => {
                self.list = None;
                Vec::new()
            }
        }
    }

    /// Opens the list at the cursor, as `Ctrl+Space` does.
    pub fn open_list(&mut self, dialect: Dialect, catalog: Option<&Catalog>) -> Vec<RelationRef> {
        self.update_list(dialect, catalog)
    }

    /// Builds the list again after the catalog received data.
    pub fn refresh_list(&mut self, dialect: Dialect, catalog: Option<&Catalog>) -> Vec<RelationRef> {
        if self.list.is_some() || self.list_waits {
            return self.update_list(dialect, catalog);
        }
        Vec::new()
    }

    /// Asks for suggestions in a query that holds the text of the field, so that the
    /// columns of the table are in scope. The offsets move back to the field.
    fn update_list(&mut self, dialect: Dialect, catalog: Option<&Catalog>) -> Vec<RelationRef> {
        let (clause, field) = match self.focus {
            TableFocus::Filter => ("WHERE", &self.filter),
            TableFocus::Order => ("ORDER BY", &self.order),
            TableFocus::Grid => return Vec::new(),
        };
        let head = format!("SELECT * FROM {}\n{clause} ", from_clause(&self.relation, dialect));
        let sql = format!("{head}{}", field.text());
        let cursor = head.len() + field.byte_cursor();
        let mut completion = suggest(&sql, cursor, dialect, catalog);
        let shift = |at: usize| at.saturating_sub(head.len());
        let range = completion.replace.clone();
        completion.replace = Range { start: shift(range.start), end: shift(range.end) };
        let missing = completion.missing_columns.clone();
        self.list_waits = !missing.is_empty();
        self.list = (!completion.items.is_empty()).then_some(SuggestionList { completion, selected: 0 });
        missing
    }

    /// Writes pasted text in the focused field. It returns false without a field.
    pub fn paste_in_field(&mut self, text: &str) -> bool {
        self.list = None;
        match self.focused_field() {
            Some(field) => {
                field.insert(text);
                true
            }
            None => false,
        }
    }
}
