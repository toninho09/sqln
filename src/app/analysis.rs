//! Commands and bounded work for analysis of loaded results.

use std::time::Instant;

use super::{App, Overlay, Picker, PickerKind};
use crate::analysis::{ChartKind, Job, STALE_ERROR};
use crate::input::Action;
use crate::ui::text;
use crate::ui::wrap::WrappedText;

pub(super) struct Choice {
    generation: u64,
    step: Step,
}

enum Step {
    ChartKind,
    X(ChartKind),
    Y(ChartKind, usize),
    /// The other tabs that a comparison can use, by the generation of their table.
    Other(Vec<u64>),
    /// The generation of the other table and the chosen key columns.
    Keys(u64, Vec<usize>),
}

impl App {
    pub(super) fn analysis_available(&self, action: Action) -> bool {
        if self.diagram_index().is_some() || self.compare_index().is_some() {
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
                    && result.is_some_and(|result| result.is_complete())
                    && !self.comparison_tabs().is_empty()
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
            self.status = Some(text::ANALYSIS_UNAVAILABLE.into());
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
            Action::GridComparePrevious => {
                let tabs = self.comparison_tabs();
                self.analysis_picker(Step::Other(tabs), 0);
            }
            _ => {}
        }
    }

    /// Lists the other complete data tabs of the console with the columns of the
    /// active result, by the generation of their tables.
    fn comparison_tabs(&self) -> Vec<u64> {
        let Some(active) = self.active_result() else {
            return Vec::new();
        };
        self.console()
            .results
            .iter()
            .filter(|result| {
                result.table.generation != active.table.generation
                    && !result.plan
                    && result.is_complete()
                    && result.table.columns == active.table.columns
            })
            .map(|result| result.table.generation)
            .collect()
    }

    /// Returns the result of the console with a table generation.
    fn console_result(&self, generation: u64) -> Option<&crate::console::ResultTab> {
        self.console()
            .results
            .iter()
            .find(|result| result.table.generation == generation)
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
            Step::Other(tabs) => (
                "Compare with result",
                tabs.iter()
                    .filter_map(|generation| self.console_result(*generation))
                    .map(|result| result.label())
                    .collect(),
            ),
            Step::Keys(_, keys) => {
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
        self.modals.picker = Some(Picker {
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
                let Some(result) = self.active_result() else {
                    self.status = Some(STALE_ERROR.into());
                    return;
                };
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
            Step::Other(tabs) => {
                let Some(&other) = tabs.get(cursor) else {
                    return;
                };
                if self.console_result(other).is_none() {
                    self.status = Some(STALE_ERROR.into());
                    return;
                }
                self.analysis_picker(Step::Keys(other, Vec::new()), 0);
            }
            Step::Keys(other, mut keys) if cursor > 0 => {
                let column = cursor - 1;
                if keys.contains(&column) {
                    keys.retain(|&key| key != column);
                } else {
                    keys.push(column);
                }
                self.analysis_picker(Step::Keys(other, keys), cursor);
            }
            Step::Keys(other, keys) => {
                let Some(result) = self.active_result() else {
                    self.status = Some(STALE_ERROR.into());
                    return;
                };
                let Some(previous) = self.console_result(other) else {
                    self.status = Some(STALE_ERROR.into());
                    return;
                };
                let names = (previous.label(), result.label());
                match Job::diff(
                    &result.table,
                    &previous.table,
                    keys,
                    (&names.0, &names.1),
                    &self.budget,
                ) {
                    Ok(job) => self.start_analysis(job),
                    Err(reason) => self.status = Some(reason.into()),
                }
            }
        }
    }

    fn start_analysis(&mut self, job: Job) {
        self.background.analysis = Some(job);
        self.modals.overlay = Some(Overlay {
            _analysis_memory: None,
            _ai_memory: None,
            title: "Result analysis".into(),
            contents: WrappedText::new("Calculating loaded rows...".into()),
            footer: "Esc cancel".into(),
            scroll: 0,
            dragging: false,
            other: None,
            row: None,
        });
    }

    pub(super) fn continue_analysis(&mut self, deadline: Instant) {
        let Some(mut job) = self.background.analysis.take() else {
            return;
        };
        if self.modals.overlay.is_none() {
            return;
        }
        let other = job
            .other_generation()
            .and_then(|generation| self.console_result(generation))
            .map(|result| &result.table);
        let outcome = match self.active_result() {
            Some(result) => job.step(&result.table, other, deadline),
            None => Err(STALE_ERROR),
        };
        match outcome {
            Ok(false) => self.background.analysis = Some(job),
            Ok(true) => {
                let report = job.finish();
                self.modals.overlay = Some(Overlay {
                    _analysis_memory: Some(report.memory),
                    _ai_memory: None,
                    title: report.title,
                    contents: WrappedText::new(report.text),
                    footer: "Esc close | Up/Down scroll | Ctrl+C copy".into(),
                    scroll: 0,
                    dragging: false,
                    other: None,
                    row: None,
                });
            }
            Err(reason) => {
                self.modals.overlay = None;
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
