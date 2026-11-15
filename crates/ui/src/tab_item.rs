//! The shared contract of the tabs that are not consoles: tables, diagrams, and compares.

use crate::compare_tab::CompareTab;
use crate::console::{Phase, RunClock};
use crate::diagram_tab::DiagramTab;
use crate::exec::{ConnectionId, ConsoleId};
use crate::grid::Budget;
use crate::table::TableTab;
use crate::ui::theme::Theme;
use crate::ui::{compare, diagram, table};

/// A tab of the tab row that is not a console.
pub trait TabItem {
    /// The place of the tab in the order of opening.
    fn opened(&self) -> u64;
    fn phase(&self) -> Phase;
    fn run(&self) -> &RunClock;
    fn run_mut(&mut self) -> &mut RunClock;
    /// The tasks of the tab. The first task is the main task.
    fn console_ids(&self) -> Vec<ConsoleId>;
    /// Returns true when the tab reads from the connection.
    fn uses(&self, connection: ConnectionId) -> bool;
    /// Returns the rows of the tab to the budget.
    fn release(&mut self, budget: &Budget);
    /// The text of the tab in the tab row.
    fn label(&self, theme: &Theme) -> String;
}

impl TabItem for TableTab {
    fn opened(&self) -> u64 {
        self.opened
    }

    fn phase(&self) -> Phase {
        self.phase
    }

    fn run(&self) -> &RunClock {
        &self.run
    }

    fn run_mut(&mut self) -> &mut RunClock {
        &mut self.run
    }

    fn console_ids(&self) -> Vec<ConsoleId> {
        vec![self.id]
    }

    fn uses(&self, connection: ConnectionId) -> bool {
        self.connection == connection
    }

    fn release(&mut self, budget: &Budget) {
        budget.release(std::mem::take(&mut self.result.table.bytes));
    }

    fn label(&self, theme: &Theme) -> String {
        table::tab_label(self, theme)
    }
}

impl TabItem for DiagramTab {
    fn opened(&self) -> u64 {
        self.opened
    }

    fn phase(&self) -> Phase {
        self.phase
    }

    fn run(&self) -> &RunClock {
        &self.run
    }

    fn run_mut(&mut self) -> &mut RunClock {
        &mut self.run
    }

    fn console_ids(&self) -> Vec<ConsoleId> {
        vec![self.id]
    }

    fn uses(&self, connection: ConnectionId) -> bool {
        self.connection == connection
    }

    fn release(&mut self, budget: &Budget) {
        DiagramTab::release(self, budget);
    }

    fn label(&self, theme: &Theme) -> String {
        diagram::tab_label(self, theme)
    }
}

impl TabItem for CompareTab {
    fn opened(&self) -> u64 {
        self.opened
    }

    fn phase(&self) -> Phase {
        self.phase
    }

    fn run(&self) -> &RunClock {
        &self.run
    }

    fn run_mut(&mut self) -> &mut RunClock {
        &mut self.run
    }

    fn console_ids(&self) -> Vec<ConsoleId> {
        self.sides.iter().map(|read| read.id).collect()
    }

    fn uses(&self, connection: ConnectionId) -> bool {
        self.sides
            .iter()
            .any(|read| read.side.connection == connection)
    }

    fn release(&mut self, budget: &Budget) {
        CompareTab::release(self, budget);
    }

    fn label(&self, theme: &Theme) -> String {
        compare::tab_label(self, theme)
    }
}
