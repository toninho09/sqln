//! Execution risks shared by interactive and batch execution.

use super::{Dialect, changes_state, lacks_where};

#[derive(Debug, PartialEq, Eq)]
pub struct RunRisk {
    pub changes: usize,
    pub no_where: usize,
    pub first: usize,
}

/// Read-only sessions rely on connector enforcement and need no write question.
pub fn run_risk(
    statements: &[String],
    dialect: Dialect,
    read_only: bool,
    production: bool,
) -> Option<RunRisk> {
    if read_only {
        return None;
    }
    let no_where: Vec<usize> = statements
        .iter()
        .enumerate()
        .filter(|(_, sql)| lacks_where(sql, dialect))
        .map(|(index, _)| index)
        .collect();
    let changes: Vec<usize> = statements
        .iter()
        .enumerate()
        .filter(|(_, sql)| production && changes_state(sql, dialect))
        .map(|(index, _)| index)
        .collect();
    Some(RunRisk {
        changes: changes.len(),
        no_where: no_where.len(),
        first: *no_where.first().or(changes.first())?,
    })
}
