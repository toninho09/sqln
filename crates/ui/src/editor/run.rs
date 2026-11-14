//! Resolves the same text ranges for execution and its preview.

use super::buffer::Snapshot;
use super::{RunPlan, RunRefusal, RunScope, SqlEditor};
use crate::sql::{Statement, split, split_tokens, subquery_at};

/// The one-based lines of the text that Run executes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunPreview {
    pub first_line: usize,
    pub last_line: usize,
}

impl RunPreview {
    pub fn contains_line(self, line: usize) -> bool {
        (self.first_line..=self.last_line).contains(&line)
    }
}

pub(super) struct Selection {
    start: usize,
    end: usize,
    statements: Vec<Statement>,
}

impl SqlEditor {
    /// Returns the cursor statement only when Run does not use a selection.
    pub fn statement_preview(&mut self) -> Option<RunPreview> {
        if self.literal_text || self.review.is_some() || self.selection_range().is_some() {
            return None;
        }
        self.run_preview().ok()
    }

    /// Returns a preview without copying SQL or changing the editor state.
    pub fn run_preview(&mut self) -> Result<RunPreview, RunRefusal> {
        self.with_run(RunScope::Default, |snapshot, statements| RunPreview {
            first_line: snapshot.row_of(statements[0].start) + 1,
            last_line: snapshot.row_of(statements.last().unwrap().end.saturating_sub(1)) + 1,
        })
    }

    /// Finds the statements of an execution, and it copies their text.
    pub fn plan_run(&mut self, scope: RunScope) -> Result<RunPlan, RunRefusal> {
        self.with_run(scope, |snapshot, statements| RunPlan {
            statements: statements
                .iter()
                .map(|statement| statement.text(&snapshot.text).to_owned())
                .collect(),
            rows: statements
                .iter()
                .map(|statement| snapshot.row_of(statement.start))
                .collect(),
            starts: statements.iter().map(|statement| statement.start).collect(),
        })
    }

    fn with_run<T>(
        &mut self,
        scope: RunScope,
        read: impl FnOnce(&Snapshot, &[Statement]) -> T,
    ) -> Result<T, RunRefusal> {
        if scope == RunScope::Subquery && self.dialect.is_command_line() {
            return Err(RunRefusal::NoSubquery);
        }
        let selection = (scope == RunScope::Default)
            .then(|| self.selection_range())
            .flatten();
        let cursor = self.state.cursor;
        let dialect = self.dialect;
        let snapshot = self.snapshot();
        let statements = if let Some((start, end)) = selection {
            let (start, end) = (snapshot.offset(start), snapshot.offset(end));
            if snapshot
                .run_selection
                .as_ref()
                .is_none_or(|cached| cached.start != start || cached.end != end)
            {
                let mut statements = split(&snapshot.text[start..end], dialect);
                for statement in &mut statements {
                    statement.start += start;
                    statement.end += start;
                    statement.end_with_delimiter += start;
                }
                snapshot.run_selection = Some(Selection {
                    start,
                    end,
                    statements,
                });
            }
            snapshot
                .run_selection
                .as_ref()
                .unwrap()
                .statements
                .as_slice()
        } else {
            let statements = snapshot
                .statements
                .get_or_init(|| split_tokens(&snapshot.text, dialect, &snapshot.tokens));
            if scope == RunScope::Script {
                statements.as_slice()
            } else {
                let offset = snapshot.offset(cursor);
                let index = statements.partition_point(|item| item.end_with_delimiter < offset);
                let statement = statements
                    .get(index)
                    .filter(|item| offset >= item.start)
                    .ok_or(if scope == RunScope::Subquery {
                        RunRefusal::NoSubquery
                    } else {
                        RunRefusal::CursorOutside
                    })?;
                if scope == RunScope::Subquery {
                    let start = snapshot
                        .tokens
                        .partition_point(|token| token.start() < statement.start);
                    let end = snapshot
                        .tokens
                        .partition_point(|token| token.start() < statement.end);
                    let subquery =
                        subquery_at(&snapshot.text, &snapshot.tokens[start..end], offset)
                            .ok_or(RunRefusal::NoSubquery)?;
                    return Ok(read(snapshot, std::slice::from_ref(&subquery)));
                }
                std::slice::from_ref(statement)
            }
        };
        if statements.is_empty() {
            return Err(RunRefusal::NoStatement);
        }
        Ok(read(snapshot, statements))
    }
}
