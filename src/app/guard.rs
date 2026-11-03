//! The question before a run: a write without `WHERE`, or a change in production.

use super::App;
use crate::editor::RunPlan;
use crate::exec::ConnectionId;
use crate::grid::printable;
use crate::sql::{Dialect, changes_state, lacks_where};
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
        if self.is_read_only(connection) {
            return None;
        }
        let item = self.connection(connection)?;
        let dialect = item.dialect;
        let production = item.is_production();
        let of_class = |class: fn(&str, Dialect) -> bool, active: bool| -> Vec<&str> {
            let statements = plan.statements.iter().map(String::as_str);
            statements
                .filter(|sql| active && class(sql, dialect))
                .collect()
        };
        let no_where = of_class(lacks_where, true);
        let changes = of_class(changes_state, production);
        let first = no_where.first().or(changes.first())?;
        Some(text::run_risk(&RunRisk {
            production: production.then_some(item.plain_name()),
            changes: changes.len(),
            no_where: no_where.len(),
            total: plan.statements.len(),
            first: &short(first),
        }))
    }
}
