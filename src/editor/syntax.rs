//! Debounced syntax checks for one editor revision.

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use super::SqlEditor;
use crate::sql::Dialect;
use crate::sql::diagnostics::{self, Report};

type Key = (u64, Dialect);

#[derive(Default)]
pub(super) struct Syntax {
    observed: Option<(Key, Instant)>,
    completed: Option<Key>,
    running: Option<(Key, Receiver<Report>)>,
    report: Option<Report>,
}

impl SqlEditor {
    /// Checks only after a pause. The caller supplies the existing blocking executor.
    pub fn poll_syntax(
        &mut self,
        now: Instant,
        spawn: impl FnOnce(Box<dyn FnOnce() + Send>),
    ) -> bool {
        if self.literal_text || self.dialect.is_command_line() {
            return false;
        }
        let key = (self.revision, self.dialect);
        let mut changed = false;
        if self
            .syntax
            .observed
            .is_none_or(|(previous, _)| previous != key)
        {
            self.syntax.observed = Some((key, now));
            changed = self.syntax.report.take().is_some();
            self.syntax.completed = None;
        }
        if let Some((running, receiver)) = &self.syntax.running {
            match receiver.try_recv() {
                Ok(report) => {
                    if *running == key {
                        self.syntax.completed = Some(key);
                        self.syntax.report = Some(report);
                        changed = true;
                    }
                    self.syntax.running = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    if *running == key {
                        self.syntax.completed = Some(key);
                        self.syntax.report = Some(Report::skipped(
                            "analysis worker stopped; edit the text to retry",
                        ));
                        changed = true;
                    }
                    self.syntax.running = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.syntax.running.is_none()
            && self.syntax.completed != Some(key)
            && self.syntax.observed.is_some_and(|(_, at)| {
                now.saturating_duration_since(at) >= Duration::from_millis(400)
            })
        {
            // Count encoded bytes without allocating a snapshot of an oversized script.
            let bytes = self.state.lines.iter_row().try_fold(0usize, |bytes, row| {
                row.iter()
                    .try_fold(bytes.saturating_add(1), |bytes, c| {
                        let bytes = bytes.saturating_add(c.len_utf8());
                        (bytes <= diagnostics::MAX_BYTES + 1).then_some(bytes)
                    })
                    .filter(|bytes| *bytes <= diagnostics::MAX_BYTES + 1)
            });
            if bytes.is_none() {
                self.syntax.completed = Some(key);
                self.syntax.report = Some(Report::skipped("text exceeds 256 KiB"));
                return true;
            }
            let sql = self.text();
            let dialect = self.dialect;
            let (sender, receiver) = mpsc::sync_channel(1);
            self.syntax.running = Some((key, receiver));
            spawn(Box::new(move || {
                let _ = sender.send(diagnostics::check(&sql, dialect));
            }));
        }
        changed
    }

    /// Returns only a report that describes the current text and dialect.
    pub fn syntax_report(&self) -> Option<&Report> {
        (self.syntax.completed == Some((self.revision, self.dialect))
            && !self.literal_text
            && !self.dialect.is_command_line())
        .then_some(self.syntax.report.as_ref())
        .flatten()
    }
}
