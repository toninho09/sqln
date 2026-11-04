//! Diagram tabs: opening, the read, the keys, the mouse, and their place in the tab row.

use crossterm::event::{MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use super::App;
use super::auth::Use;
use super::mouse::Drag;
use super::tables::Tab;
use crate::catalog::RelationKind;
use crate::connection::CatalogState;
use crate::console::Phase;
use crate::diagram::Focus;
use crate::diagram::layout::Direction;
use crate::diagram::sql::diagram_sql;
use crate::diagram::tab::DiagramTab;
use crate::exec::{ConnectionId, RunOptions};
use crate::input::{Action, Region};
use crate::navigator::{Row, RowKind};
use crate::sql::Dialect;
use crate::structure::Scope;
use crate::ui::text;

/// What a row or a tab gives to a new diagram.
enum Target {
    Open(ConnectionId, Focus),
    /// The current schema of the connection needs its catalog first.
    Wait(ConnectionId),
}

/// The rows of one turn of the mouse wheel, and the columns of a turn to the side.
const WHEEL_ROWS: i64 = 3;
const WHEEL_COLUMNS: i64 = 6;

impl App {
    pub fn diagrams(&self) -> &[DiagramTab] {
        &self.diagrams
    }

    /// Returns the diagram tab on screen.
    pub fn diagram(&self) -> Option<&DiagramTab> {
        self.diagram_active.map(|index| &self.diagrams[index])
    }

    pub(super) fn diagram_mut(&mut self) -> Option<&mut DiagramTab> {
        self.diagram_active.map(|index| &mut self.diagrams[index])
    }

    /// Returns true when the console on screen receives the keys of a console: no table,
    /// no diagram, and no compare is on screen.
    pub(super) fn console_on_screen(&self) -> bool {
        self.table_active.is_none()
            && self.diagram_active.is_none()
            && self.compare_active.is_none()
    }

    /// Returns true when the home screen is on screen: no tab is open, and the active
    /// console is the hidden console.
    pub fn home(&self) -> bool {
        self.console_on_screen() && self.console().hidden
    }

    /// Returns true when an action of the diagram region has an effect now.
    pub(super) fn diagram_command_available(&self, action: Action) -> bool {
        let Some(tab) = self.diagram() else {
            return false;
        };
        match action {
            Action::DiagramReload => !tab.is_busy(),
            Action::DiagramOpen | Action::DiagramStructure => tab.selected.is_some(),
            _ => !tab.model.entities.is_empty(),
        }
    }

    /// Returns true when the row of the explorer can open a diagram.
    pub(super) fn can_open_diagram(&self) -> bool {
        self.navigator
            .current(&self.connections)
            .is_some_and(|row| self.row_diagram(&row).is_ok())
    }

    /// Returns the diagram of a navigator row, or the reason that it has none. A table
    /// shows its neighbors, a column shows the diagram of its table, and a connection
    /// shows its current schema.
    fn row_diagram(&self, row: &Row) -> Result<Target, &'static str> {
        let item = &self.connections[row.connection];
        if item.dialect == Dialect::Redis {
            return Err(text::DIAGRAM_REFUSED);
        }
        let focus = match row.kind {
            RowKind::Connection => return self.schema_target(item.id, None),
            RowKind::Schema => Focus::Schema(row.name(&self.connections).to_owned()),
            RowKind::Table | RowKind::Column => {
                let relation = row.relation_ref(&self.connections);
                Focus::Table(relation.ok_or(text::DIAGRAM_EMPTY)?)
            }
            RowKind::View => return Err(text::DIAGRAM_VIEW),
            RowKind::Consoles | RowKind::Console => return Err(text::DIAGRAM_CONSOLE),
            RowKind::Group(_) | RowKind::Object(_) => return Err(text::NOT_FOR_OBJECT),
        };
        self.focus_target(item.id, focus)
    }

    /// Refuses the diagram of a view, which has no keys.
    fn focus_target(&self, connection: ConnectionId, focus: Focus) -> Result<Target, &'static str> {
        if let Focus::Table(table) = &focus {
            let catalog = self
                .connection(connection)
                .and_then(|item| item.catalog.as_ref());
            let found =
                catalog.and_then(|catalog| catalog.relation(Some(&table.schema), &table.relation));
            if found.is_some_and(|(_, relation)| relation.kind == RelationKind::View) {
                return Err(text::DIAGRAM_VIEW);
            }
        }
        Ok(Target::Open(connection, focus))
    }

    /// Returns the diagram of a chosen schema, or of the current schema of a connection.
    /// Before its catalog, a connection does not know its current schema.
    fn schema_target(
        &self,
        connection: ConnectionId,
        chosen: Option<String>,
    ) -> Result<Target, &'static str> {
        let item = self
            .connection(connection)
            .ok_or(text::NO_SELECTED_CONNECTION)?;
        match chosen.or_else(|| item.default_schema.clone()) {
            Some(schema) => Ok(Target::Open(connection, Focus::Schema(schema))),
            None if matches!(
                item.catalog_state,
                CatalogState::NotLoaded | CatalogState::Loading
            ) =>
            {
                Ok(Target::Wait(connection))
            }
            None => Err(text::DIAGRAM_NO_SCHEMA),
        }
    }

    /// Opens the diagram of a navigator row. A row without a diagram gives the reason.
    pub(super) fn open_row_diagram(&mut self, row: &Row) {
        let target = self.row_diagram(row);
        self.go_to_diagram(target);
    }

    /// Returns the diagram that `Ctrl+W D` opens: the row of the explorer when it has the
    /// focus, the diagram or the table of the tab, or the current schema of the console.
    fn diagram_target(&self) -> Result<Target, &'static str> {
        if self.navigator_focused {
            let row = self.navigator.current(&self.connections);
            return self.row_diagram(&row.ok_or(text::NO_SELECTED_CONNECTION)?);
        }
        if let Some(tab) = self.diagram() {
            return Ok(Target::Open(tab.connection, tab.focus.clone()));
        }
        let (connection, table, schema) = match self.table() {
            Some(tab) => {
                let rows = tab
                    .structure
                    .as_ref()
                    .is_none_or(|structure| structure.scope == Scope::Relation);
                (tab.connection, rows.then(|| tab.relation.clone()), None)
            }
            None => {
                let console = self.console();
                let connection = console.connection.ok_or(text::NO_SELECTED_CONNECTION)?;
                (connection, None, console.schema.clone())
            }
        };
        let dialect = self.connection(connection).map(|item| item.dialect);
        if dialect.is_none_or(|dialect| dialect == Dialect::Redis) {
            return Err(text::DIAGRAM_REFUSED);
        }
        match table {
            Some(table) => self.focus_target(connection, Focus::Table(table)),
            None => self.schema_target(connection, schema),
        }
    }

    /// Returns true when `Ctrl+W D` opens a new diagram or selects another tab.
    pub(super) fn can_open_diagram_here(&self) -> bool {
        self.diagram_active.is_none() && self.diagram_target().is_ok()
    }

    /// Opens the diagram that `Ctrl+W D` names.
    pub(super) fn open_diagram_here(&mut self) {
        let target = self.diagram_target();
        self.go_to_diagram(target);
    }

    /// Opens a diagram, or reads the catalog that it needs first. Without a diagram, the
    /// status line gives the reason.
    fn go_to_diagram(&mut self, target: Result<Target, &'static str>) {
        match target {
            Ok(Target::Open(connection, focus)) => self.open_diagram(connection, focus),
            Ok(Target::Wait(connection)) => {
                if !self.pending_diagrams.contains(&connection) {
                    self.pending_diagrams.push(connection);
                }
                self.ensure_catalog(connection);
            }
            Err(reason) => self.status = Some(reason.to_owned()),
        }
    }

    /// Opens the diagram that waited for the catalog of a connection. A failed read
    /// already shows its reason. The other waiting diagrams keep their places.
    pub(super) fn open_pending_diagram(&mut self, connection: ConnectionId, loaded: bool) {
        let Some(index) = self
            .pending_diagrams
            .iter()
            .position(|id| *id == connection)
        else {
            return;
        };
        self.pending_diagrams.remove(index);
        if loaded {
            let target = self.schema_target(connection, None);
            self.go_to_diagram(target);
        }
    }

    /// Opens a diagram tab. A diagram that is open only gets the focus. For a source
    /// without tables, the status line gives the reason.
    pub(super) fn open_diagram(&mut self, connection: ConnectionId, focus: Focus) {
        if self.config_blocked() {
            return;
        }
        let Some(dialect) = self.connection(connection).map(|item| item.dialect) else {
            return;
        };
        if diagram_sql(dialect, focus.schema()).is_none() {
            self.status = Some(text::DIAGRAM_REFUSED.to_owned());
            return;
        }
        if let Some(index) = self
            .diagrams
            .iter()
            .position(|tab| tab.is(connection, &focus))
        {
            self.select_diagram(index);
            self.navigator_focused = false;
            return;
        }
        let Some(id) = self
            .manager
            .as_mut()
            .and_then(|manager| manager.open_console(connection))
        else {
            return;
        };
        let opened = self.next_tab_place();
        self.diagrams
            .push(DiagramTab::new(id, connection, focus, opened));
        self.select_diagram(self.diagrams.len() - 1);
        self.navigator_focused = false;
        self.read_diagram();
    }

    pub(super) fn select_diagram(&mut self, index: usize) {
        if index < self.diagrams.len() {
            self.console_mut().editor.close_suggestions();
            if self.diagram_active != Some(index) {
                self.leave_tab();
            }
            self.table_active = None;
            self.compare_active = None;
            self.diagram_active = Some(index);
        }
    }

    /// Reads the diagram on screen again.
    pub(super) fn read_diagram(&mut self) {
        let Some(tab) = self.diagram() else { return };
        if tab.is_busy() {
            self.status = Some(text::TABLE_BUSY.to_owned());
            return;
        }
        let (console, connection) = (tab.id, tab.connection);
        let dialect = self.connection(connection).map(|item| item.dialect);
        let Some(statements) = dialect.and_then(|dialect| tab.statements(dialect)) else {
            return;
        };
        if self.config_blocked() {
            return;
        }
        self.authorize(
            connection,
            Use::Diagram {
                console,
                connection,
                statements,
            },
        );
    }

    /// Starts the read that the user authorized. The tab must still be open.
    pub(super) fn start_diagram_read(
        &mut self,
        console: crate::exec::ConsoleId,
        connection: ConnectionId,
        statements: Vec<String>,
    ) {
        let Some(manager) = &self.manager else { return };
        let Some(tab) = self
            .diagrams
            .iter_mut()
            .find(|tab| tab.id == console && tab.connection == connection)
        else {
            return;
        };
        // The diagram needs each row, so only the budget limits the read.
        let options = RunOptions {
            row_cap: Some(usize::MAX),
            ..RunOptions::default()
        };
        let execution = manager.run(console, statements, options);
        tab.started(execution);
    }

    fn stop_diagram(&mut self) {
        let Some(index) = self.diagram_active else {
            return;
        };
        if self.cancel_auth_console(self.diagrams[index].id) {
            return;
        }
        let tab = &mut self.diagrams[index];
        if !tab.is_busy() {
            return;
        }
        if let Some(manager) = &self.manager {
            manager.stop(tab.id);
            tab.phase = Phase::Stopping;
        }
    }

    /// Closes a diagram tab, and the rows of a read in progress leave the budget. The tab
    /// at the same place of the tab row takes the screen.
    pub(super) fn close_diagram(&mut self, index: usize) {
        if index >= self.diagrams.len() {
            return;
        }
        let place = self
            .tab_order()
            .iter()
            .position(|tab| *tab == Tab::Diagram(index));
        self.cancel_auth_console(self.diagrams[index].id);
        let mut tab = self.diagrams.remove(index);
        tab.release(&self.budget);
        if let Some(manager) = &mut self.manager {
            manager.close_console(tab.id);
        }
        match self.diagram_active {
            Some(active) if active == index => {
                self.diagram_active = None;
                let order = self.tab_order();
                if order.is_empty() {
                    self.focus_navigator();
                } else if let Some(place) = place {
                    self.select_tab(place.min(order.len() - 1));
                }
            }
            Some(active) if active > index => self.diagram_active = Some(active - 1),
            _ => {}
        }
    }

    /// Closes the diagram tabs of a connection that the user removed.
    pub(super) fn close_diagrams_of(&mut self, connection: ConnectionId) {
        for index in (0..self.diagrams.len()).rev() {
            if self.diagrams[index].connection == connection {
                self.close_diagram(index);
            }
        }
    }

    /// Applies an action to the diagram on screen. It returns false for an action that
    /// does not belong to the diagram.
    pub(super) fn perform_on_diagram(&mut self, action: Action) -> bool {
        let Some(index) = self.diagram_active else {
            return false;
        };
        let tab = &mut self.diagrams[index];
        match action {
            Action::DiagramUp => tab.select(Direction::Up),
            Action::DiagramDown => tab.select(Direction::Down),
            Action::DiagramLeft => tab.select(Direction::Left),
            Action::DiagramRight => tab.select(Direction::Right),
            Action::DiagramPanUp => tab.pan_step(Direction::Up),
            Action::DiagramPanDown => tab.pan_step(Direction::Down),
            Action::DiagramPanLeft => tab.pan_step(Direction::Left),
            Action::DiagramPanRight => tab.pan_step(Direction::Right),
            Action::DiagramPageUp => tab.page(false),
            Action::DiagramPageDown => tab.page(true),
            Action::DiagramKeysOnly => tab.toggle_keys_only(),
            _ => return self.perform_on_diagram_tab(action, index),
        }
        true
    }

    /// Applies an action of the diagram on screen that acts outside its picture.
    fn perform_on_diagram_tab(&mut self, action: Action, index: usize) -> bool {
        let tab = &self.diagrams[index];
        match action {
            Action::DiagramReload => self.read_diagram(),
            Action::DiagramCopy => {
                if !tab.model.entities.is_empty() {
                    let mermaid = tab.mermaid();
                    self.copy(&mermaid);
                }
            }
            Action::DiagramOpen | Action::DiagramStructure => {
                let connection = tab.connection;
                let Some(relation) = tab.selected_relation() else {
                    return true;
                };
                if action == Action::DiagramOpen {
                    self.open_relation(connection, relation, false, None);
                } else {
                    let name = relation.relation.clone();
                    self.open_structure(connection, Scope::Relation, relation, name);
                }
            }
            Action::Stop => self.stop_diagram(),
            Action::CloseTab => self.close_diagram(index),
            // A diagram has no editor, no results, no transaction, and no schema choice.
            Action::Run
            | Action::RunScript
            | Action::Explain
            | Action::ExplainAnalyze
            | Action::FormatSql
            | Action::ToggleComment
            | Action::OpenHistory
            | Action::ToggleMode
            | Action::Commit
            | Action::Rollback
            | Action::ChooseSchema
            | Action::RenameConsole
            | Action::ChangeConnection
            | Action::FocusUp
            | Action::FocusDown => {}
            action if action.region() == Region::Grid => {}
            _ => return false,
        }
        true
    }

    /// Selects the box under a click and starts a drag of the view.
    pub(super) fn on_diagram_press(&mut self, mouse: MouseEvent, position: Position) {
        let area = self.layout.diagram;
        if !area.contains(position) {
            return;
        }
        self.navigator_focused = false;
        if let Some(tab) = self.diagram_mut() {
            tab.select_at(mouse.column - area.x, mouse.row - area.y);
        }
        self.drag = Some(Drag::Diagram {
            column: mouse.column,
            row: mouse.row,
        });
    }

    /// Moves the view with the mouse, so the picture follows the pointer.
    pub(super) fn drag_diagram(&mut self, mouse: MouseEvent) {
        let Some(Drag::Diagram { column, row }) = self.drag else {
            return;
        };
        if let Some(tab) = self.diagram_mut() {
            tab.pan(
                i64::from(column) - i64::from(mouse.column),
                i64::from(row) - i64::from(mouse.row),
            );
        }
        self.drag = Some(Drag::Diagram {
            column: mouse.column,
            row: mouse.row,
        });
    }

    /// Moves the view with the wheel. It returns false when the pointer is not on the
    /// picture.
    pub(super) fn on_diagram_wheel(&mut self, mouse: MouseEvent, position: Position) -> bool {
        if !self.layout.diagram.contains(position) {
            return false;
        }
        let Some(tab) = self.diagram_mut() else {
            return false;
        };
        match mouse.kind {
            MouseEventKind::ScrollUp => tab.pan(0, -WHEEL_ROWS),
            MouseEventKind::ScrollDown => tab.pan(0, WHEEL_ROWS),
            MouseEventKind::ScrollLeft => tab.pan(-WHEEL_COLUMNS, 0),
            MouseEventKind::ScrollRight => tab.pan(WHEEL_COLUMNS, 0),
            _ => {}
        }
        true
    }
}
