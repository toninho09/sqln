//! The question before a run: a write without `WHERE`, or a change in production.

use super::App;
use crate::editor::RunPlan;
use crate::exec::ConnectionId;
use crate::grid::printable;
use crate::sql::run_risk;
use crate::ui::text::{self, RunRisk};

/// The longest part of a statement that the question shows.
const SHOWN_CHARS: usize = 200;

/// Writes the start of a statement in one row.
pub(super) fn short(sql: &str) -> String {
    let row = printable(&sql.split_whitespace().collect::<Vec<_>>().join(" "));
    let mut shown: String = row.chars().take(SHOWN_CHARS).collect();
    if row.chars().count() > SHOWN_CHARS {
        shown.push('…');
    }
    shown
}

impl App {
    /// Returns true when the settings or the definition keep a connection read-only.
    pub(super) fn is_read_only(&self, connection: ConnectionId) -> bool {
        let definition = self
            .connection(connection)
            .and_then(|item| item.definition.as_ref());
        self.settings.read_only || definition.is_some_and(|definition| definition.read_only)
    }

    /// Returns the question that a run needs. A read-only connection needs none,
    /// because the database refuses the write.
    pub(super) fn run_warning(&self, connection: ConnectionId, plan: &RunPlan) -> Option<String> {
        let item = self.connection(connection)?;
        let production = item.is_production();
        let risk = run_risk(
            &plan.statements,
            item.dialect,
            self.is_read_only(connection),
            production,
        )?;
        Some(text::run_risk(&RunRisk {
            production: production.then_some(item.plain_name()),
            changes: risk.changes,
            no_where: risk.no_where,
            total: plan.statements.len(),
            first: &short(&plan.statements[risk.first]),
        }))
    }
}
