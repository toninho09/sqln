//! One console: the editor, its results, and its execution state.

use std::time::{Duration, Instant};

use crossterm::event::KeyEvent;

use crate::command_log::{CommandState, History};
use crate::connector::{StatementEnd, TransactionCommand, TransactionState};
use crate::editor::{KeyboardMode, MarkState, RunPlan, SqlEditor};
use crate::exec::{
    ConnectionId, ConsoleId, ExecEvent, ExecutionEnd, ExecutionId, ProgressSnapshot, ReadEnd,
    ReadLimit, ReadMeter, StatementKey, StatementStatus,
};
use crate::grid::{Budget, Row};
use crate::input::Action;
use crate::plan::{Explain, plan_view};
pub use crate::result::{GridAction, ResultSource, ResultTab};
use crate::script_file::ScriptFile;
use crate::ui::text;

pub use crate::message::{Message, MessageLevel};

/// How a run ended, for the mark of its tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunEnd {
    Done,
    Failed,
    Stopped,
}

impl RunEnd {
    /// Returns the end of a run. A refused execution did not start, so it has no end.
    fn of(end: ExecutionEnd) -> Option<Self> {
        match end {
            ExecutionEnd::Completed => Some(Self::Done),
            ExecutionEnd::Failed | ExecutionEnd::TimedOut | ExecutionEnd::ConnectionClosed => {
                Some(Self::Failed)
            }
            ExecutionEnd::Stopped | ExecutionEnd::CanceledBeforeStart => Some(Self::Stopped),
            ExecutionEnd::Refused => None,
        }
    }
}

/// The auto-refresh of one tab: the interval and the next moment it fires.
#[derive(Clone, Copy, Debug)]
pub struct AutoRefresh {
    pub every: Duration,
    pub next: Instant,
}

impl AutoRefresh {
    /// Arms the next repetition from `now`.
    pub fn new(every: Duration, now: Instant) -> Self {
        Self {
            every,
            next: now + every,
        }
    }

    /// True when the moment of the next repetition arrived.
    pub fn due(&self, now: Instant) -> bool {
        now >= self.next
    }

    /// Moves the next repetition after a fire.
    pub fn spent(&mut self, now: Instant) {
        self.next = now + self.every;
    }
}

/// The time and the end of the runs of one tab.
#[derive(Clone, Copy, Debug, Default)]
pub struct RunClock {
    started: Option<Instant>,
    /// The time of the last run.
    pub last: Option<Duration>,
    /// The end of the last run, until the user leaves the tab.
    pub end: Option<RunEnd>,
}

impl RunClock {
    /// Clears the last run. The time starts with the execution, so the wait for
    /// credentials does not count.
    pub fn wait(&mut self) {
        *self = Self::default();
    }

    pub fn start(&mut self) {
        *self = Self {
            started: Some(Instant::now()),
            ..Self::default()
        };
    }

    pub fn finish(&mut self, end: ExecutionEnd) {
        let Some(end) = RunEnd::of(end) else { return };
        self.end = Some(end);
        self.last = self.started.take().map(|started| started.elapsed());
    }

    /// Returns the time of the run in progress.
    pub fn elapsed(&self, now: Instant) -> Option<Duration> {
        self.started
            .map(|started| now.saturating_duration_since(started))
    }

    /// Returns true when a run of `limit` or more ended without a stop request. The user
    /// who asked for the stop is at the keyboard, so that run needs no bell.
    pub fn rings_after(&self, limit: Duration) -> bool {
        matches!(self.end, Some(RunEnd::Done | RunEnd::Failed))
            && self.last.is_some_and(|last| last >= limit)
    }

    /// The user saw the end of the run.
    pub fn seen(&mut self) {
        self.end = None;
    }
}

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
    Parameters,
    Results,
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
    /// The schema of the last execution. A new read of its statements needs this schema.
    ran_schema: Option<String>,
    pub editor: SqlEditor,
    pub parameters: crate::parameters::Panel,
    /// Immutable bindings of the last execution, for a full export reread.
    pub ran_parameters: Vec<crate::sql::parameters::Parameter>,
    /// The files of the console. A console without a file keeps its text in memory.
    pub file: Option<ScriptFile>,
    /// The external `.sql` file that a linked console edits. The console writes it
    /// only on an explicit save, so it never enters the store.
    pub external: Option<crate::script_file::ExternalFile>,
    /// True after the user chose to close without the text that no file holds.
    pub discard_text: bool,
    /// The place of the tab in the order of opening.
    pub opened: u64,
    /// True for the empty console that stays after the last tab of a console closes.
    /// The application needs one console, and the tab row does not show this one.
    pub hidden: bool,
    /// The results in the order of their numbers. A run replaces only the tabs of the
    /// statements that it runs.
    pub results: Vec<ResultTab>,
    pub messages: History,
    /// The index of a result. The value `results.len()` selects **Messages**.
    pub active: usize,
    pub focus: Focus,
    pub phase: Phase,
    pub run: RunClock,
    pub transaction: TransactionState,
    /// The moment that the application first saw the open transaction.
    transaction_since: Option<Instant>,
    pub manual: bool,
    /// The warnings in `messages`. Each frame needs this number.
    warnings: usize,
    pub messages_scroll: usize,
    message_key: Option<StatementKey>,
    /// True while the close of this console waits for a step.
    pub closing: bool,
    pub pending_transaction: Option<(TransactionCommand, AfterTransaction)>,
    execution: Option<ExecutionId>,
    /// True while the result of `filling` receives rows.
    reading: bool,
    /// The place of the tab that receives the rows of the current execution.
    filling: Option<(usize, bool, usize)>,
    /// The place of the last tab that the current execution made.
    last_made: Option<(usize, bool, usize)>,
    /// The grids of each statement of the current execution.
    grids: Vec<usize>,
    /// The place in the editor text of each statement of the current execution.
    ordinals: Vec<usize>,
    /// True when a statement already reported the stop.
    stop_reported: bool,
    /// The auto-refresh of this console, armed by the user.
    pub auto_refresh: Option<AutoRefresh>,
    /// The statements of the last run, which an auto-refresh repeats.
    pub auto_snapshot: Option<Vec<String>>,
    /// The places in the editor text of the statements of `auto_snapshot`, so a
    /// repetition replaces the same tabs.
    pub auto_ordinals: Vec<usize>,
    /// The execution of the last auto-refresh, so a failure turns it off.
    pub auto_execution: Option<ExecutionId>,
    /// The plan that the current execution shows, in place of its statement.
    pub explain: Option<Explain>,
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
    sql.split(|c: char| !c.is_ascii_alphabetic())
        .find(|word| !word.is_empty())
        .unwrap_or("")
}

impl Console {
    pub fn new(id: ConsoleId, name: String, editor: SqlEditor) -> Self {
        Self {
            id,
            name,
            connection: None,
            schema: None,
            ran_schema: None,
            editor,
            parameters: crate::parameters::Panel::default(),
            ran_parameters: Vec::new(),
            file: None,
            // The external `.sql` file that a linked console edits.
            external: None,
            discard_text: false,
            opened: 0,
            hidden: false,
            results: Vec::new(),
            messages: History::default(),
            active: 0,
            focus: Focus::Editor,
            phase: Phase::Idle,
            run: RunClock::default(),
            transaction: TransactionState::None,
            transaction_since: None,
            manual: false,
            warnings: 0,
            messages_scroll: 0,
            message_key: None,
            closing: false,
            pending_transaction: None,
            execution: None,
            reading: false,
            filling: None,
            last_made: None,
            grids: Vec::new(),
            ordinals: Vec::new(),
            stop_reported: false,
            auto_refresh: None,
            auto_snapshot: None,
            auto_ordinals: Vec::new(),
            auto_execution: None,
            explain: None,
            first_not_executed: None,
            statements: Vec::new(),
            transaction_before: TransactionState::None,
        }
    }

    pub fn is_busy(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// Measures the speed of the read that runs, and writes its summary.
    pub fn sample_progress(&mut self, now: Instant) {
        if self.reading
            && let Some(result) = self.filling_mut()
        {
            result.sample_progress(now);
        }
    }

    pub fn in_transaction(&self) -> bool {
        matches!(
            self.transaction,
            TransactionState::Open | TransactionState::Failed
        )
    }

    /// Returns true when the console has a connection or a file. Without both, the
    /// console has nothing to run.
    pub fn can_run(&self) -> bool {
        self.connection.is_some() || self.file.is_some()
    }

    /// Sets the state of the transaction. The clock of an open transaction keeps its
    /// start when the transaction only changes from open to failed.
    pub fn set_transaction(&mut self, state: TransactionState) {
        self.transaction = state;
        let since = self.transaction_since.take();
        self.transaction_since = self
            .in_transaction()
            .then(|| since.unwrap_or_else(Instant::now));
    }

    /// Returns the time since the application first saw the open transaction.
    pub fn transaction_age(&self, now: Instant) -> Option<Duration> {
        self.transaction_since
            .map(|since| now.saturating_duration_since(since))
    }

    /// Returns the statement of a result, for a new read of its rows. A plan has none.
    pub fn statement_of<'a>(&self, result: &'a ResultTab) -> Option<&'a str> {
        (!result.explained && !result.source.sql.is_empty()).then_some(result.source.sql.as_str())
    }

    /// Returns the text of a statement of the current execution.
    pub fn statement_at(&self, key: &StatementKey) -> Option<&str> {
        if !self.is_current(key.execution) {
            return None;
        }
        self.statements.get(key.index).map(String::as_str)
    }

    /// Returns the original statement of the plan on screen, not the current editor text.
    pub fn plan_statement(&self) -> Option<&str> {
        let result = self.results.get(self.active).filter(|result| result.plan)?;
        Some(result.source.sql.as_str())
    }

    /// Returns the result that receives the rows of the current execution.
    fn filling_mut(&mut self) -> Option<&mut ResultTab> {
        let place = self.filling?;
        let index = self.index_of(place)?;
        self.results.get_mut(index)
    }

    /// Returns the index of the tab at a place of the row.
    fn index_of(&self, place: (usize, bool, usize)) -> Option<usize> {
        let index = self
            .results
            .partition_point(|result| result.place() < place);
        self.results
            .get(index)
            .is_some_and(|result| result.place() == place)
            .then_some(index)
    }

    /// Removes the tabs of one statement number, and keeps the selected tab.
    fn remove_group(&mut self, number: usize, explained: bool, budget: &Budget) {
        let mut index = 0;
        while index < self.results.len() {
            let result = &self.results[index];
            if result.number == number && result.explained == explained {
                let result = self.results.remove(index);
                budget.release(result.table.bytes);
                if index < self.active {
                    self.active -= 1;
                }
            } else {
                index += 1;
            }
        }
        self.active = self.active.min(self.results.len());
    }

    /// Puts a tab at its place in the row, and keeps the selected tab.
    fn insert_result(&mut self, result: ResultTab) -> usize {
        let place = result.place();
        let index = self.results.partition_point(|other| other.place() < place);
        self.results.insert(index, result);
        if index <= self.active {
            self.active += 1;
        }
        index
    }

    /// Returns the bytes of the tabs that a run of `plan` replaces.
    fn replaced_bytes(&self, plan: &RunPlan, explained: bool) -> usize {
        let numbers: std::collections::HashSet<usize> = (0..plan.statements.len())
            .map(|index| plan.ordinal(index) + 1)
            .collect();
        self.results
            .iter()
            .filter(|result| result.explained == explained && numbers.contains(&result.number))
            .map(|result| result.table.bytes)
            .sum()
    }

    /// Returns true when the data budget has no room for a run of `plan`. The tabs that
    /// the run replaces leave the budget, so their bytes count as free.
    pub fn run_blocked(&self, plan: &RunPlan, explained: bool, budget: &Budget) -> bool {
        let limit = budget.limit();
        let free = limit
            .saturating_sub(budget.used())
            .saturating_add(self.replaced_bytes(plan, explained));
        free < (limit / 100).max(1)
    }

    pub fn showing_messages(&self) -> bool {
        self.active >= self.results.len()
    }

    /// Returns true when the result on screen shows the tree of a query plan.
    pub fn shows_plan(&self) -> bool {
        self.results
            .get(self.active)
            .is_some_and(|result| result.plan)
    }

    /// Accepts an execution. Optional retention keeps one complete result; other old results leave the budget.
    pub fn begin_execution(&mut self, execution: ExecutionId, plan: &RunPlan, budget: &Budget) {
        self.prepare_execution(Some(execution), plan, budget);
        self.run.start();
    }

    /// Accepts a request that waits for credentials, without assigning a database execution.
    pub fn await_credentials(&mut self, plan: &RunPlan, budget: &Budget) {
        self.prepare_execution(None, plan, budget);
        self.run.wait();
    }

    fn prepare_execution(
        &mut self,
        execution: Option<ExecutionId>,
        plan: &RunPlan,
        budget: &Budget,
    ) {
        // The tabs of other statements stay. Each statement replaces its own tab when
        // it ends or makes its first grid.
        let _ = budget;
        self.reset_message_context();
        self.filling = None;
        self.last_made = None;
        self.reading = false;
        self.grids = vec![0; plan.statements.len()];
        self.ordinals = (0..plan.statements.len())
            .map(|index| plan.ordinal(index))
            .collect();
        self.stop_reported = false;
        self.first_not_executed = None;
        self.execution = execution;
        self.statements.clone_from(&plan.statements);
        self.ran_schema.clone_from(&self.schema);
        self.phase = Phase::Waiting;
        self.editor.set_marks(plan);
    }

    /// Ends the results of the last connection. The text stays.
    pub fn clear_results(&mut self, budget: &Budget) {
        self.ran_parameters.clear();
        for result in self.results.drain(..) {
            budget.release(result.table.bytes);
        }
        self.reset_message_context();
        self.active = 0;
        self.filling = None;
        self.last_made = None;
        self.editor.clear_marks();
    }

    /// Returns the number of warnings in Messages.
    pub fn warnings(&self) -> usize {
        self.warnings
    }

    fn push_message(&mut self, level: MessageLevel, text: String) {
        self.warnings += usize::from(level == MessageLevel::Warning);
        self.messages.note(level, &text, self.message_key);
    }

    fn push_statement_message(&mut self, level: MessageLevel, text: String, state: CommandState) {
        self.messages
            .summarize(level, &text, self.message_key, state);
    }

    fn reset_message_context(&mut self) {
        self.warnings = 0;
        self.message_key = None;
    }

    /// Returns true while a continuous statement grows its result.
    pub fn is_live(&self) -> bool {
        self.phase != Phase::Idle
            && self
                .results
                .last()
                .is_some_and(|result| result.live.is_some())
    }

    fn is_current(&self, execution: ExecutionId) -> bool {
        self.execution == Some(execution)
    }

    pub fn on_event(&mut self, event: ExecEvent, budget: &Budget) -> Option<Signal> {
        match event {
            ExecEvent::Queued { .. } => {}
            ExecEvent::Started { execution, .. }
                if self.is_current(execution) && self.phase == Phase::Waiting =>
            {
                self.phase = Phase::Running;
            }
            ExecEvent::StatementStarted { key } if self.is_current(key.execution) => {
                self.message_key = Some(key);
                self.transaction_before = self.transaction;
                self.editor.set_mark_state(key.index, MarkState::Running);
            }
            ExecEvent::Columns {
                key,
                columns,
                live,
                progress,
            } if self.is_current(key.execution) => {
                self.on_columns(key, columns, live, progress.map(ReadMeter::new), budget);
            }
            ExecEvent::Rows { key, rows, bytes } => self.on_rows(key, rows, bytes, budget),
            ExecEvent::ResultDone { key, rows, limit } if self.is_current(key.execution) => {
                self.reading = false;
                if let Some(result) = self.filling_mut() {
                    let progress = result.progress.take().map(|meter| meter.snapshot());
                    result.summary = match (result.live, progress) {
                        (Some(_), None) => {
                            text::live_rows_done(result.table.rows.len(), result.dropped)
                        }
                        (Some(_), Some(progress)) => format!(
                            "{} {}",
                            text::live_rows_done(result.table.rows.len(), result.dropped),
                            // A continuous read always ends with a stop, so it gives no
                            // reason.
                            text::kafka_read_end(&ProgressSnapshot {
                                end: ReadEnd::None,
                                ..progress
                            })
                        ),
                        (None, None) => read_summary(rows, limit),
                        (None, Some(progress)) => {
                            let rows = match limit {
                                ReadLimit::None => text::rows_read_sentence(rows),
                                _ => read_summary(rows, limit),
                            };
                            format!("{rows} {}", text::kafka_read_end(&progress))
                        }
                    };
                    result.limit = limit;
                    // The list can hold room for twice its rows, and no more rows arrive.
                    result.table.rows.shrink_to_fit();
                }
                self.show_plan(limit, budget);
            }
            ExecEvent::Notice { key, text } if self.is_current(key.execution) => {
                self.message_key = Some(key);
                let message = text::statement_error(key.index, &text);
                self.push_message(MessageLevel::Warning, message);
            }
            ExecEvent::LimitAdded { key, rows } if self.is_current(key.execution) => {
                self.message_key = Some(key);
                let message = text::statement_error(key.index, &text::limit_added(rows));
                self.push_message(MessageLevel::Info, message);
            }
            ExecEvent::StatementFinished {
                key,
                status,
                end,
                error_offset,
            } if self.is_current(key.execution) => {
                self.message_key = Some(key);
                self.on_statement_finished(key.index, status, end, budget);
                if let Some(offset) = error_offset {
                    let prefix = self.explain.map_or(0, |explain| explain.prefix_len());
                    self.editor
                        .set_error_offset(key.index, offset.saturating_sub(prefix));
                }
            }
            ExecEvent::ConfirmClose { execution, .. } if self.is_current(execution) => {
                return Some(Signal::ConfirmClose);
            }
            ExecEvent::ExecutionFinished {
                execution,
                end,
                message,
                ..
            } if self.is_current(execution) => {
                self.message_key = None;
                return Some(self.on_execution_finished(end, message, budget));
            }
            ExecEvent::TransactionChanged { state, .. } => {
                self.set_transaction(state);
                return Some(Signal::TransactionChanged);
            }
            ExecEvent::TransactionFailed { message, state, .. } => {
                self.set_transaction(state);
                self.message_key = None;
                self.push_message(MessageLevel::Error, text::transaction_failed(&message));
                self.active = self.results.len();
                return Some(Signal::TransactionFailed(message));
            }
            _ => {}
        }
        None
    }

    /// Shows the rows of a plan as a tree. The text of the database stays after a read
    /// that stopped at a limit, a text that SQLN cannot read, or a full budget.
    fn show_plan(&mut self, limit: ReadLimit, budget: &Budget) {
        let Some(explain) = self.explain else {
            return;
        };
        let Some(result) = self.filling_mut() else {
            return;
        };
        let view = plan_view(explain.dialect, &result.table).filter(|_| limit == ReadLimit::None);
        if !view.is_some_and(|view| result.show_plan(view, budget)) {
            self.push_message(MessageLevel::Warning, text::PLAN_UNREADABLE.to_owned());
        }
    }

    fn on_rows(&mut self, key: StatementKey, rows: Vec<Row>, bytes: usize, budget: &Budget) {
        // An old execution has no grid, so its data leaves the budget.
        let current = self.execution == Some(key.execution);
        match self.filling_mut() {
            Some(result) if current => match result.live {
                Some(cap) => result.append_live(rows, cap, budget),
                None => result.table.append(rows, bytes),
            },
            // A live batch reserves its rows in the grid, not in the sink. A console
            // accepts one execution at a time, so this branch holds no live batch.
            _ => budget.release(bytes),
        }
    }

    /// Makes the tab of a new grid. The first grid of a statement replaces the tabs that
    /// an earlier run of the same statement number left.
    fn on_columns(
        &mut self,
        key: StatementKey,
        columns: Vec<String>,
        live: Option<usize>,
        progress: Option<ReadMeter>,
        budget: &Budget,
    ) {
        self.reading = true;
        let number = self.ordinals.get(key.index).copied().unwrap_or(key.index) + 1;
        let explained = self.explain.is_some();
        let made = self.grids.get(key.index).copied().unwrap_or(0);
        if made == 0 {
            self.remove_group(number, explained, budget);
        }
        if let Some(count) = self.grids.get_mut(key.index) {
            *count += 1;
        }
        let mut result = ResultTab::new(number, columns);
        result.part = made + 1;
        result.explained = explained;
        result.statement = key.index;
        result.source = ResultSource {
            sql: self.statements.get(key.index).cloned().unwrap_or_default(),
            schema: self.ran_schema.clone(),
            parameters: self.ran_parameters.clone(),
            alone: self.statements.len() == 1,
        };
        result.live = live;
        result.progress = progress;
        result.show_progress();
        let place = result.place();
        let first = self.last_made.is_none();
        let index = self.insert_result(result);
        // The first grid of a run comes on screen, as the grid of a new run did before.
        if first {
            self.active = index;
        }
        self.filling = Some(place);
        self.last_made = Some(place);
    }

    fn on_statement_finished(
        &mut self,
        index: usize,
        status: StatementStatus,
        end: Option<StatementEnd>,
        budget: &Budget,
    ) {
        self.reading = false;
        self.filling = None;
        // A statement that ran and gave no grid leaves no tab of an earlier run, because
        // that tab would no longer describe this statement.
        let ran = !matches!(
            status,
            StatementStatus::Running | StatementStatus::NotExecuted
        );
        if ran && self.grids.get(index) == Some(&0) {
            let number = self.ordinals.get(index).copied().unwrap_or(index) + 1;
            self.remove_group(number, self.explain.is_some(), budget);
        }
        match status {
            StatementStatus::Running => {}
            StatementStatus::Success => self.on_statement_success(index, end.unwrap_or_default()),
            StatementStatus::Error(message) => {
                self.editor.set_mark_state(index, MarkState::Error);
                self.push_statement_message(
                    MessageLevel::Error,
                    text::statement_error(index, &message),
                    CommandState::Failed,
                );
            }
            StatementStatus::NotExecuted => {
                self.editor.set_mark_state(index, MarkState::NotExecuted);
                self.first_not_executed.get_or_insert(index);
            }
            StatementStatus::Stopped => {
                self.stop_reported = true;
                self.editor.set_mark_state(index, MarkState::Stopped);
                self.push_statement_message(
                    MessageLevel::Info,
                    text::statement_stopped(index),
                    CommandState::Stopped,
                );
            }
            StatementStatus::Timeout => {
                self.editor.set_mark_state(index, MarkState::Timeout);
                self.push_statement_message(
                    MessageLevel::Error,
                    text::statement_timeout(index),
                    CommandState::TimedOut,
                );
            }
            StatementStatus::Unknown => {
                self.editor.set_mark_state(index, MarkState::Unknown);
                self.push_statement_message(
                    MessageLevel::Error,
                    text::statement_unknown(index),
                    CommandState::Unknown,
                );
            }
        }
        let word = first_word(self.statements.get(index).map_or("", String::as_str)).to_owned();
        let user_ended = ["COMMIT", "ROLLBACK", "END"]
            .iter()
            .any(|item| word.eq_ignore_ascii_case(item));
        let database_ended = self.transaction == TransactionState::None
            && self.transaction_before == TransactionState::Open;
        if database_ended && !user_ended {
            self.push_message(
                MessageLevel::Info,
                text::DATABASE_ENDED_TRANSACTION.to_owned(),
            );
        }
    }

    fn on_statement_success(&mut self, index: usize, end: StatementEnd) {
        let state = if end.limit == ReadLimit::None {
            MarkState::Success
        } else {
            MarkState::Capped
        };
        self.editor.set_mark_state(index, state);

        if end.has_columns {
            let summary = read_summary(end.rows, end.limit);
            self.push_statement_message(
                MessageLevel::Info,
                format!("Statement {}: {summary}", index + 1),
                CommandState::Completed,
            );
            return;
        }
        let mut message = match end.changed {
            Some(rows) => text::statement_changed(index, rows),
            None => text::statement_done(index),
        };
        if self.in_transaction() && end.changed.is_some() {
            message = format!("{message} {}", text::NOT_COMMITTED);
        }
        self.push_statement_message(MessageLevel::Info, message, CommandState::Completed);
    }

    fn on_execution_finished(
        &mut self,
        end: ExecutionEnd,
        message: Option<String>,
        budget: &Budget,
    ) -> Signal {
        let _ = budget;
        let last = self.last_made.and_then(|place| self.index_of(place));
        if end == ExecutionEnd::Refused {
            // A refused execution started no statement. The console must not wait for
            // an end that already came, so its state returns to idle.
            if let Some(message) = message {
                self.push_message(MessageLevel::Error, message);
            }
            self.phase = Phase::Idle;
            self.execution = None;
            self.reading = false;
            self.run.wait();
            return Signal::ExecutionFinished {
                schema_changed: false,
            };
        }
        self.run.finish(end);
        // A stopped statement can report a success, so only the end of the execution
        // tells that the last result can miss rows.
        if let Some(result) = last.and_then(|index| self.results.get_mut(index)) {
            result.cut_short = end != ExecutionEnd::Completed;
        }
        if let Some(message) = message {
            self.push_message(MessageLevel::Error, message);
        }
        if end == ExecutionEnd::CanceledBeforeStart {
            self.push_message(MessageLevel::Info, text::CANCELED_BEFORE_START.to_owned());
        }
        // A stop can arrive between two statements, or a statement can end with success
        // after the stop request. **Messages** must still explain the end.
        // A stop is the normal end of a live result, so its grid stays on screen.
        let live_stopped = end == ExecutionEnd::Stopped
            && last
                .and_then(|index| self.results.get(index))
                .is_some_and(|result| result.live.is_some());
        if live_stopped {
            self.push_message(MessageLevel::Info, text::LIVE_STOPPED.to_owned());
        } else if end == ExecutionEnd::Stopped && !self.stop_reported {
            let message = self
                .first_not_executed
                .map_or_else(|| text::STOPPED_AFTER_LAST.to_owned(), text::stopped_before);
            self.push_message(MessageLevel::Info, message);
        }
        self.phase = Phase::Idle;
        self.execution = None;
        self.reading = false;
        self.filling = None;
        // A script that ends normally shows the last result of this run. Any other end,
        // or a run without a grid, shows **Messages**, because that tab explains the end.
        let shows_result = end == ExecutionEnd::Completed || live_stopped;
        self.active = match last {
            Some(index) if shows_result => index,
            _ => self.results.len(),
        };
        let schema_changed = self.statements.iter().any(|sql| {
            ["CREATE", "ALTER", "DROP"]
                .iter()
                .any(|word| first_word(sql).eq_ignore_ascii_case(word))
        });
        Signal::ExecutionFinished { schema_changed }
    }

    /// Moves between the results and **Messages**.
    pub fn switch_tab(&mut self, forward: bool) {
        let count = self.results.len() + 1;
        let active = self.active.min(count - 1);
        self.active = (active + if forward { 1 } else { count - 1 }) % count;
    }

    pub fn can_close_result(&self) -> bool {
        self.active < self.results.len()
            && !(self.reading
                && self.filling.is_some()
                && self.filling == self.results.get(self.active).map(ResultTab::place))
    }

    /// Closes the active result and frees its budget. It returns false while the
    /// result receives rows.
    pub fn close_result(&mut self, budget: &Budget) -> bool {
        if self.active >= self.results.len() {
            return true;
        }
        if !self.can_close_result() {
            return false;
        }
        let result = self.results.remove(self.active);
        budget.release(result.table.bytes);
        true
    }

    /// Returns true when the search of the active result has rows to read.
    pub fn search_has_work(&self) -> bool {
        self.results
            .get(self.active)
            .is_some_and(ResultTab::search_has_work)
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
        focused
            && self
                .results
                .get_mut(self.active)
                .is_some_and(|result| result.paste_in_search(text))
    }

    pub fn search_is_open(&self) -> bool {
        self.results
            .get(self.active)
            .is_some_and(|result| result.search.is_some())
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

    pub fn scroll_messages(&mut self, rows: isize) {
        self.messages.scroll_by(&mut self.messages_scroll, rows);
    }

    fn on_messages_action(&mut self, action: Action) {
        match action {
            Action::GridUp => self.scroll_messages(-1),
            Action::GridDown => self.scroll_messages(1),
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
