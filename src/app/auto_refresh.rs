//! The auto-refresh of the tab on screen: a console repeats its last run, and a table
//! tab reads its page again.

use std::time::{Duration, Instant};

use super::App;
use crate::app::DialogAction;
use crate::console::AutoRefresh;
use crate::editor::RunPlan;
use crate::exec::{ExecEvent, ExecutionEnd, ExecutionId, RunOptions};
use crate::sql::{Dialect, is_known_redis_read, is_plain_read};
use crate::ui::text;

/// The intervals of the dialog, in seconds. `None` turns the refresh off.
pub const INTERVALS: [Option<u64>; 7] = [
    None,
    Some(1),
    Some(2),
    Some(5),
    Some(10),
    Some(30),
    Some(60),
];

impl App {
    /// True when the tab on screen accepts an auto-refresh choice.
    pub(super) fn auto_refresh_available(&self) -> bool {
        if self.console_on_screen() {
            self.console().connection.is_some()
        } else {
            self.table_active.is_some()
        }
    }

    /// Opens the dialog that arms or disarms the auto-refresh of the tab on screen.
    pub(super) fn open_auto_refresh(&mut self) {
        let current = self.auto_seconds();
        let options: Vec<_> = INTERVALS
            .iter()
            .map(|seconds| {
                (
                    text::auto_refresh_choice(*seconds),
                    DialogAction::AutoRefresh(*seconds),
                )
            })
            .collect();
        self.show_dialog(
            text::AUTO_REFRESH_TITLE,
            text::auto_refresh_body(current),
            options,
        );
    }

    /// The interval of the tab on screen, or `None` when it is off.
    fn auto_seconds(&self) -> Option<u64> {
        let every = if self.console_on_screen() {
            self.console().auto_refresh.as_ref().map(|auto| auto.every)
        } else {
            self.table()
                .and_then(|tab| tab.auto_refresh.as_ref().map(|auto| auto.every))
        }?;
        Some(every.as_secs())
    }

    /// Applies the answer of the dialog to the tab on screen.
    pub(super) fn choose_auto_refresh(&mut self, seconds: Option<u64>) {
        let now = Instant::now();
        if self.console_on_screen() {
            let index = self.active;
            match seconds {
                None => self.consoles[index].auto_refresh = None,
                Some(seconds) => {
                    let Some(statements) = self.consoles[index].auto_snapshot.clone() else {
                        self.status = Some(text::AUTO_REFRESH_NEEDS_RUN.to_owned());
                        return;
                    };
                    let Some(connection) = self.consoles[index].connection else {
                        return;
                    };
                    let Some(dialect) = self.dialect_of(connection) else {
                        return;
                    };
                    // An auto-refresh never asks the run question, so it repeats reads
                    // only. Anything else refuses the choice.
                    if !statements_are_reads(&statements, dialect) {
                        self.status = Some(text::AUTO_REFRESH_READ_ONLY.to_owned());
                        return;
                    }
                    self.consoles[index].auto_refresh =
                        Some(AutoRefresh::new(Duration::from_secs(seconds), now));
                }
            }
            return;
        }
        let Some(tab) = self.table_mut() else { return };
        match seconds {
            None => tab.auto_refresh = None,
            Some(seconds) => {
                tab.auto_refresh = Some(AutoRefresh::new(Duration::from_secs(seconds), now));
            }
        }
    }

    /// One step of the main loop. It fires the due refresh of the tab on screen and
    /// returns true when the screen changed. A busy tab, an open modal, or a blocked
    /// configuration skips the moment in silence.
    pub(super) fn auto_refresh_tick(&mut self, now: Instant) -> bool {
        if self.modal_is_open() || self.config_blocked() {
            return false;
        }
        if self.console_on_screen() {
            self.console_auto_tick(now)
        } else {
            self.table_auto_tick(now)
        }
    }

    fn console_auto_tick(&mut self, now: Instant) -> bool {
        let index = self.active;
        if !self.consoles[index]
            .auto_refresh
            .is_some_and(|auto| auto.due(now))
            || self.consoles[index].is_busy()
            || self.consoles[index].transaction == crate::connector::TransactionState::Unknown
            || self.consoles[index].auto_snapshot.is_none()
        {
            return false;
        }
        let fired = self.replay_last_run(index);
        if let Some(auto) = self.consoles[index].auto_refresh.as_mut() {
            auto.spent(now);
        }
        fired
    }

    /// Repeats the statements of the last run with the current parameter values.
    fn replay_last_run(&mut self, index: usize) -> bool {
        let Some(statements) = self.consoles[index].auto_snapshot.clone() else {
            return false;
        };
        let Some(connection) = self.consoles[index].connection else {
            return false;
        };
        let Some(dialect) = self.dialect_of(connection) else {
            return false;
        };
        let console = &mut self.consoles[index];
        let Ok(parameters) = console.parameters.values(&statements, dialect) else {
            // A missing value waits for the user, so the moment leaves in silence.
            return false;
        };
        let manual = console.manual && !self.agent.owns(console.id);
        let schema = console.schema.clone();
        let console = console.id;
        let bound = parameters.clone();
        let options = RunOptions {
            manual: manual && self.has_transactions(connection),
            schema,
            only_schema: false,
            row_cap: None,
            export: None,
            stream: false,
            continue_on_error: false,
            hold_transaction: false,
            untimed: false,
            parameters,
        };
        let plan = RunPlan {
            statements,
            rows: Vec::new(),
            starts: Vec::new(),
        };
        let Some(manager) = &self.manager else {
            return false;
        };
        let execution = manager.run(console, plan.statements.clone(), options);
        let budget = self.budget.clone();
        self.consoles[index].auto_execution = Some(execution);
        self.consoles[index].begin_execution(execution, &plan, &budget);
        // A repetition is no plan, and a full export reread uses its parameters.
        self.consoles[index].explain = None;
        self.consoles[index].ran_parameters = bound;
        true
    }

    fn table_auto_tick(&mut self, now: Instant) -> bool {
        let Some(index) = self.table_active else {
            return false;
        };
        let connection = self.tables[index].connection;
        if !self.tables[index]
            .auto_refresh
            .is_some_and(|auto| auto.due(now))
            || self.tables[index].is_busy()
            || !self.tables[index].result.edits.is_empty()
        {
            return false;
        }
        let Some(dialect) = self.dialect_of(connection) else {
            return false;
        };
        let budget = self.budget.clone();
        let request = self.tables[index].refresh_page(dialect, &budget);
        let fired = request.is_some();
        if let Some(auto) = self.tables[index].auto_refresh.as_mut() {
            auto.spent(now);
        }
        if fired {
            self.run_page_quietly(index, request);
            self.tables[index].auto_execution = self.tables[index].current_execution();
        }
        fired
    }

    /// Sends a page request of a table tab without the guards of a user action, because
    /// the conditions were checked before the request was built.
    fn run_page_quietly(&mut self, index: usize, request: Option<crate::table::PageRequest>) {
        let Some(request) = request else { return };
        let tab = &self.tables[index];
        let (console, connection) = (tab.id, tab.connection);
        self.authorize(
            connection,
            super::auth::Use::Page {
                console,
                connection,
                request,
            },
        );
    }

    /// Ends the auto-refresh of a tab when its execution did not complete. The marker
    /// of the execution always leaves, because the execution is over.
    pub(super) fn on_auto_execution_end(
        &mut self,
        event: &ExecEvent,
        id: crate::exec::ConsoleId,
        execution: Option<ExecutionId>,
    ) {
        let Some(ended) = execution else { return };
        let ExecEvent::ExecutionFinished {
            execution,
            end,
            message,
            ..
        } = event
        else {
            return;
        };
        if *execution != ended {
            return;
        }
        let disabled = if let Some(console) = self
            .consoles
            .iter_mut()
            .find(|console| console.id == id && console.auto_execution == Some(ended))
        {
            console.auto_execution = None;
            if *end != ExecutionEnd::Completed {
                console.auto_refresh = None;
                true
            } else {
                false
            }
        } else if let Some(tab) = self
            .tables
            .iter_mut()
            .find(|tab| tab.id == id && tab.auto_execution == Some(ended))
        {
            tab.auto_execution = None;
            if *end != ExecutionEnd::Completed {
                tab.auto_refresh = None;
                true
            } else {
                false
            }
        } else {
            return;
        };
        if disabled {
            self.status = Some(text::auto_refresh_stopped(message.as_deref()));
        }
    }
}

/// Returns true when every statement is a read that an auto-refresh can repeat without
/// a question. A Redis command must be a known read.
pub fn statements_are_reads(statements: &[String], dialect: Dialect) -> bool {
    statements.iter().all(|sql| {
        if dialect == Dialect::Redis {
            is_known_redis_read(sql)
        } else {
            is_plain_read(sql, dialect)
        }
    })
}
