//! A table tab: the rows of one table or view, loaded one page at a time.
//!
//! Each page is a new query, and a page keeps no read open. The tab asks for one row
//! more than a page, so it knows that a next page exists without a count.

use std::ops::Range;

use crossterm::event::{KeyCode, KeyEvent};

use crate::catalog::{Catalog, RelationRef};
use crate::complete::{quote_name, suggest};
use crate::connector::redis::value_page;
use crate::console::{Message, MessageLevel, Phase, RunClock};
use crate::editor::SuggestionList;
use crate::exec::{
    ConnectionId, ConsoleId, ExecEvent, ExecutionEnd, ExecutionId, ReadLimit, StatementStatus,
};
use crate::grid::{Budget, Row, row_cost};
use crate::input::SuggestionKeys;
use crate::line_input::{Edit, LineInput};
use crate::result::ResultTab;
use crate::sql::{Dialect, TokenKind, quote_identifier, split, tokenize};
use crate::structure::{Scope, Section, SessionSignal, Structure, signal_sql, structure_sql};
use crate::ui::text;

/// The part of a table tab that receives the keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableFocus {
    #[default]
    Grid,
    Filter,
    Order,
}

/// What one request of a table tab reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PageKind {
    /// A page of rows.
    #[default]
    Rows,
    /// The query of the total, which reads one number and no page.
    Count,
    /// A signal to a session. It changes no state of the pages.
    Signal,
}

/// One page that the application must ask the database for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageRequest {
    pub sql: String,
    /// True for the first page of a query, which replaces the loaded rows.
    pub first: bool,
    /// The cap of the sink: a page and the row that proves a next page.
    pub row_cap: usize,
    /// The page to show when this page ends.
    pub show: Option<usize>,
    pub kind: PageKind,
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
    /// The page to show at the end of this load.
    show: Option<usize>,
    kind: PageKind,
    /// True when the database reported that the session of a signal does not exist.
    session_gone: bool,
}

pub struct TableTab {
    /// The place of the tab in the order of opening.
    pub opened: u64,
    /// The identity of the task of this tab. It shares the numbers of the consoles.
    pub id: ConsoleId,
    pub connection: ConnectionId,
    pub relation: RelationRef,
    pub is_view: bool,
    /// False for a tab without SQL text: a value of a key, or a structure.
    pub filters: bool,
    /// The catalog grid of a structure tab. `None` means a tab of rows.
    pub structure: Option<Structure>,
    pub result: ResultTab,
    pub focus: TableFocus,
    pub filter: LineInput,
    pub order: LineInput,
    pub list: Option<SuggestionList>,
    pub phase: Phase,
    pub run: RunClock,
    pub message: Option<Message>,
    /// True when a next page can exist.
    pub has_more: bool,
    /// The rows of the applied filter when the user last asked for the count.
    pub total: Option<usize>,
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
    /// True for a tab of the last session that read no rows yet, or a tab of sessions
    /// that must read them again after a signal.
    pub pending: bool,
    /// The note of a signal that the database accepted. The application shows it, and
    /// then the tab reads the sessions again.
    pub signal_note: Option<&'static str>,
}

/// Returns the relation as the `FROM` clause writes it.
pub fn from_clause(relation: &RelationRef, dialect: Dialect) -> String {
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
            sql[token.start()..].starts_with("/*") && !token.closed
        } else {
            !token.closed
        }
    });
    !open && split(sql, dialect).len() == 1
}

/// Writes the `FROM` and `WHERE` clauses after a select list.
fn from_where(dialect: Dialect, relation: &RelationRef, filter: &str) -> String {
    let mut sql = format!(" FROM {}", from_clause(relation, dialect));
    if !filter.is_empty() {
        sql.push_str("\nWHERE ");
        sql.push_str(filter);
    }
    sql
}

/// Builds the query of the rows of a filter. It returns `None` for a filter that does
/// not make one statement.
pub fn count_sql(dialect: Dialect, relation: &RelationRef, filter: &str) -> Option<String> {
    let sql = format!("SELECT COUNT(*){}", from_where(dialect, relation, filter));
    is_one_statement(&sql, dialect).then_some(sql)
}

/// Builds the query of all rows. Each clause starts a line, so a line comment in
/// the filter cannot remove the next clause.
pub fn select_sql(dialect: Dialect, relation: &RelationRef, filter: &str, order: &str) -> String {
    let mut sql = format!("SELECT *{}", from_where(dialect, relation, filter));
    if !order.is_empty() {
        sql.push_str("\nORDER BY ");
        sql.push_str(order);
    }
    sql
}

/// Writes the part of a query that reads one page and the row after it.
fn limit_clause(page_size: usize, offset: usize) -> String {
    format!("\nLIMIT {} OFFSET {offset}", page_size.saturating_add(1))
}

/// Builds the query of one page.
pub fn page_sql(
    dialect: Dialect,
    relation: &RelationRef,
    filter: &str,
    order: &str,
    page_size: usize,
    offset: usize,
) -> Result<String, String> {
    if dialect == Dialect::Redis {
        return Ok(value_page(relation, offset, page_size.saturating_add(1)));
    }
    let mut sql = select_sql(dialect, relation, filter, order);
    sql.push_str(&limit_clause(page_size, offset));
    if is_one_statement(&sql, dialect) {
        Ok(sql)
    } else {
        Err(text::TABLE_TEXT_REFUSED.to_owned())
    }
}

/// Compares two `ORDER BY` texts without case and without extra blanks.
fn same_order(a: &str, b: &str) -> bool {
    let words = |text: &str| {
        text.split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>()
    };
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
            filters: true,
            structure: None,
            result: ResultTab::new(0, Vec::new()),
            focus: TableFocus::Grid,
            filter: LineInput::default(),
            order: LineInput::default(),
            list: None,
            phase: Phase::Idle,
            run: RunClock::default(),
            message: None,
            has_more: false,
            total: None,
            applied_filter: String::new(),
            applied_order: String::new(),
            page_size,
            load: None,
            limit: ReadLimit::None,
            blocked_at: 0,
            list_waits: false,
            pending: false,
            signal_note: None,
        }
    }

    /// Returns the page of the cursor, from 0.
    pub fn page(&self) -> usize {
        self.result.view.row / self.page_size.max(1)
    }

    /// Returns the count of pages on the grid.
    pub fn loaded_pages(&self) -> usize {
        self.loaded().div_ceil(self.page_size.max(1))
    }

    /// Returns true when the grid has more than one page, or the table can have one.
    pub fn has_pages(&self) -> bool {
        self.loaded_pages() > 1 || self.has_more
    }

    /// Moves the cursor to the first row of a loaded page, at the top of the grid.
    pub fn show_page(&mut self, page: usize) {
        if page < self.loaded_pages() {
            let ResultTab { view, table, .. } = &mut self.result;
            view.move_to(page * self.page_size.max(1), view.col, table, false);
            view.top = view.row;
        }
    }

    /// Marks a tab of the last session. The first use reads its rows, so the start of
    /// the application opens no connection and asks for no password.
    pub fn wait_for_use(&mut self, note: String) {
        self.pending = true;
        self.message = Some(Message {
            level: MessageLevel::Info,
            text: note,
        });
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
        match &self.structure {
            Some(structure) => text::table_label(&structure.name, self.kind()),
            None => text::table_label(&self.relation.relation, self.kind()),
        }
    }

    /// Returns the word that names what this tab shows.
    pub fn kind(&self) -> &'static str {
        match (&self.structure, self.filters, self.is_view) {
            _ if self.shows_sessions() => "sessions",
            (Some(_), ..) => "structure",
            (None, false, _) => "key",
            (None, true, true) => "view",
            (None, true, false) => "table",
        }
    }

    /// Returns true for the rows of the same relation of the same connection.
    pub fn is(&self, connection: ConnectionId, relation: &RelationRef) -> bool {
        self.connection == connection && self.relation == *relation && self.structure.is_none()
    }

    /// Returns true for the same structure of the same connection.
    pub fn shows_structure(
        &self,
        connection: ConnectionId,
        scope: Scope,
        relation: &RelationRef,
    ) -> bool {
        self.connection == connection
            && self.relation == *relation
            && self
                .structure
                .as_ref()
                .is_some_and(|item| item.scope == scope)
    }

    /// Returns true for the tab of the sessions of a server.
    pub fn shows_sessions(&self) -> bool {
        self.structure
            .as_ref()
            .is_some_and(|item| item.scope == Scope::Sessions)
    }

    /// Returns the section on screen, for a tab with sections.
    pub fn section(&self) -> Option<Section> {
        let structure = self.structure.as_ref()?;
        structure.has_sections().then_some(structure.section)
    }

    /// Changes the section. The next query of the tab reads it.
    pub fn set_section(&mut self, section: Section) {
        if let Some(structure) = &mut self.structure {
            structure.section = section;
        }
    }

    /// Returns the query of the whole grid, without a page.
    pub fn read_sql(&self, dialect: Dialect) -> Option<String> {
        match &self.structure {
            Some(item) => structure_sql(dialect, item.scope, &self.relation, item.section),
            None => Some(select_sql(
                dialect,
                &self.relation,
                &self.applied_filter,
                &self.applied_order,
            )),
        }
    }

    /// Builds the query of one page of this tab.
    fn page_sql(&self, dialect: Dialect, offset: usize) -> Result<String, String> {
        match &self.structure {
            Some(_) => self
                .read_sql(dialect)
                .map(|sql| sql + &limit_clause(self.page_size, offset))
                .ok_or_else(|| text::STRUCTURE_REFUSED.to_owned()),
            None => page_sql(
                dialect,
                &self.relation,
                &self.applied_filter,
                &self.applied_order,
                self.page_size,
                offset,
            ),
        }
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
        self.total = None;
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
    pub fn sort_by(
        &mut self,
        column: usize,
        dialect: Dialect,
        budget: &Budget,
    ) -> Option<PageRequest> {
        if !self.filters {
            return None;
        }
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
        self.pending = false;
        self.applied_filter = filter;
        self.applied_order = order;
        self.message = None;
        self.has_more = false;
        self.limit = ReadLimit::None;
        self.load = None;
        self.list = None;
        match self.page_sql(dialect, 0) {
            Ok(sql) => Some(PageRequest {
                sql,
                first: true,
                row_cap: self.page_size.saturating_add(1),
                show: None,
                kind: PageKind::Rows,
            }),
            Err(reason) => {
                self.message = Some(Message {
                    level: MessageLevel::Error,
                    text: reason,
                });
                None
            }
        }
    }

    /// Returns the next page. The error is the reason that the page cannot start.
    pub fn next_page(
        &mut self,
        dialect: Dialect,
        budget: &Budget,
    ) -> Result<PageRequest, &'static str> {
        if self.is_busy() {
            return Err(text::TABLE_BUSY);
        }
        if !self.has_more {
            return Err(text::TABLE_NO_MORE);
        }
        if self.is_blocked(budget) {
            return Err(text::TABLE_BLOCKED);
        }
        let sql = self
            .page_sql(dialect, self.loaded())
            .map_err(|_| text::TABLE_TEXT_REFUSED)?;
        self.message = None;
        Ok(PageRequest {
            sql,
            first: false,
            row_cap: self.page_size.saturating_add(1),
            show: None,
            kind: PageKind::Rows,
        })
    }

    /// Returns the query of the total of the applied filter.
    pub fn count_request(&self, dialect: Dialect) -> Option<PageRequest> {
        Some(PageRequest {
            sql: count_sql(dialect, &self.relation, &self.applied_filter)?,
            first: false,
            row_cap: 1,
            show: None,
            kind: PageKind::Count,
        })
    }

    /// Returns the statement of a signal to a session.
    pub fn signal_request(
        dialect: Dialect,
        signal: SessionSignal,
        session: u64,
    ) -> Option<PageRequest> {
        Some(PageRequest {
            sql: signal_sql(dialect, signal, session)?,
            first: false,
            row_cap: 1,
            show: None,
            kind: PageKind::Signal,
        })
    }

    /// Returns true while the tab reads its total.
    pub fn is_counting(&self) -> bool {
        self.load
            .as_ref()
            .is_some_and(|load| load.kind == PageKind::Count)
    }

    /// Records the execution that the manager accepted for a page.
    pub fn started(&mut self, execution: ExecutionId, first: bool, show: Option<usize>) {
        self.limit = ReadLimit::None;
        self.begin(execution, first, show, PageKind::Rows);
    }

    /// Records the execution that the manager accepted for a request without a page.
    pub fn started_side_request(&mut self, execution: ExecutionId, kind: PageKind) {
        self.begin(execution, false, None, kind);
    }

    fn begin(&mut self, execution: ExecutionId, first: bool, show: Option<usize>, kind: PageKind) {
        self.load = Some(Load {
            execution,
            first,
            received: 0,
            extra: false,
            done: false,
            show,
            kind,
            session_gone: false,
        });
        self.phase = Phase::Waiting;
        self.run.start();
    }

    fn is_current(&self, execution: ExecutionId) -> bool {
        self.load
            .as_ref()
            .is_some_and(|load| load.execution == execution)
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
            ExecEvent::Rows { key, rows, bytes } => {
                self.on_rows(key.execution, rows, bytes, budget)
            }
            ExecEvent::ResultDone { key, limit, .. } if self.is_current(key.execution) => {
                self.on_page_done(limit, budget);
            }
            ExecEvent::StatementFinished {
                key,
                status: StatementStatus::Error(message),
                ..
            } if self.is_current(key.execution) => {
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
            } if self.is_current(execution) => {
                self.on_finished(end, message);
            }
            _ => {}
        }
    }

    fn on_rows(
        &mut self,
        execution: ExecutionId,
        mut rows: Vec<Row>,
        mut bytes: usize,
        budget: &Budget,
    ) {
        // An old execution has no grid, so its data leaves the budget.
        let Some(load) = self
            .load
            .as_mut()
            .filter(|load| load.execution == execution)
        else {
            budget.release(bytes);
            return;
        };
        let first_value = rows.first().and_then(|row| row.get(0));
        match load.kind {
            PageKind::Rows => {}
            PageKind::Count => {
                self.total = first_value.and_then(|value| value.full_text().parse().ok());
                budget.release(bytes);
                return;
            }
            PageKind::Signal => {
                // PostgreSQL answers `f` for a session that does not exist.
                load.session_gone = first_value.is_some_and(|value| value.full_text() == "f");
                budget.release(bytes);
                return;
            }
        }
        let room = self.page_size.saturating_sub(load.received);
        if rows.len() > room {
            let extra = rows.split_off(room);
            let cost: usize = extra.iter().map(row_cost).sum();
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
        if load.kind != PageKind::Rows {
            return;
        }
        self.has_more = load.extra || limit != ReadLimit::None;
        self.limit = limit;
        self.blocked_at = budget.used();
    }

    fn on_finished(&mut self, end: ExecutionEnd, message: Option<String>) {
        let load = self.load.take();
        let done = load.as_ref().is_some_and(|load| load.done);
        let kind = load.as_ref().map_or(PageKind::Rows, |load| load.kind);
        let page = kind == PageKind::Rows;
        if kind == PageKind::Signal && end == ExecutionEnd::Completed {
            let gone = load.as_ref().is_some_and(|load| load.session_gone);
            self.signal_note = Some(if gone {
                text::SESSION_GONE
            } else {
                text::SESSION_SIGNAL_SENT
            });
            self.pending = true;
        }
        self.phase = Phase::Idle;
        self.run.finish(end);
        self.result.cut_short |= page && end != ExecutionEnd::Completed;
        // A page that ended early leaves rows that the next page continues.
        if page && !done && self.loaded() > 0 {
            self.has_more = true;
        }
        // A page that did not end leaves the cursor in its place.
        if let Some(page) = load
            .and_then(|load| load.show)
            .filter(|_| end == ExecutionEnd::Completed)
        {
            self.show_page(page);
        }
        let info = |text: &str| Message {
            level: MessageLevel::Info,
            text: text.to_owned(),
        };
        let error = |text: &str| Message {
            level: MessageLevel::Error,
            text: text.to_owned(),
        };
        let note = match end {
            // The error of a statement is already in `message`.
            ExecutionEnd::Completed | ExecutionEnd::Refused => None,
            ExecutionEnd::Failed => message.map(|text| Message {
                level: MessageLevel::Error,
                text,
            }),
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
        self.focus = if self.focus == TableFocus::Filter {
            TableFocus::Order
        } else {
            TableFocus::Filter
        };
    }

    /// Handles a key for the open list. It returns false when other handlers must act.
    pub fn on_list_key(&mut self, key: KeyEvent, keys: SuggestionKeys) -> bool {
        let Some(list) = &mut self.list else {
            return false;
        };
        if keys.accepts(key) {
            self.accept_suggestion();
            return true;
        }
        let count = list.completion.items.len();
        let plain = key.modifiers.is_empty();
        match key.code {
            KeyCode::Esc => self.list = None,
            KeyCode::Up if plain => list.selected = (list.selected + count - 1) % count,
            KeyCode::Down if plain => list.selected = (list.selected + 1) % count,
            _ => return false,
        }
        true
    }

    fn accept_suggestion(&mut self) {
        let Some(list) = self.list.take() else { return };
        let Some(item) = list.completion.items.get(list.selected) else {
            return;
        };
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
        let Some(field) = self.focused_field() else {
            return Vec::new();
        };
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
    pub fn refresh_list(
        &mut self,
        dialect: Dialect,
        catalog: Option<&Catalog>,
    ) -> Vec<RelationRef> {
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
        let head = format!(
            "SELECT * FROM {}\n{clause} ",
            from_clause(&self.relation, dialect)
        );
        let sql = format!("{head}{}", field.text());
        let cursor = head.len() + field.byte_cursor();
        let mut completion = suggest(&sql, cursor, dialect, catalog);
        let shift = |at: usize| at.saturating_sub(head.len());
        let range = completion.replace.clone();
        completion.replace = Range {
            start: shift(range.start),
            end: shift(range.end),
        };
        let missing = completion.missing_columns.clone();
        self.list_waits = !missing.is_empty();
        self.list = (!completion.items.is_empty()).then_some(SuggestionList {
            completion,
            selected: 0,
        });
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
