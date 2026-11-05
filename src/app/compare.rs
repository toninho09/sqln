//! Compare tabs: the dialog, the reads of the two sides, the keys, the script, and their
//! place in the tab row.

use crossterm::event::{KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use super::App;
use super::auth::Use;
use super::tables::Tab;
use crate::catalog::RelationKind;
use crate::compare::form::{Choice, CompareForm, CompareRequest, SchemaChoice};
use crate::compare::tab::{CompareTab, SideRead};
use crate::compare::{Kind, Side};
use crate::connection::CatalogState;
use crate::console::Phase;
use crate::diagram::Focus;
use crate::exec::{ConnectionId, ConsoleId, RunOptions};
use crate::input::Action;
use crate::navigator::{Row, RowKind};
use crate::sql::Dialect;
use crate::structure::Scope;
use crate::ui::text;

/// The source that a row gives to the dialog: a connection, a schema, and a table.
type Source = (ConnectionId, Option<String>, Option<String>);

/// What the dialog of a compare starts with.
enum Start {
    Source(Source),
    /// The kind, the sides, and the removals of an open compare.
    Again(Kind, [Side; 2], bool),
}

impl App {
    pub fn compares(&self) -> &[CompareTab] {
        &self.compares
    }

    /// Returns the compare tab on screen.
    pub fn compare(&self) -> Option<&CompareTab> {
        self.compare_active.map(|index| &self.compares[index])
    }

    pub(super) fn compare_mut(&mut self) -> Option<&mut CompareTab> {
        self.compare_active.map(|index| &mut self.compares[index])
    }

    pub fn compare_form(&self) -> Option<&CompareForm> {
        self.compare_form.as_ref()
    }

    /// Returns the source of a compare for a row of the explorer, or the reason that the
    /// row has none. A column gives its table.
    fn row_compare(&self, row: &Row) -> Result<Source, &'static str> {
        let item = &self.connections[row.connection];
        if item.dialect == Dialect::Redis {
            return Err(text::COMPARE_REFUSED);
        }
        match row.kind {
            RowKind::Connection => Ok((item.id, item.default_schema.clone(), None)),
            RowKind::Schema => Ok((item.id, Some(row.name(&self.connections).to_owned()), None)),
            RowKind::Table | RowKind::Column => {
                let relation = row
                    .relation_ref(&self.connections)
                    .ok_or(text::COMPARE_REFUSED)?;
                Ok((item.id, Some(relation.schema), Some(relation.relation)))
            }
            RowKind::View => Err(text::COMPARE_VIEW),
            RowKind::Consoles | RowKind::Console => Err(text::COMPARE_CONSOLE),
            RowKind::Group(_) | RowKind::Object(_) => Err(text::NOT_FOR_OBJECT),
        }
    }

    /// Returns true when the row of the explorer can open the dialog of a compare.
    pub(super) fn can_compare_row(&self) -> bool {
        self.navigator
            .current(&self.connections)
            .is_some_and(|row| self.row_compare(&row).is_ok())
    }

    /// Opens the dialog of a compare with the source of a row.
    pub(super) fn open_row_compare(&mut self, row: &Row) {
        let start = self.row_compare(row).map(Start::Source);
        self.start_compare_form(start);
    }

    /// Returns what `Ctrl+W v` compares: the row of the explorer when it has the focus,
    /// the sides of a compare tab, the focus of a diagram, the table of a table tab, or the
    /// current schema of the console.
    fn compare_here(&self) -> Result<Start, &'static str> {
        if self.navigator_focused {
            let row = self.navigator.current(&self.connections);
            return self
                .row_compare(&row.ok_or(text::NO_SELECTED_CONNECTION)?)
                .map(Start::Source);
        }
        if let Some(tab) = self.compare() {
            let sides = [tab.sides[0].side.clone(), tab.sides[1].side.clone()];
            return Ok(Start::Again(tab.kind, sides, tab.removals));
        }
        let (connection, schema, table) = if let Some(tab) = self.diagram() {
            match &tab.focus {
                Focus::Schema(schema) => (tab.connection, Some(schema.clone()), None),
                Focus::Table(table) => (
                    tab.connection,
                    Some(table.schema.clone()),
                    Some(table.relation.clone()),
                ),
            }
        } else if let Some(tab) = self.table() {
            let rows = tab
                .structure
                .as_ref()
                .is_none_or(|structure| structure.scope == Scope::Relation);
            let schema = (!tab.relation.schema.is_empty()).then(|| tab.relation.schema.clone());
            let table = rows.then(|| tab.relation.relation.clone());
            (tab.connection, schema, table)
        } else {
            let console = self.console();
            let connection = console.connection.ok_or(text::NO_SELECTED_CONNECTION)?;
            (connection, console.schema.clone(), None)
        };
        let item = self
            .connection(connection)
            .ok_or(text::NO_SELECTED_CONNECTION)?;
        if item.dialect == Dialect::Redis {
            return Err(text::COMPARE_REFUSED);
        }
        let schema = schema.or_else(|| item.default_schema.clone());
        Ok(Start::Source((connection, schema, table)))
    }

    /// Returns true when `Ctrl+W v` opens the dialog of a compare.
    pub(super) fn can_open_compare_here(&self) -> bool {
        self.compare_here().is_ok()
    }

    /// Opens the dialog that `Ctrl+W v` names.
    pub(super) fn open_compare_here(&mut self) {
        let start = self.compare_here();
        self.start_compare_form(start);
    }

    /// Opens the dialog of a compare, or gives the reason that it has none.
    fn start_compare_form(&mut self, start: Result<Start, &'static str>) {
        let choices = self.compare_choices();
        let form = match start {
            Ok(Start::Source((connection, schema, table))) => {
                CompareForm::new(choices, connection, schema, table)
            }
            Ok(Start::Again(kind, sides, removals)) => {
                CompareForm::again(choices, kind, &sides, removals)
            }
            Err(reason) => {
                self.status = Some(reason.to_owned());
                return;
            }
        };
        self.compare_form = Some(form);
        self.sync_compare_form();
    }

    /// Lists the connections that a compare can use. Redis has no tables.
    fn compare_choices(&self) -> Vec<Choice> {
        self.connections
            .iter()
            .filter(|item| item.dialect != Dialect::Redis)
            .map(|item| Choice {
                id: item.id,
                name: item.name.clone(),
                dialect: item.dialect,
            })
            .collect()
    }

    /// Gives the dialog the schemas and the tables of each side. A connection without its
    /// catalog reads it, and the dialog shows the schemas when they arrive. Views stay out,
    /// because a compare refuses them.
    pub(super) fn sync_compare_form(&mut self) {
        for side in 0..2 {
            let Some(form) = &self.compare_form else {
                return;
            };
            if form.sides[side].schemas.is_some() {
                continue;
            }
            let Some(id) = form.choice(side).map(|choice| choice.id) else {
                continue;
            };
            let Some(item) = self.connection(id) else {
                continue;
            };
            match (&item.catalog, item.catalog_state) {
                (Some(catalog), CatalogState::Loaded) => {
                    let schemas = catalog.schemas.iter().map(|schema| SchemaChoice {
                        name: schema.name.clone(),
                        tables: schema
                            .relations
                            .iter()
                            .filter(|relation| relation.kind == RelationKind::Table)
                            .map(|relation| relation.name.clone())
                            .collect(),
                    });
                    let schemas = schemas.collect();
                    let default = item.default_schema.clone();
                    if let Some(form) = &mut self.compare_form {
                        form.set_schemas(side, schemas, default.as_deref());
                    }
                }
                (_, CatalogState::Failed) => {
                    if let Some(form) = &mut self.compare_form {
                        form.set_schemas(side, Vec::new(), None);
                    }
                }
                _ => self.ensure_catalog(id),
            }
        }
    }

    pub(super) fn on_compare_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        let Some(form) = &mut self.compare_form else {
            return;
        };
        match form.on_key(key, vim) {
            CompareRequest::None => self.sync_compare_form(),
            CompareRequest::Cancel => self.compare_form = None,
            CompareRequest::Compare => self.confirm_compare(),
        }
    }

    pub(super) fn on_compare_mouse(&mut self, mouse: MouseEvent) {
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            self.on_compare_click(Position::new(mouse.column, mouse.row));
        }
    }

    /// Acts on a click. A row of the list comes first, because the list covers fields.
    fn on_compare_click(&mut self, position: Position) {
        let layout = &self.layout.compare_form;
        let row = layout
            .list
            .filter(|(area, _)| area.contains(position))
            .map(|(area, first)| first + usize::from(position.y - area.y));
        let field = layout
            .fields
            .iter()
            .find(|(_, area)| area.contains(position))
            .map(|(field, _)| *field);
        let Some(form) = &mut self.compare_form else {
            return;
        };
        let request = match (row, field) {
            (Some(row), _) => {
                form.pick(row);
                CompareRequest::None
            }
            (None, Some(field)) => {
                form.focus_on(field);
                form.press()
            }
            (None, None) => return,
        };
        match request {
            CompareRequest::None => self.sync_compare_form(),
            CompareRequest::Cancel => self.compare_form = None,
            CompareRequest::Compare => self.confirm_compare(),
        }
    }

    /// Checks the sides of the dialog and opens the compare. A wrong field keeps the
    /// dialog open with the reason.
    fn confirm_compare(&mut self) {
        let Some(form) = &self.compare_form else {
            return;
        };
        let (kind, removals) = (form.kind, form.removals);
        let checked = form.sides().and_then(|sides| {
            self.check_compare_table(&sides[0])?;
            Ok(sides)
        });
        match checked {
            Ok(sides) => {
                self.compare_form = None;
                self.open_compare(kind, sides, removals);
            }
            Err(note) => {
                if let Some(form) = &mut self.compare_form {
                    form.note = Some(note);
                }
            }
        }
    }

    /// Refuses a source table that the catalog does not hold, and a view.
    fn check_compare_table(&self, side: &Side) -> Result<(), String> {
        let Some(table) = &side.table else {
            return Ok(());
        };
        let catalog = self
            .connection(side.connection)
            .and_then(|item| item.catalog.as_ref());
        match catalog.and_then(|catalog| catalog.relation(Some(&side.schema), table)) {
            Some((_, relation)) if relation.kind == RelationKind::View => {
                Err(text::COMPARE_VIEW.to_owned())
            }
            Some(_) => Ok(()),
            None => Err(text::compare_no_table(table)),
        }
    }

    /// Opens a compare tab and reads both sides. A compare that is open reads again.
    pub(super) fn open_compare(&mut self, kind: Kind, sides: [Side; 2], removals: bool) {
        if self.config_blocked() {
            return;
        }
        self.navigator_focused = false;
        if let Some(index) = self.compares.iter().position(|tab| tab.is(kind, &sides)) {
            self.select_compare(index);
            self.compares[index].removals = removals;
            self.read_compare();
            return;
        }
        let mut reads = Vec::new();
        for side in sides {
            let Some(item) = self.connection(side.connection) else {
                return;
            };
            let (dialect, name) = (item.dialect, item.name.clone());
            let Some(id) = self
                .manager
                .as_mut()
                .and_then(|manager| manager.open_console(side.connection))
            else {
                for read in &reads {
                    self.close_compare_console(read);
                }
                return;
            };
            reads.push(SideRead::new(id, side, dialect, name));
        }
        let Ok(sides) = <[SideRead; 2]>::try_from(reads) else {
            return;
        };
        let opened = self.next_tab_place();
        self.compares
            .push(CompareTab::new(opened, kind, sides, removals));
        self.select_compare(self.compares.len() - 1);
        self.read_compare();
    }

    fn close_compare_console(&mut self, read: &SideRead) {
        if let Some(manager) = &mut self.manager {
            manager.close_console(read.id);
        }
    }

    pub(super) fn select_compare(&mut self, index: usize) {
        if index < self.compares.len() {
            self.console_mut().editor.close_suggestions();
            if self.compare_active != Some(index) {
                self.leave_tab();
            }
            self.table_active = None;
            self.diagram_active = None;
            self.compare_active = Some(index);
        }
    }

    /// Reads both sides of the compare on screen again.
    pub(super) fn read_compare(&mut self) {
        let Some(index) = self.compare_active else {
            return;
        };
        let tab = &self.compares[index];
        if tab.is_busy() {
            self.status = Some(text::TABLE_BUSY.to_owned());
            return;
        }
        let (Some(first), Some(second)) = (tab.statements(0), tab.statements(1)) else {
            self.status = Some(text::COMPARE_REFUSED.to_owned());
            return;
        };
        let uses: Vec<(ConnectionId, Use)> = tab
            .sides
            .iter()
            .zip([first, second])
            .map(|(read, statements)| {
                let connection = read.side.connection;
                let action = Use::Compare {
                    console: read.id,
                    connection,
                    statements,
                    schema: read.side.schema.clone(),
                    untimed: false,
                };
                (connection, action)
            })
            .collect();
        if self.config_blocked() {
            return;
        }
        for read in &self.compares[index].sides {
            self.agent.release_retired(read.id);
        }
        let budget = self.budget.clone();
        self.compares[index].begin(&budget);
        for (connection, action) in uses {
            self.authorize(connection, action);
        }
    }

    /// Starts the next step of a data compare after an event of one of its tasks: the
    /// rows of the source, and then the rows of the target. Each step passes the gate of
    /// the password, and a read of rows has no time limit.
    pub(super) fn continue_compare(&mut self, console: ConsoleId) {
        if self.agent.task_retired(console) {
            self.cancel_compare_side(console);
            return;
        }
        let Some(tab) = self.compares.iter_mut().find(|tab| tab.owns(console)) else {
            return;
        };
        let Some((side, statements)) = tab.take_next() else {
            return;
        };
        let read = &tab.sides[side];
        let connection = read.side.connection;
        let action = Use::Compare {
            console: read.id,
            connection,
            statements,
            schema: read.side.schema.clone(),
            untimed: true,
        };
        self.authorize(connection, action);
    }

    /// Starts the read of one side that the user authorized. The tab must still be open.
    pub(super) fn start_compare_read(
        &mut self,
        console: ConsoleId,
        connection: ConnectionId,
        statements: Vec<String>,
        (schema, untimed): (String, bool),
    ) {
        let Some(manager) = &self.manager else { return };
        let Some(tab) = self.compares.iter_mut().find(|tab| tab.owns(console)) else {
            return;
        };
        if !tab
            .sides
            .iter()
            .any(|read| read.id == console && read.side.connection == connection)
        {
            return;
        }
        // The compare needs each row, so only the budget limits the read. On PostgreSQL,
        // a `search_path` with only the schema keeps the names of the schema without the
        // schema, and the names of other schemas with their schema.
        let options = RunOptions {
            row_cap: Some(usize::MAX),
            schema: Some(schema),
            only_schema: true,
            untimed,
            ..RunOptions::default()
        };
        let execution = self.agent.run(manager, console, statements, options);
        tab.started(console, execution);
    }

    /// Ends the read of a side whose password request the user canceled.
    pub(super) fn cancel_compare_side(&mut self, console: ConsoleId) {
        let budget = self.budget.clone();
        if let Some(tab) = self.compares.iter_mut().find(|tab| tab.owns(console)) {
            tab.canceled(console, &budget);
        }
    }

    fn stop_compare(&mut self) {
        let Some(index) = self.compare_active else {
            return;
        };
        if self.cancel_agent_console(self.compares[index].sides[0].id) {
            return;
        }
        let ids: Vec<ConsoleId> = self.compares[index]
            .sides
            .iter()
            .map(|read| read.id)
            .collect();
        for id in &ids {
            self.cancel_auth_console(*id);
        }
        let tab = &mut self.compares[index];
        if !tab.is_busy() {
            return;
        }
        if let Some(manager) = &self.manager {
            for id in ids {
                manager.stop(id);
            }
            tab.phase = Phase::Stopping;
        }
    }

    /// Closes a compare tab, and its rows leave the budget. The tab at the same place of
    /// the tab row takes the screen.
    pub(super) fn close_compare(&mut self, index: usize) {
        if index >= self.compares.len() {
            return;
        }
        self.cancel_agent_console(self.compares[index].sides[0].id);
        for read in &self.compares[index].sides {
            self.agent.forget_console(read.id);
        }
        let place = self
            .tab_order()
            .iter()
            .position(|tab| *tab == Tab::Compare(index));
        let ids: Vec<ConsoleId> = self.compares[index]
            .sides
            .iter()
            .map(|read| read.id)
            .collect();
        for id in &ids {
            self.cancel_auth_console(*id);
        }
        let mut tab = self.compares.remove(index);
        tab.release(&self.budget);
        if let Some(manager) = &mut self.manager {
            for id in ids {
                manager.close_console(id);
            }
        }
        match self.compare_active {
            Some(active) if active == index => {
                self.compare_active = None;
                let order = self.tab_order();
                if order.is_empty() {
                    self.focus_navigator();
                } else if let Some(place) = place {
                    self.select_tab(place.min(order.len() - 1));
                }
            }
            Some(active) if active > index => self.compare_active = Some(active - 1),
            _ => {}
        }
    }

    /// Closes the compare tabs that use a connection that the user removed.
    pub(super) fn close_compares_of(&mut self, connection: ConnectionId) {
        for index in (0..self.compares.len()).rev() {
            let uses = self.compares[index]
                .sides
                .iter()
                .any(|read| read.side.connection == connection);
            if uses {
                self.close_compare(index);
            }
        }
    }

    /// Applies an action to the compare on screen. It returns false for an action that the
    /// grid of the compare shares with the other grids.
    pub(super) fn perform_on_compare(&mut self, action: Action) -> bool {
        let Some(index) = self.compare_active else {
            return false;
        };
        match action {
            Action::CompareScript => self.open_compare_script(),
            Action::CompareCopy => match self.compare_script() {
                Ok(script) => self.copy(&script),
                Err(reason) => self.status = Some(reason.to_owned()),
            },
            Action::GridReload => self.read_compare(),
            Action::Stop => self.stop_compare(),
            Action::CloseTab => self.close_compare(index),
            // A compare has no editor, no transaction, no filter, and one result.
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
            | Action::FocusDown
            | Action::GridImport
            | Action::GridCloseResult
            | Action::GridPreviousResult
            | Action::GridNextResult => {}
            action if action.is_table_only() => {}
            _ => return false,
        }
        true
    }

    /// Returns true when an action of the grid has an effect on the compare on screen.
    pub(super) fn compare_command_available(&self, action: Action) -> bool {
        let Some(tab) = self.compare() else {
            return false;
        };
        match action {
            _ if !compare_has(action) => false,
            Action::CompareScript | Action::CompareCopy => self.compare_script().is_ok(),
            Action::GridReload => !tab.is_busy(),
            Action::GridOpenRow => !tab.result.table.rows.is_empty(),
            Action::GridExport => self.can_export(),
            _ => tab.done,
        }
    }

    /// Returns the script of the compare on screen, or the reason that it has none.
    fn compare_script(&self) -> Result<String, &'static str> {
        let tab = self.compare().ok_or(text::COMPARE_NOT_READ)?;
        if !tab.done {
            return Err(text::COMPARE_NOT_READ);
        }
        if tab.count == 0 {
            return Err(text::COMPARE_NO_SCRIPT);
        }
        if tab.script.is_empty() {
            return Err(text::COMPARE_NO_SCRIPT_DIALECTS);
        }
        Ok(tab.script.clone())
    }

    /// Opens the script in a new console of the target. It does not run the script.
    fn open_compare_script(&mut self) {
        let script = match self.compare_script() {
            Ok(script) => script,
            Err(reason) => {
                self.status = Some(reason.to_owned());
                return;
            }
        };
        let Some(target) = self.compare().map(|tab| tab.sides[1].side.clone()) else {
            return;
        };
        self.open_text_console(target.connection, target.schema, &script);
    }
}

/// Returns true for an action of the grid that a compare has: not the import, the filter,
/// the pages, the sessions, or the list of results.
pub(super) fn compare_has(action: Action) -> bool {
    match action {
        Action::GridReload => true,
        Action::GridImport
        | Action::GridCloseResult
        | Action::GridPreviousResult
        | Action::GridNextResult => false,
        action => !action.is_table_only(),
    }
}
