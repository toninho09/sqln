//! Commands and bounded work for analysis of loaded results.

use std::time::Instant;

use super::{App, Overlay, Picker, PickerKind};
use crate::analysis::{ChartKind, Job, STALE_ERROR};
use crate::input::Action;
use crate::ui::wrap::WrappedText;

pub(super) struct Choice {
    generation: u64,
    step: Step,
}

enum Step {
    ChartKind,
    X(ChartKind),
    Y(ChartKind, usize),
    Keys(Vec<usize>),
}

impl App {
    pub(super) fn analysis_available(&self, action: Action) -> bool {
        if action == Action::GridKeepPrevious {
            return self.console_on_screen() && !self.console().is_busy();
        }
        if self.diagram_active.is_some() || self.compare_active.is_some() {
            return false;
        }
        let idle = self
            .table()
            .map_or_else(|| !self.console().is_busy(), |tab| !tab.is_busy());
        let result = self.active_result();
        let usable = idle && result.is_some_and(|result| !result.plan && result.edits.is_empty());
        match action {
            Action::GridComparePrevious => {
                usable
                    && self.console_on_screen()
                    && self.console().comparison_ready
                    && self.console().previous_result.is_some()
                    && result.is_some_and(|result| result.is_complete())
            }
            Action::GridProfile => usable,
            Action::GridChart => {
                usable
                    && result.is_some_and(|result| {
                        result.table.columns.len() >= 2 && !result.table.rows.is_empty()
                    })
            }
            _ => false,
        }
    }

    pub(super) fn perform_analysis(&mut self, action: Action) {
        if !self.analysis_available(action) {
            self.status = Some("Analysis unavailable. Wait for the result; comparison needs two complete single-result executions and retention enabled.".into());
            return;
        }
        if action == Action::GridKeepPrevious {
            let budget = self.budget.clone();
            let console = self.console_mut();
            console.retain_result = !console.retain_result;
            if !console.retain_result {
                console.discard_previous(&budget);
            }
            self.status = Some(
                if console.retain_result {
                    "Previous-result retention: On. The next run keeps this complete result."
                } else {
                    "Previous-result retention: Off. The retained result is released."
                }
                .into(),
            );
            return;
        }
        let Some(result) = self.active_result() else {
            return;
        };
        match action {
            Action::GridProfile => {
                let job = Job::profile(
                    &result.table,
                    result.view.col,
                    self.analysis_complete(),
                    &self.budget,
                );
                self.start_analysis(job);
            }
            Action::GridChart => self.analysis_picker(Step::ChartKind, 0),
            Action::GridComparePrevious => self.analysis_picker(Step::Keys(Vec::new()), 0),
            _ => {}
        }
    }

    fn analysis_complete(&self) -> bool {
        self.active_result()
            .is_some_and(|result| result.is_complete())
            && self
                .table()
                .is_none_or(|tab| tab.page() == 0 && !tab.has_more)
    }

    fn analysis_picker(&mut self, step: Step, cursor: usize) {
        let Some(result) = self.active_result() else {
            return;
        };
        let (title, items) = match &step {
            Step::ChartKind => ("Chart type", vec!["Bar".to_owned(), "Line".to_owned()]),
            Step::X(_) => (
                "Choose numeric X column",
                column_names(&result.table.columns),
            ),
            Step::Y(_, _) => (
                "Choose numeric Y column",
                column_names(&result.table.columns),
            ),
            Step::Keys(keys) => {
                let mut items = vec!["Compare with selected key columns".to_owned()];
                items.extend(
                    result
                        .table
                        .columns
                        .iter()
                        .enumerate()
                        .map(|(index, name)| {
                            format!(
                                "[{}] {}: {}",
                                if keys.contains(&index) { "x" } else { " " },
                                index + 1,
                                crate::grid::printable(name)
                            )
                        }),
                );
                ("Key columns: Enter toggles; first row compares", items)
            }
        };
        self.picker = Some(Picker {
            kind: PickerKind::Analysis(Choice {
                generation: result.table.generation,
                step,
            }),
            title: title.to_owned(),
            cursor: cursor.min(items.len().saturating_sub(1)),
            items,
            loading: false,
        });
    }

    pub(super) fn choose_analysis(&mut self, choice: Choice, cursor: usize) {
        if self
            .active_result()
            .is_none_or(|result| result.table.generation != choice.generation)
        {
            self.status = Some(STALE_ERROR.into());
            return;
        }
        match choice.step {
            Step::ChartKind => self.analysis_picker(
                Step::X(if cursor == 0 {
                    ChartKind::Bar
                } else {
                    ChartKind::Line
                }),
                0,
            ),
            Step::X(kind) => self.analysis_picker(Step::Y(kind, cursor), 0),
            Step::Y(kind, x) => {
                if x == cursor {
                    self.status = Some("Choose a different column for Y.".into());
                    self.analysis_picker(Step::Y(kind, x), cursor);
                    return;
                }
                let result = self.active_result().unwrap();
                let job = Job::chart(
                    &result.table,
                    x,
                    cursor,
                    kind,
                    self.analysis_complete(),
                    &self.budget,
                );
                self.start_analysis(job);
            }
            Step::Keys(mut keys) if cursor > 0 => {
                let column = cursor - 1;
                if keys.contains(&column) {
                    keys.retain(|&key| key != column);
                } else {
                    keys.push(column);
                }
                self.analysis_picker(Step::Keys(keys), cursor);
            }
            Step::Keys(keys) => {
                let result = self.active_result().unwrap();
                let Some(previous) = self.console().previous_result.as_ref() else {
                    self.status = Some(STALE_ERROR.into());
                    return;
                };
                match Job::diff(&result.table, previous, keys, &self.budget) {
                    Ok(job) => self.start_analysis(job),
                    Err(reason) => self.status = Some(reason.into()),
                }
            }
        }
    }

    fn start_analysis(&mut self, job: Job) {
        self.analysis_job = Some(job);
        self.overlay = Some(Overlay {
            _analysis_memory: None,
            title: "Result analysis".into(),
            contents: WrappedText::new("Calculating loaded rows...".into()),
            footer: "Esc cancel".into(),
            scroll: 0,
            other: None,
            row: None,
        });
    }

    pub(super) fn continue_analysis(&mut self, deadline: Instant) {
        let Some(mut job) = self.analysis_job.take() else {
            return;
        };
        if self.overlay.is_none() {
            return;
        }
        let outcome = match self.active_result() {
            Some(result) => job.step(
                &result.table,
                self.console().previous_result.as_ref(),
                deadline,
            ),
            None => Err(STALE_ERROR),
        };
        match outcome {
            Ok(false) => self.analysis_job = Some(job),
            Ok(true) => {
                let report = job.finish();
                self.overlay = Some(Overlay {
                    _analysis_memory: Some(report.memory),
                    title: report.title,
                    contents: WrappedText::new(report.text),
                    footer: "Esc close | Up/Down scroll | Ctrl+C copy".into(),
                    scroll: 0,
                    other: None,
                    row: None,
                });
            }
            Err(reason) => {
                self.overlay = None;
                self.status = Some(reason.into());
            }
        }
    }
}

fn column_names(columns: &[String]) -> Vec<String> {
    columns
        .iter()
        .enumerate()
        .map(|(index, name)| format!("{}: {}", index + 1, crate::grid::printable(name)))
        .collect()
}
