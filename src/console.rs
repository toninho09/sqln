//! One console: the editor, its results, and its execution state.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::connector::{StatementEnd, TransactionState};
use crate::editor::{KeyboardMode, MarkState, RunPlan, SqlEditor};
use crate::exec::{
    ConsoleId, ExecEvent, ExecutionEnd, ExecutionId, ReadLimit, StatementKey, StatementStatus,
};
use crate::grid::{Budget, GridView, ResultTable, Row, RowSearch, Value};
use crate::ui::text;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Waiting,
    Running,
    Stopping,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    #[default]
    Editor,
    Results,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageLevel {
    Info,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub level: MessageLevel,
    pub text: String,
}

pub struct ResultTab {
    /// The number of the tab. It stays the same when the user closes another tab.
    pub number: usize,
    pub table: ResultTable,
    pub view: GridView,
    pub summary: String,
    pub search: Option<RowSearch>,
}

/// What the application must do after an execution event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Signal {
    ExecutionFinished { schema_changed: bool },
    ConfirmClose,
    TransactionChanged,
    TransactionFailed(String),
}

/// The action of a key in the results panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GridAction {
    None,
    Copy(String),
    OpenValue { title: String, text: String, footer: String },
    CloseResult,
}

pub struct Console {
    pub id: ConsoleId,
    pub editor: SqlEditor,
    pub results: Vec<ResultTab>,
    pub messages: Vec<Message>,
    /// The index of a result. The value `results.len()` selects **Messages**.
    pub active: usize,
    pub focus: Focus,
    pub phase: Phase,
    pub transaction: TransactionState,
    pub manual: bool,
    pub messages_scroll: u16,
    execution: Option<ExecutionId>,
    /// True while the last result receives rows.
    reading: bool,
    results_made: usize,
    /// True when a statement already reported the stop.
    stop_reported: bool,
    first_not_executed: Option<usize>,
    statements: Vec<String>,
    transaction_before: TransactionState,
    pending_g: bool,
}

fn read_summary(rows: usize, limit: ReadLimit) -> String {
    match limit {
        ReadLimit::None => text::rows_read(rows),
        ReadLimit::RowCap => text::row_cap_reached(rows),
        ReadLimit::Budget => text::budget_reached(rows),
        ReadLimit::ValueTooLarge => text::value_too_large(rows),
    }
}

fn first_word(sql: &str) -> &str {
    sql.split(|c: char| !c.is_ascii_alphabetic()).find(|word| !word.is_empty()).unwrap_or("")
}

impl Console {
    pub fn new(id: ConsoleId, editor: SqlEditor) -> Self {
        Self {
            id,
            editor,
            results: Vec::new(),
            messages: Vec::new(),
            active: 0,
            focus: Focus::Editor,
            phase: Phase::Idle,
            transaction: TransactionState::None,
            manual: false,
            messages_scroll: 0,
            execution: None,
            reading: false,
            results_made: 0,
            stop_reported: false,
            first_not_executed: None,
            statements: Vec::new(),
            transaction_before: TransactionState::None,
            pending_g: false,
        }
    }

    pub fn is_busy(&self) -> bool {
        self.phase != Phase::Idle
    }

    pub fn in_transaction(&self) -> bool {
        matches!(self.transaction, TransactionState::Open | TransactionState::Failed)
    }

    pub fn showing_messages(&self) -> bool {
        self.active >= self.results.len()
    }

    /// Accepts an execution. It clears the old results and frees their budget.
    pub fn begin_execution(&mut self, execution: ExecutionId, plan: &RunPlan, budget: &Budget) {
        for result in self.results.drain(..) {
            budget.release(result.table.bytes);
        }
        self.messages.clear();
        self.messages_scroll = 0;
        self.active = 0;
        self.results_made = 0;
        self.stop_reported = false;
        self.first_not_executed = None;
        self.execution = Some(execution);
        self.statements.clone_from(&plan.statements);
        self.phase = Phase::Waiting;
        self.editor.set_marks(plan);
    }

    fn push_message(&mut self, level: MessageLevel, text: String) {
        self.messages.push(Message { level, text });
    }

    fn is_current(&self, execution: ExecutionId) -> bool {
        self.execution == Some(execution)
    }

    pub fn on_event(&mut self, event: ExecEvent, budget: &Budget) -> Option<Signal> {
        match event {
            ExecEvent::Queued { .. } => {}
            ExecEvent::Started { execution, .. } if self.is_current(execution) => {
                if self.phase == Phase::Waiting {
                    self.phase = Phase::Running;
                }
            }
            ExecEvent::StatementStarted { key } if self.is_current(key.execution) => {
                self.transaction_before = self.transaction;
                self.editor.set_mark_state(key.index, MarkState::Running);
            }
            ExecEvent::Columns { key, columns } if self.is_current(key.execution) => {
                self.results_made += 1;
                self.reading = true;
                self.results.push(ResultTab {
                    number: self.results_made,
                    table: ResultTable::new(columns),
                    view: GridView::default(),
                    summary: String::new(),
                    search: None,
                });
            }
            ExecEvent::Rows { key, rows, bytes } => self.on_rows(key, rows, bytes, budget),
            ExecEvent::ResultDone { key, rows, limit } if self.is_current(key.execution) => {
                self.reading = false;
                if let Some(result) = self.results.last_mut() {
                    result.summary = read_summary(rows, limit);
                }
            }
            ExecEvent::StatementFinished { key, status, end, error_offset }
                if self.is_current(key.execution) =>
            {
                self.on_statement_finished(key.index, status, end);
                if let Some(offset) = error_offset {
                    self.editor.set_error_offset(key.index, offset);
                }
            }
            ExecEvent::ConfirmClose { execution, .. } if self.is_current(execution) => {
                return Some(Signal::ConfirmClose);
            }
            ExecEvent::ExecutionFinished { execution, end, message, .. }
                if self.is_current(execution) =>
            {
                return Some(self.on_execution_finished(end, message));
            }
            ExecEvent::TransactionChanged { state, .. } => {
                self.transaction = state;
                return Some(Signal::TransactionChanged);
            }
            ExecEvent::TransactionFailed { message, .. } => {
                self.push_message(MessageLevel::Error, text::transaction_failed(&message));
                self.active = self.results.len();
                return Some(Signal::TransactionFailed(message));
            }
            _ => {}
        }
        None
    }

    fn on_rows(&mut self, key: StatementKey, rows: Vec<Row>, bytes: usize, budget: &Budget) {
        // An old execution has no grid, so its data leaves the budget.
        match self.results.last_mut() {
            Some(result) if self.execution == Some(key.execution) => result.table.append(rows, bytes),
            _ => budget.release(bytes),
        }
    }

    fn on_statement_finished(
        &mut self,
        index: usize,
        status: StatementStatus,
        end: Option<StatementEnd>,
    ) {
        self.reading = false;
        match status {
            StatementStatus::Running => {}
            StatementStatus::Success => self.on_statement_success(index, end.unwrap_or_default()),
            StatementStatus::Error(message) => {
                self.editor.set_mark_state(index, MarkState::Error);
                self.push_message(MessageLevel::Error, text::statement_error(index, &message));
            }
            StatementStatus::NotExecuted => {
                self.editor.set_mark_state(index, MarkState::NotExecuted);
                self.first_not_executed.get_or_insert(index);
            }
            StatementStatus::Stopped => {
                self.stop_reported = true;
                self.editor.set_mark_state(index, MarkState::Stopped);
                self.push_message(MessageLevel::Info, text::statement_stopped(index));
            }
            StatementStatus::Timeout => {
                self.editor.set_mark_state(index, MarkState::Timeout);
                self.push_message(MessageLevel::Error, text::statement_timeout(index));
            }
            StatementStatus::Unknown => {
                self.editor.set_mark_state(index, MarkState::Unknown);
                self.push_message(MessageLevel::Error, text::statement_unknown(index));
            }
        }
        let word = first_word(self.statements.get(index).map_or("", String::as_str)).to_owned();
        let user_ended = ["COMMIT", "ROLLBACK", "END"].iter().any(|item| word.eq_ignore_ascii_case(item));
        let database_ended = self.transaction == TransactionState::None
            && self.transaction_before == TransactionState::Open;
        if database_ended && !user_ended {
            self.push_message(MessageLevel::Info, text::DATABASE_ENDED_TRANSACTION.to_owned());
        }
    }

    fn on_statement_success(&mut self, index: usize, end: StatementEnd) {
        let state = if end.limit == ReadLimit::None { MarkState::Success } else { MarkState::Capped };
        self.editor.set_mark_state(index, state);

        if end.has_columns {
            if end.limit != ReadLimit::None {
                let summary = read_summary(end.rows, end.limit);
                self.push_message(MessageLevel::Info, format!("Statement {}: {summary}", index + 1));
            }
            return;
        }
        let mut message = match end.changed {
            Some(rows) => text::statement_changed(index, rows),
            None => text::statement_done(index),
        };
        if self.in_transaction() && end.changed.is_some() {
            message = format!("{message} {}", text::NOT_COMMITTED);
        }
        self.push_message(MessageLevel::Info, message);
    }

    fn on_execution_finished(&mut self, end: ExecutionEnd, message: Option<String>) -> Signal {
        if end == ExecutionEnd::Refused {
            return Signal::ExecutionFinished { schema_changed: false };
        }
        if let Some(message) = message {
            self.push_message(MessageLevel::Error, message);
        }
        if end == ExecutionEnd::CanceledBeforeStart {
            self.push_message(MessageLevel::Info, text::CANCELED_BEFORE_START.to_owned());
        }
        // A stop can arrive between two statements, or a statement can end with success
        // after the stop request. **Messages** must still explain the end.
        if end == ExecutionEnd::Stopped && !self.stop_reported {
            let message = self
                .first_not_executed
                .map_or_else(|| text::STOPPED_AFTER_LAST.to_owned(), text::stopped_before);
            self.push_message(MessageLevel::Info, message);
        }
        self.phase = Phase::Idle;
        self.execution = None;
        self.reading = false;
        // A script that ends normally shows its last result. Any other end shows
        // **Messages**, because that tab explains the end.
        self.active = if end == ExecutionEnd::Completed && !self.results.is_empty() {
            self.results.len() - 1
        } else {
            self.results.len()
        };
        let schema_changed = self.statements.iter().any(|sql| {
            ["CREATE", "ALTER", "DROP"].iter().any(|word| first_word(sql).eq_ignore_ascii_case(word))
        });
        Signal::ExecutionFinished { schema_changed }
    }

    /// Moves between the results and **Messages**.
    pub fn switch_tab(&mut self, forward: bool) {
        let count = self.results.len() + 1;
        let active = self.active.min(count - 1);
        self.active = (active + if forward { 1 } else { count - 1 }) % count;
    }

    /// Closes the active result and frees its budget. It returns false while the
    /// result receives rows.
    pub fn close_result(&mut self, budget: &Budget) -> bool {
        if self.active >= self.results.len() {
            return true;
        }
        if self.reading && self.active + 1 == self.results.len() {
            return false;
        }
        let result = self.results.remove(self.active);
        budget.release(result.table.bytes);
        true
    }

    /// Returns true when the search of the active result has rows to read.
    pub fn search_has_work(&self) -> bool {
        self.results.get(self.active).is_some_and(|result| {
            result.search.as_ref().is_some_and(|search| search.has_work(&result.table))
        })
    }

    /// Continues the search of the active result until the deadline.
    pub fn continue_search(&mut self, deadline: Instant) {
        let Some(result) = self.results.get_mut(self.active) else { return };
        let Some(search) = &mut result.search else { return };
        if let Some((row, col)) = search.scan(&result.table, deadline) {
            result.view.move_to(row, col, &result.table, false);
        }
    }

    /// Writes pasted text in the search bar. It returns false without a bar.
    pub fn paste_in_search(&mut self, text: &str) -> bool {
        let search = self.results.get_mut(self.active).and_then(|result| result.search.as_mut());
        match search {
            Some(search) if self.focus == Focus::Results => {
                search.push_str(text);
                true
            }
            _ => false,
        }
    }

    pub fn search_is_open(&self) -> bool {
        self.results.get(self.active).is_some_and(|result| result.search.is_some())
    }

    pub fn on_results_key(&mut self, key: KeyEvent) -> GridAction {
        let vim = self.editor.keyboard() == KeyboardMode::Vim;
        if self.search_is_open() {
            self.on_search_key(key);
            return GridAction::None;
        }
        match key.code {
            KeyCode::Char('[') => {
                self.switch_tab(false);
                return GridAction::None;
            }
            KeyCode::Char(']') => {
                self.switch_tab(true);
                return GridAction::None;
            }
            _ => {}
        }
        if self.showing_messages() {
            self.on_messages_key(key, vim);
            return GridAction::None;
        }
        self.on_grid_key(key, vim)
    }

    fn on_messages_key(&mut self, key: KeyEvent, vim: bool) {
        match key.code {
            KeyCode::Up => self.messages_scroll = self.messages_scroll.saturating_sub(1),
            KeyCode::Char('k') if vim => self.messages_scroll = self.messages_scroll.saturating_sub(1),
            KeyCode::Down => self.messages_scroll = self.messages_scroll.saturating_add(1),
            KeyCode::Char('j') if vim => self.messages_scroll = self.messages_scroll.saturating_add(1),
            _ => {}
        }
    }

    fn on_search_key(&mut self, key: KeyEvent) {
        let Some(result) = self.results.get_mut(self.active) else { return };
        let Some(search) = &mut result.search else { return };
        let cell = match key.code {
            KeyCode::Enter | KeyCode::Down => search.advance(true),
            KeyCode::Up => search.advance(false),
            KeyCode::Backspace => {
                search.pop();
                None
            }
            KeyCode::Esc => {
                result.search = None;
                None
            }
            KeyCode::Char(c) if (key.modifiers - KeyModifiers::SHIFT).is_empty() => {
                search.push_str(c.encode_utf8(&mut [0; 4]));
                None
            }
            _ => None,
        };
        if let Some((row, col)) = cell {
            result.view.move_to(row, col, &result.table, false);
        }
    }

    fn on_grid_key(&mut self, key: KeyEvent, vim: bool) -> GridAction {
        let Some(result) = self.results.get_mut(self.active) else { return GridAction::None };
        match key.code {
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                result.search = Some(RowSearch::new((result.view.row, result.view.col)));
                return GridAction::None;
            }
            KeyCode::Char('/') if vim => {
                result.search = Some(RowSearch::new((result.view.row, result.view.col)));
                return GridAction::None;
            }
            KeyCode::Char('<') => result.table.resize(result.view.col, -1),
            KeyCode::Char('>') => result.table.resize(result.view.col, 1),
            KeyCode::Char('=') => result.table.reset_width(result.view.col),
            KeyCode::Char('x') | KeyCode::Delete => return GridAction::CloseResult,
            _ => {}
        }
        let table = &result.table;
        let view = &mut result.view;
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let extend = shift || (vim && view.anchor.is_some());
        let last_row = table.rows.len().saturating_sub(1);
        let last_col = table.columns.len().saturating_sub(1);
        let pending_g = std::mem::take(&mut self.pending_g);

        match key.code {
            KeyCode::Up if alt => view.move_to(0, view.col, table, extend),
            KeyCode::Down if alt => view.move_to(last_row, view.col, table, extend),
            KeyCode::Left if alt => view.move_to(view.row, 0, table, extend),
            KeyCode::Right if alt => view.move_to(view.row, last_col, table, extend),
            KeyCode::Up => view.move_by(-1, 0, table, extend),
            KeyCode::Down => view.move_by(1, 0, table, extend),
            KeyCode::Left => view.move_by(0, -1, table, extend),
            KeyCode::Right => view.move_by(0, 1, table, extend),
            KeyCode::PageUp => view.move_by(-20, 0, table, extend),
            KeyCode::PageDown => view.move_by(20, 0, table, extend),
            KeyCode::Char('a') if ctrl => view.select_all(table),
            KeyCode::Char('c') if ctrl => {
                return view.copy(table).map_or(GridAction::None, GridAction::Copy);
            }
            KeyCode::Enter => return Self::open_value(table, view),
            KeyCode::Esc => view.anchor = None,
            KeyCode::Char('k') if vim => view.move_by(-1, 0, table, extend),
            KeyCode::Char('j') if vim => view.move_by(1, 0, table, extend),
            KeyCode::Char('h') if vim => view.move_by(0, -1, table, extend),
            KeyCode::Char('l') if vim => view.move_by(0, 1, table, extend),
            KeyCode::Char('0') if vim => view.move_to(view.row, 0, table, extend),
            KeyCode::Char('$') if vim => view.move_to(view.row, last_col, table, extend),
            KeyCode::Char('G') if vim => view.move_to(last_row, view.col, table, extend),
            KeyCode::Char('g') if vim && pending_g => view.move_to(0, view.col, table, extend),
            KeyCode::Char('g') if vim => self.pending_g = true,
            KeyCode::Char('v') if vim => view.toggle_selection(),
            KeyCode::Char('y') if vim => {
                return view.copy(table).map_or(GridAction::None, GridAction::Copy);
            }
            _ => {}
        }
        GridAction::None
    }

    fn open_value(table: &ResultTable, view: &GridView) -> GridAction {
        let Some(value) = table.rows.get(view.row).and_then(|row| row.get(view.col)) else {
            return GridAction::None;
        };
        let column = table.columns.get(view.col).map_or("", String::as_str);
        let contents = value.full_text();
        let kind = match value {
            Value::Null => text::VALUE_NULL,
            Value::Text(_) => text::VALUE_TEXT,
            Value::Number(_) => text::VALUE_NUMBER,
            Value::Binary(_) => text::VALUE_BINARY,
        };
        GridAction::OpenValue {
            title: text::value_title(&crate::grid::printable(column), view.row),
            footer: text::value_footer(contents.chars().count(), kind),
            text: contents,
        }
    }
}
