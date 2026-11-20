//! Running SQL: runs, explanations, plans, formats, stops, and transactions.

use super::guard::short;
use super::{App, DialogAction, Overlay};
use crate::connector::{TransactionCommand, TransactionState};
use crate::console::{AfterTransaction, Focus, Phase};
use crate::editor::{FormatOutcome, RunPlan, RunRefusal, RunScope};
use crate::exec::{ConnectionId, RunOptions};
use crate::input::Action;
use crate::plan::Explain;
use crate::sql::{first_keyword, is_plain_read};
use crate::ui::text;
use crate::ui::wrap::WrappedText;

/// What a run starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RunTarget {
    Statements(RunScope),
    /// The plan of one statement. With `analyze`, the database also runs it.
    Plan {
        analyze: bool,
    },
}

impl App {
    pub(super) fn run(&mut self, target: RunTarget) {
        let console = self.console();
        if console.connection.is_none() {
            if console.file.is_some() {
                // A script without a connection needs a choice before it runs.
                self.start_association();
            } else {
                self.status = Some(text::NO_CONNECTION.to_owned());
            }
            return;
        }
        if self.config_blocked() {
            return;
        }
        let console = self.console();
        if console.is_busy() {
            self.status = Some(text::EXECUTION_ACTIVE.to_owned());
            return;
        }
        if console.transaction == TransactionState::Unknown {
            let id = console.id;
            self.show_dialog(
                text::TITLE_UNKNOWN,
                text::BODY_UNKNOWN.to_owned(),
                vec![
                    (text::OPTION_CANCEL, DialogAction::Dismiss),
                    (text::OPTION_CONTINUE, DialogAction::RunAnyway(id, target)),
                ],
            );
            return;
        }
        self.start_run(self.tabs.active, target);
    }

    pub(super) fn start_run(&mut self, index: usize, target: RunTarget) {
        self.plan_and_run(index, target, true);
    }

    /// Runs after the answer to the question of `run_question`.
    pub(super) fn run_confirmed(&mut self, index: usize, target: RunTarget) {
        self.plan_and_run(index, target, false);
    }

    /// Returns the plan that a target asks for. A plan needs a source with plans and one
    /// statement, and the error is the reason for the status line.
    pub(super) fn explain_of(
        &self,
        connection: ConnectionId,
        target: RunTarget,
        plan: &RunPlan,
    ) -> Result<Option<Explain>, &'static str> {
        let RunTarget::Plan { analyze } = target else {
            return Ok(None);
        };
        let dialect = self.connection(connection).map(|item| item.dialect);
        let explain = dialect.and_then(|dialect| Explain::new(dialect, analyze));
        let Some(explain) = explain else {
            // The source has a plan, but it cannot run the statement in it.
            let no_analyze = analyze && dialect.and_then(|d| Explain::new(d, false)).is_some();
            return Err(if no_analyze {
                text::PLAN_ANALYZE_REFUSED
            } else {
                text::PLAN_REFUSED
            });
        };
        match plan.statements.as_slice() {
            [statement]
                if first_keyword(statement, explain.dialect)
                    .is_some_and(|word| word.eq_ignore_ascii_case("EXPLAIN")) =>
            {
                Err(text::PLAN_HAS_EXPLAIN)
            }
            [_] => Ok(Some(explain)),
            _ => Err(text::PLAN_ONE_STATEMENT),
        }
    }

    /// Returns the question before a run. A plan does not run its statement, so it needs
    /// no question. A real run of a statement that can write always asks.
    pub(super) fn run_question(
        &self,
        connection: ConnectionId,
        target: RunTarget,
        plan: &RunPlan,
    ) -> Option<String> {
        if target == (RunTarget::Plan { analyze: false }) {
            return None;
        }
        let warning = self.run_warning(connection, plan);
        if target == RunTarget::Statements(RunScope::Subquery) {
            return Some(text::subquery_question(
                plan.rows[0] + 1,
                &short(&plan.statements[0]),
                warning.as_deref(),
            ));
        }
        let dialect = self.connection(connection)?.dialect;
        let statement = plan.statements.first()?;
        let writes = matches!(target, RunTarget::Plan { .. })
            && !is_plain_read(statement, dialect)
            && !self.is_read_only(connection);
        if !writes {
            return warning;
        }
        let details = warning.unwrap_or_else(|| short(statement));
        Some(format!("{}\n\n{details}", text::PLAN_ANALYZE_RUNS))
    }

    pub(super) fn plan_and_run(&mut self, index: usize, target: RunTarget, ask: bool) {
        let scope = match target {
            RunTarget::Statements(scope) => scope,
            RunTarget::Plan { .. } => RunScope::Default,
        };
        let console = &mut self.tabs.consoles[index];
        match console.editor.plan_run(scope) {
            Ok(plan) => {
                let Some(connection) = console.connection else {
                    return;
                };
                let dialect = self.tabs.consoles[index].editor.dialect();
                let full_text = self.tabs.consoles[index].editor.text();
                self.tabs.consoles[index]
                    .parameters
                    .update_names(crate::sql::parameters::names(&full_text, dialect));
                let parameters = match self.tabs.consoles[index]
                    .parameters
                    .values(&plan.statements, dialect)
                {
                    Ok(parameters) => parameters,
                    Err(reason) => {
                        self.navigator_focused = false;
                        self.tabs.consoles[index].focus = Focus::Parameters;
                        self.status = Some(reason);
                        return;
                    }
                };
                if !parameters.is_empty()
                    && !self
                        .connection(connection)
                        .is_some_and(|item| item.capabilities.parameters)
                {
                    self.status =
                        Some("This connector does not support query parameters.".to_owned());
                    return;
                }
                let explain = match self.explain_of(connection, target, &plan) {
                    Ok(explain) => explain,
                    Err(reason) => {
                        self.status = Some(reason.to_owned());
                        return;
                    }
                };
                if ask && let Some(body) = self.run_question(connection, target, &plan) {
                    let id = self.tabs.consoles[index].id;
                    self.show_dialog(
                        text::TITLE_CONFIRM_RUN,
                        body,
                        vec![
                            (text::OPTION_CANCEL, DialogAction::Dismiss),
                            (text::OPTION_RUN, DialogAction::RunConfirmed(id, target)),
                        ],
                    );
                    return;
                }
                let console = &self.tabs.consoles[index];
                // An agent tab runs in Auto mode: an open transaction would block the agent.
                let manual = console.manual && !self.agent.owns(console.id);
                let schema = console.schema.clone();
                // A console can keep its manual mode when it moves to another source.
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
                    live: true,
                };
                let console = &self.tabs.consoles[index];
                let revision = console.editor.revision();
                let console = console.id;
                self.authorize(
                    connection,
                    super::auth::Use::Run {
                        console,
                        connection,
                        plan,
                        options,
                        revision,
                        explain,
                    },
                );
            }
            Err(RunRefusal::CursorOutside) => {
                self.status = Some(text::CURSOR_OUTSIDE.to_owned());
            }
            Err(RunRefusal::NoStatement) => self.status = Some(text::NO_STATEMENT.to_owned()),
            Err(RunRefusal::NoSubquery) => self.status = Some(text::NO_SUBQUERY.to_owned()),
        }
    }

    pub(super) fn show_sql_diagnostics(&mut self) {
        if !self.command_available(Action::SqlDiagnostics) {
            let message =
                if !self.console_on_screen() || self.console().editor.dialect().is_command_line() {
                    text::SYNTAX_UNAVAILABLE
                } else if self.console().editor.syntax_report().is_some() {
                    text::SYNTAX_CLEAN
                } else {
                    text::SYNTAX_WAITING
                };
            self.status = Some(message.into());
            return;
        }
        let Some(report) = self.console().editor.syntax_report() else {
            return;
        };
        let contents = text::syntax_details(report);
        self.console_mut().editor.close_suggestions();
        self.modals.overlay = Some(Overlay {
            _analysis_memory: None,
            _ai_memory: None,
            title: text::SYNTAX_TITLE.into(),
            contents: WrappedText::new(contents),
            footer: "Esc close | Up/Down scroll | Ctrl+C copy".into(),
            scroll: 0,
            dragging: false,
            other: None,
            row: None,
        });
    }

    pub(super) fn format_console(&mut self) {
        if !self.command_available(Action::FormatSql) {
            return;
        }
        let message = match self.console_mut().editor.format_sql() {
            Ok(FormatOutcome::Changed) => return,
            Ok(FormatOutcome::Unchanged) => text::FORMAT_UNCHANGED,
            Ok(FormatOutcome::Refused) => text::FORMAT_REFUSED,
            Err(RunRefusal::CursorOutside) => text::CURSOR_OUTSIDE,
            Err(RunRefusal::NoStatement) => text::NO_STATEMENT,
            Err(RunRefusal::NoSubquery) => text::NO_SUBQUERY,
        };
        self.status = Some(message.to_owned());
    }

    pub(super) fn comment_console(&mut self) {
        if self.command_available(Action::ToggleComment) {
            self.console_mut().editor.toggle_comment();
        }
    }

    pub(super) fn stop(&mut self, index: usize) {
        let id = self.tabs.consoles[index].id;
        if self.cancel_ai_explanation(id) {
            return;
        }
        if self.cancel_agent_console(id) {
            if self.tabs.consoles[index].closing && !self.tabs.consoles[index].is_busy() {
                self.close_step(id);
            }
            return;
        }
        if self.stop_export(id) {
            return;
        }
        if self.cancel_auth_console(id) {
            if self.tabs.consoles[index].closing {
                self.close_step(id);
            }
            return;
        }
        let console = &mut self.tabs.consoles[index];
        if !console.is_busy() {
            return;
        }
        if let Some(manager) = &self.manager {
            manager.stop(console.id);
            console.phase = Phase::Stopping;
        }
    }

    pub(super) fn transaction(&mut self, command: TransactionCommand) {
        let console = self.console();
        if console.is_busy() {
            self.status = Some(text::EXECUTION_ACTIVE.to_owned());
            return;
        }
        if !console.in_transaction() {
            self.status = Some(text::NO_TRANSACTION.to_owned());
            return;
        }
        if let Some(manager) = &self.manager {
            manager.transaction(console.id, command);
        }
    }

    pub(super) fn toggle_mode(&mut self) {
        let console = self.console_mut();
        if console.is_busy() {
            self.status = Some(text::EXECUTION_ACTIVE.to_owned());
            return;
        }
        if !console.manual {
            console.manual = true;
            return;
        }
        if !console.in_transaction() {
            console.manual = false;
            return;
        }
        let id = console.id;
        let after = AfterTransaction::UseAutocommit;
        let rollback = DialogAction::Transaction(id, TransactionCommand::Rollback, after);
        let commit = DialogAction::Transaction(id, TransactionCommand::Commit, after);
        self.show_dialog(
            text::TITLE_TRANSACTION,
            text::BODY_TRANSACTION_MODE.to_owned(),
            vec![
                (text::OPTION_CANCEL, DialogAction::Dismiss),
                (text::OPTION_ROLLBACK_AUTO, rollback),
                (text::OPTION_COMMIT_AUTO, commit),
            ],
        );
    }
}
