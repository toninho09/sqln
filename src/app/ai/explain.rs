//! AI explanations of errors and plans.

use super::super::{App, Overlay};
use super::{Completed, RunKind};
use crate::ai::{self, Memory};
use crate::exec::ConsoleId;
use crate::grid::ResultTable;
use crate::ui::text;
use crate::ui::wrap::WrappedText;

impl App {
    pub(in crate::app) fn ai_explanation_running(&self, console: ConsoleId) -> bool {
        self.ai
            .runs
            .iter()
            .any(|run| run.console == console && matches!(run.kind, RunKind::Explain(_)))
    }

    pub(in crate::app) fn cancel_ai_explanation(&mut self, console: ConsoleId) -> bool {
        let id = self
            .ai
            .runs
            .iter()
            .find(|run| run.console == console && matches!(run.kind, RunKind::Explain(_)))
            .map(|run| run.id);
        if let Some(id) = id {
            self.cancel_ai_id(id);
            self.status = Some("AI explanation cancelled.".into());
        }
        id.is_some()
    }

    /// Returns true when the console can ask for an AI explanation. Every source
    /// qualifies, Redis and Kafka included: the request explains text, not SQL edits.
    pub(super) fn ai_explain_available(&self) -> bool {
        self.console_on_screen()
            && !self.home()
            && self.ai.preferences.enabled()
            && self.ai.preferences.provider.is_some()
            && self.console().connection.is_some()
            && !self.console().is_busy()
            && self.console().editor.review().is_none()
            && !self.association_pending(self.console().id)
    }

    /// Starts the explanation of the newest error of the console.
    pub(in crate::app) fn start_ai_explain_error(&mut self) {
        let Some(explanation) = self.explain_error_source() else {
            return;
        };
        self.start_ai_explain(explanation);
    }

    /// Starts the explanation of the plan on screen.
    pub(in crate::app) fn start_ai_explain_plan(&mut self) {
        let Some(explanation) = self.explain_plan_source() else {
            return;
        };
        self.start_ai_explain(explanation);
    }

    fn explain_error_source(&mut self) -> Option<ai::explain::Explanation> {
        if !self.ai_explain_ready() {
            return None;
        }
        let console = self.console();
        let Some((dialect, statement, error)) = console.messages.last_error() else {
            self.status = Some(text::AI_EXPLAIN_NO_ERROR.to_owned());
            return None;
        };
        Some(ai::explain::Explanation::error(dialect, statement, error))
    }

    fn explain_plan_source(&mut self) -> Option<ai::explain::Explanation> {
        if !self.ai_explain_ready() {
            return None;
        }
        let console = self.console();
        if !console.shows_plan() {
            self.status = Some(text::AI_EXPLAIN_NO_PLAN.to_owned());
            return None;
        }
        let Some(statement) = console.plan_statement() else {
            self.status = Some(text::AI_EXPLAIN_NO_PLAN.to_owned());
            return None;
        };
        let plan = plan_text(&console.results[console.active].table);
        Some(ai::explain::Explanation::plan(
            console.editor.dialect(),
            statement,
            &plan,
        ))
    }

    /// The checks that every explanation shares.
    fn ai_explain_ready(&mut self) -> bool {
        if self.config_blocked() {
            return false;
        }
        if !self.ai.preferences.enabled() {
            self.status = Some(text::AI_EXPLAIN_DISABLED.to_owned());
            return false;
        }
        if !self.console_on_screen() || self.home() || self.console().is_busy() {
            self.status = Some("Open an idle console and try again.".into());
            return false;
        }
        if self.console().connection.is_none() {
            self.status = Some(text::NO_CONNECTION.to_owned());
            return false;
        }
        true
    }

    fn start_ai_explain(&mut self, explanation: ai::explain::Explanation) {
        let Some(provider) = self.ai.preferences.provider.clone() else {
            self.status = Some(text::AI_EXPLAIN_NO_PROVIDER.to_owned());
            return;
        };
        if let Err(error) = provider.validate() {
            self.status = Some(error);
            return;
        }
        let (scope, base, project) = match self.ai_location() {
            Ok(values) => values,
            Err(error) => {
                self.status = Some(error);
                return;
            }
        };
        let connection = self.console().connection.expect("a checked connection");
        let console = self.console().id;
        // One explanation of a console at a time: the answer opens a window, and a
        // second request would replace it unseen.
        if self
            .ai
            .runs
            .iter()
            .any(|r| r.console == console && matches!(r.kind, RunKind::Explain(_)))
        {
            self.status = Some(text::AI_EXPLAIN_BUSY.to_owned());
            return;
        }
        let mut memory = Memory::new(&self.budget);
        if let Err(error) = memory.charge(explanation.memory_bytes()) {
            self.status = Some(error);
            return;
        }
        self.push_ai_run(
            RunKind::Explain(explanation),
            None,
            provider,
            (scope, base, project),
            connection,
            memory,
        );
    }

    /// Shows an AI explanation in the text window of the application.
    pub(super) fn show_ai_explanation(&mut self, label: &str, completed: Completed) {
        let title = if label == "error" {
            text::AI_EXPLAIN_ERROR_TITLE
        } else {
            text::AI_EXPLAIN_PLAN_TITLE
        };
        self.console_mut().editor.close_suggestions();
        self.modals.overlay = Some(Overlay {
            _analysis_memory: None,
            _ai_memory: completed.memory,
            title: title.to_owned(),
            contents: WrappedText::new(completed.text),
            footer: "Esc close | Up/Down scroll | Ctrl+C copy".into(),
            scroll: 0,
            dragging: false,
            other: None,
            row: None,
        });
    }

    /// Keeps the answer pending until no window or input prompt needs the focus.
    pub(super) fn show_ready_ai_explanation(&mut self) -> bool {
        if self.ai_obscured()
            || self.ai.modal()
            || self.ai_input_active()
            || self.console().editor.review().is_some()
        {
            return false;
        }
        let Some(run) = self.ai.runs.iter_mut().find(|run| run.completed.is_some()) else {
            return false;
        };
        let id = run.id;
        let completed = run.completed.take().expect("a pending answer");
        self.show_ai_proposal(id, completed);
        self.cancel_ai_id(id);
        true
    }
}

/// Writes the plan tree of the grid as text: one line per row, the cells joined with
/// " | ". The first line names the columns.
fn plan_text(table: &ResultTable) -> String {
    use crate::grid::Value;
    use std::fmt::Write;
    let mut text = crate::limited::LimitedText::new(ai::explain::DETAIL_LIMIT);
    for column in &table.columns {
        if write!(text, "{column} | ").is_err() {
            return text.finish();
        }
    }
    for row in &table.rows {
        if writeln!(text).is_err() {
            break;
        }
        for cell in row.cells() {
            let value = match cell {
                Value::Text(value) | Value::Number(value) => value,
                Value::Null => "NULL",
                Value::Binary(_) => "[binary]",
            };
            if write!(text, "{value} | ").is_err() {
                return text.finish();
            }
        }
    }
    text.finish()
}
