//! The marks of statements and errors in the gutter.

use super::*;

impl SqlEditor {
    /// Removes the marks of the last execution.
    pub fn clear_marks(&mut self) {
        self.marks.clear();
        self.error_spot = None;
    }

    pub fn set_marks(&mut self, plan: &RunPlan) {
        self.set_marks_at_revision(plan, self.revision);
    }

    /// Keeps marks tied to the text that starts a request, including credential waits.
    pub fn set_marks_at_revision(&mut self, plan: &RunPlan, revision: u64) {
        self.marks = plan
            .statements
            .iter()
            .zip(plan.rows.iter().zip(&plan.starts))
            .enumerate()
            .map(|(index, (text, (row, start)))| StatementMark {
                index,
                row: *row,
                start: *start,
                text: text.clone(),
                state: None,
            })
            .collect();
        self.marks_revision = revision;
        self.plan_revision = revision;
        self.error_spot = None;
    }

    /// Marks the token of a database error. A text that changed after the start of
    /// the execution has no reliable place for it, so the editor marks nothing.
    pub fn set_error_offset(&mut self, index: usize, offset: usize) {
        if self.revision != self.plan_revision {
            return;
        }
        let Some(mark) = self.mark_place(index).map(|place| &self.marks[place]) else {
            return;
        };
        let (at, statement_end) = (mark.start + offset, mark.start + mark.text.len());
        let revision = self.revision;
        let tokens = &self.snapshot().tokens;
        let next = tokens.partition_point(|token| token.end() <= at);
        let token = tokens
            .get(next)
            .filter(|token| token.start() <= at && at < statement_end)
            .or_else(|| tokens[..next].last());
        self.error_spot = token.map(|token| ErrorSpot {
            start: token.start(),
            end: token.end(),
            revision,
        });
    }

    /// Returns the place of a mark. The marks stay in the order of their statements.
    pub(super) fn mark_place(&self, index: usize) -> Option<usize> {
        self.marks
            .binary_search_by_key(&index, |mark| mark.index)
            .ok()
    }

    pub fn set_mark_state(&mut self, index: usize, state: MarkState) {
        if let Some(place) = self.mark_place(index) {
            self.marks[place].state = Some(state);
        }
    }

    /// Returns the marks of the current text. A statement with another text loses
    /// its mark, because an old result must not describe the new text.
    pub fn marks(&mut self) -> &[StatementMark] {
        if self.marks_revision != self.revision && !self.marks.is_empty() {
            self.marks_revision = self.revision;
            let dialect = self.dialect;
            let mut marks = std::mem::take(&mut self.marks);
            let snapshot = self.snapshot();
            let text = snapshot.text.as_str();
            let mut free: HashMap<&str, BTreeSet<(usize, usize)>> = HashMap::new();
            for (index, statement) in split(text, dialect).iter().enumerate() {
                let row = snapshot.row_of(statement.start);
                free.entry(statement.text(text))
                    .or_default()
                    .insert((row, index));
            }
            marks.retain_mut(|mark| {
                let Some(places) = free.get_mut(mark.text.as_str()) else {
                    return false;
                };
                let Some(place) = nearest_place(places, mark.row) else {
                    return false;
                };
                places.remove(&place);
                mark.row = place.0;
                true
            });
            self.marks = marks;
        }
        &self.marks
    }
}
