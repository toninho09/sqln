//! One console: the editor, its results, and its execution state.

use std::time::Instant;

use crossterm::event::KeyEvent;

use crate::connector::{StatementEnd, TransactionCommand, TransactionState};
use crate::editor::{KeyboardMode, MarkState, RunPlan, SqlEditor};
use crate::exec::{
    ConnectionId, ConsoleId, ExecEvent, ExecutionEnd, ExecutionId, ReadLimit, StatementKey,
    StatementStatus,
};
use crate::grid::{Budget, Row};
use crate::input::Action;
pub use crate::result::{GridAction, ResultTab};
use crate::script_file::ScriptFile;
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

/// The next step after a commit or a rollback that a dialog asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterTransaction {
    Close,
    UseAutocommit,
}

/// What the application must do after an execution event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Signal {
    ExecutionFinished { schema_changed: bool },
    ConfirmClose,
    TransactionChanged,
    TransactionFailed(String),
}

pub struct Console {
    pub id: ConsoleId,
    pub name: String,
    /// A console without a connection keeps its text, and it cannot run.
    pub connection: Option<ConnectionId>,
    /// The schema or database that each execution of this console applies.
    pub schema: Option<String>,
    pub editor: SqlEditor,
    /// The files of the console. A console without a file keeps its text in memory.
    pub file: Option<ScriptFile>,
    /// True after the user chose to close without the text that no file holds.
    pub discard_text: bool,
    pub results: Vec<ResultTab>,
    pub messages: Vec<Message>,
    /// The index of a result. The value `results.len()` selects **Messages**.
    pub active: usize,
    pub focus: Focus,
    pub phase: Phase,
    pub transaction: TransactionState,
    pub manual: bool,
    pub messages_scroll: u16,
    /// True while the close of this console waits for a step.
    pub closing: bool,
    pub pending_transaction: Option<(TransactionCommand, AfterTransaction)>,
    execution: Option<ExecutionId>,
    /// True while the last result receives rows.
    reading: bool,
    results_made: usize,
    /// True when a statement already reported the stop.
    stop_reported: bool,
    first_not_executed: Option<usize>,
    statements: Vec<String>,
    transaction_before: TransactionState,
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
    pub fn new(id: ConsoleId, name: String, editor: SqlEditor) -> Self {
        Self {
            id,
            name,
            connection: None,
            schema: None,
            editor,
            file: None,
            discard_text: false,
            results: Vec::new(),
            messages: Vec::new(),
            active: 0,
            focus: Focus::Editor,
            phase: Phase::Idle,
            transaction: TransactionState::None,
            manual: false,
            messages_scroll: 0,
            closing: false,
            pending_transaction: None,
            execution: None,
            reading: false,
            results_made: 0,
            stop_reported: false,
            first_not_executed: None,
            statements: Vec::new(),
            transaction_before: TransactionState::None,
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
                self.results.push(ResultTab::new(self.results_made, columns));
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
        self.results.get(self.active).is_some_and(ResultTab::search_has_work)
    }

    /// Continues the search of the active result until the deadline.
    pub fn continue_search(&mut self, deadline: Instant) {
        if let Some(result) = self.results.get_mut(self.active) {
            result.continue_search(deadline);
        }
    }

    /// Writes pasted text in the search bar. It returns false without a bar.
    pub fn paste_in_search(&mut self, text: &str) -> bool {
        let focused = self.focus == Focus::Results;
        focused && self.results.get_mut(self.active).is_some_and(|result| result.paste_in_search(text))
    }

    pub fn search_is_open(&self) -> bool {
        self.results.get(self.active).is_some_and(|result| result.search.is_some())
    }

    /// Applies an action of the grid region. `extend` moves the end of the selection.
    pub fn on_results_action(&mut self, action: Action, extend: bool) -> GridAction {
        match action {
            Action::GridPreviousResult => self.switch_tab(false),
            Action::GridNextResult => self.switch_tab(true),
            _ if self.showing_messages() => self.on_messages_action(action),
            _ => return self.on_grid_action(action, extend),
        }
        GridAction::None
    }

    fn on_messages_action(&mut self, action: Action) {
        match action {
            Action::GridUp => self.messages_scroll = self.messages_scroll.saturating_sub(1),
            Action::GridDown => self.messages_scroll = self.messages_scroll.saturating_add(1),
            _ => {}
        }
    }

    /// Applies a key to the search bar of the active result.
    pub fn on_search_key(&mut self, key: KeyEvent) {
        if let Some(result) = self.results.get_mut(self.active) {
            result.on_search_key(key);
        }
    }

    fn on_grid_action(&mut self, action: Action, extend: bool) -> GridAction {
        let vim = self.editor.keyboard() == KeyboardMode::Vim;
        match self.results.get_mut(self.active) {
            Some(result) => result.on_action(action, extend, vim),
            None => GridAction::None,
        }
    }
}
