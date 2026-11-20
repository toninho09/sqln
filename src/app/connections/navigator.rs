//! Navigator actions: templates, the tree filter, expansion, and opening consoles.

use super::*;

impl App {
    pub(in crate::app) fn on_navigator_action(&mut self, action: Action) {
        if action == Action::NavigatorAdd {
            if !self.config_blocked() {
                let mut form = ConnectionForm::new_in(self.has_project());
                form.themes = self.appearance.custom_themes.clone();
                self.modals.form = Some(form);
            }
            return;
        }
        if action == Action::NavigatorScripts {
            self.open_orphans();
            return;
        }
        if action == Action::NavigatorFilter {
            self.focus_navigator();
            if self.navigator_focused {
                self.navigator.open_filter();
            }
            return;
        }
        if action == Action::NavigatorClearFilter {
            self.navigator.clear_filter(&self.connections);
            self.refresh_tree_consoles();
            return;
        }
        let Some(row) = self.navigator.current(&self.connections) else {
            self.status = Some(text::NO_SELECTED_CONNECTION.to_owned());
            return;
        };
        if row.kind == RowKind::Folder
            && !matches!(
                action,
                Action::NavigatorUp
                    | Action::NavigatorDown
                    | Action::NavigatorCollapse
                    | Action::NavigatorExpand
                    | Action::NavigatorOpen
            )
        {
            self.on_folder_action(action, &row);
            return;
        }
        let id = self.connections[row.connection].id;
        match action {
            Action::NavigatorUp => self.navigator.move_by(-1, &self.connections),
            Action::NavigatorDown => self.navigator.move_by(1, &self.connections),
            Action::NavigatorCollapse => self.collapse(&row),
            Action::NavigatorExpand => self.expand(&row),
            Action::NavigatorOpen if row.kind == RowKind::Connection => self.open_console(id),
            Action::NavigatorOpen if row.kind == RowKind::Console => self.open_tree_console(&row),
            Action::NavigatorOpen if is_relation(&row) => self.open_table(&row),
            Action::NavigatorOpen if matches!(row.kind, RowKind::Object(_)) => {
                self.open_row_definition(&row);
            }
            Action::NavigatorOpen => self.toggle(&row),
            Action::NavigatorNewConsole => self.request_console(id),
            Action::NavigatorStructure => self.open_row_structure(&row),
            Action::NavigatorDiagram => self.open_row_diagram(&row),
            Action::NavigatorCompare => self.open_row_compare(&row),
            Action::NavigatorSessions => self.open_row_sessions(&row),
            Action::NavigatorGenerate => self.open_templates(&row),
            Action::NavigatorImport => self.import_row(&row),
            Action::NavigatorCopyName => {
                let object = row.object(&self.connections);
                let name = object.map_or_else(|| row.name(&self.connections), |item| &item.name);
                let name = name.to_owned();
                self.copy(&name);
            }
            Action::NavigatorRefresh if row.kind == RowKind::Console => {
                self.rename_tree_console(&row);
            }
            Action::NavigatorRefresh => {
                self.refresh_tree_consoles();
                if !self.ask_new_password(id) {
                    self.reload_catalog(id);
                }
            }
            Action::NavigatorDeletePassword => self.confirm_password_deletion(id),
            Action::NavigatorDisconnect => self.disconnect(id),
            Action::NavigatorEdit => self.edit_connection(id),
            Action::NavigatorDelete if row.kind == RowKind::Console => {
                self.delete_tree_console(&row);
            }
            Action::NavigatorDelete => self.confirm_removal(id),
            Action::NavigatorMoveUp => self.move_tree_item(&row, true),
            Action::NavigatorMoveDown => self.move_tree_item(&row, false),
            Action::NavigatorMoveToFolder => self.open_folder_list(&row),
            _ => {}
        }
    }

    /// Returns the request of the templates of a row: a table, a view, or the table of a
    /// column, with SQL text.
    pub(super) fn row_templates(&self, row: &Row) -> Option<(SqlRequest, &'static [Template])> {
        let item = &self.connections[row.connection];
        if item.dialect.is_command_line()
            || matches!(row.kind, RowKind::Connection | RowKind::Schema)
        {
            return None;
        }
        let relation = row.relation_ref(&self.connections)?;
        let found = item
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.relation(Some(&relation.schema), &relation.relation));
        let view = found.is_some_and(|(_, found)| found.kind == RelationKind::View);
        let templates: &[Template] = if view {
            &Template::READS
        } else {
            &Template::ALL
        };
        let request = SqlRequest {
            connection: item.id,
            relation,
        };
        Some((request, templates))
    }

    /// Returns true when the row of the navigator has templates or a DDL.
    pub(in crate::app) fn can_generate(&self) -> bool {
        self.navigator
            .current(&self.connections)
            .is_some_and(|row| {
                self.row_templates(&row).is_some() || self.row_definition(&row).is_some()
            })
    }

    /// Opens the list of SQL templates of a table, of a view, or of the table of a column,
    /// with the DDL entry last. An object has only the DDL entry.
    pub(super) fn open_templates(&mut self, row: &Row) {
        if !self.can_generate() || self.config_blocked() {
            return;
        }
        let (request, templates) = match self.row_templates(row) {
            Some((request, templates)) => (Some(request), templates.to_vec()),
            None => (None, Vec::new()),
        };
        let definition = self.row_definition(row);
        let mut items: Vec<String> = templates
            .iter()
            .map(|template| text::template_name(*template).to_owned())
            .collect();
        if definition.is_some() {
            items.push(text::TEMPLATE_DDL.to_owned());
        }
        self.modals.picker = Some(Picker {
            kind: PickerKind::Generate {
                request,
                templates,
                definition,
            },
            title: text::TITLE_GENERATE.to_owned(),
            items,
            cursor: 0,
            loading: false,
        });
    }

    /// Writes a template. A template with columns first reads them.
    pub(super) fn start_template(&mut self, request: SqlRequest, template: Template) {
        let found = self.catalog(request.connection).and_then(|catalog| {
            catalog.relation(Some(&request.relation.schema), &request.relation.relation)
        });
        let missing = found.is_some_and(|(_, found)| found.columns == Columns::NotLoaded);
        if template.needs_columns() && missing {
            let (connection, relation) = (request.connection, request.relation.clone());
            self.pending.sql = Some((request, template));
            self.request_columns(connection, relation);
        } else {
            self.write_sql(&request, template);
        }
    }

    /// Selects the console of the connection, and inserts the text at its cursor. It does
    /// not run the text, as the query history does not.
    pub(super) fn write_sql(&mut self, request: &SqlRequest, template: Template) {
        let SqlRequest {
            connection,
            relation,
        } = request;
        let index = if self.console().connection == Some(*connection) {
            Some(self.tabs.active)
        } else {
            self.tabs
                .consoles
                .iter()
                .position(|console| console.connection == Some(*connection))
        };
        let columns = self
            .catalog(*connection)
            .and_then(|catalog| catalog.relation(Some(&relation.schema), &relation.relation))
            .and_then(|(_, found)| match &found.columns {
                Columns::Loaded(columns) => Some(columns.clone()),
                Columns::NotLoaded => None,
            })
            .unwrap_or_default();
        if template.needs_columns() && columns.is_empty() {
            self.status = Some(text::NO_COLUMNS_TO_LIST.to_owned());
            return;
        }
        match index {
            Some(index) => self.select_console(index),
            None => self.new_console(*connection),
        }
        self.catalog_for(self.tabs.active);
        let Some(item) = self.connection(*connection) else {
            return;
        };
        let current = item.catalog.as_ref().is_some_and(|catalog| {
            catalog
                .schema(&relation.schema)
                .is_some_and(|schema| catalog.is_current(schema))
        });
        let schema = (!current).then_some(relation.schema.as_str());
        let name = qualified_name(schema, &relation.relation, item.dialect);
        let sql = generate(template, item.dialect, &name, &columns);
        self.leave_navigator();
        self.on_paste(&sql);
    }

    /// Writes in the field of the filter of the tree. The tree under the field changes
    /// with each key, and `Enter` opens the table of the cursor.
    pub(in crate::app) fn on_tree_filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.navigator.cancel_filter(),
            KeyCode::Tab => self.navigator.leave_filter_field(),
            KeyCode::Up => self.step_filtered(false),
            KeyCode::Down => self.step_filtered(true),
            KeyCode::Enter => self.accept_tree_filter(),
            _ => self.navigator.on_filter_key(key, &self.connections),
        }
    }

    pub(super) fn step_filtered(&mut self, forward: bool) {
        if self.navigator.is_filtered() {
            self.navigator.step_relation(forward, &self.connections);
        } else {
            let rows = if forward { 1 } else { -1 };
            self.navigator.move_by(rows, &self.connections);
        }
    }

    /// Opens the table of the cursor. The filter stays, so the next table is near.
    pub(super) fn accept_tree_filter(&mut self) {
        let row = self.navigator.current(&self.connections);
        let object = |row: &Row| matches!(row.kind, RowKind::Object(_));
        match row.filter(|row| is_relation(row) || object(row)) {
            Some(row) if self.navigator.is_filtered() => {
                self.navigator.leave_filter_field();
                if object(&row) {
                    self.open_row_definition(&row);
                } else {
                    self.open_table(&row);
                }
            }
            _ if !self.navigator.is_filtered() => self.navigator.leave_filter_field(),
            _ => {}
        }
    }

    pub(in crate::app) fn toggle(&mut self, row: &Row) {
        if self.navigator.is_filtered() {
            return;
        }
        if self.navigator.is_open(row, &self.connections) {
            self.navigator.collapse(row, &self.connections);
        } else {
            self.expand(row);
        }
    }

    pub(super) fn expand(&mut self, row: &Row) {
        if !Navigator::is_branch(row) || self.navigator.is_filtered() {
            return;
        }
        if self.navigator.is_open(row, &self.connections) {
            self.navigator.move_by(1, &self.connections);
            return;
        }
        self.navigator.expand(row, &self.connections);
        let id = self.connections[row.connection].id;
        match row.kind {
            RowKind::Connection => {
                self.refresh_tree_consoles();
                self.ensure_catalog(id);
            }
            RowKind::Table | RowKind::View => self.read_columns(row, id),
            _ => {}
        }
    }

    pub(super) fn read_columns(&mut self, row: &Row, id: ConnectionId) {
        let Some(relation) = row.relation_ref(&self.connections) else {
            return;
        };
        let catalog = self.connections[row.connection].catalog.as_ref();
        let found = catalog
            .and_then(|catalog| catalog.relation(Some(&relation.schema), &relation.relation));
        if found.is_some_and(|(_, item)| item.columns == Columns::NotLoaded) {
            self.request_columns(id, relation);
        }
    }

    pub(super) fn collapse(&mut self, row: &Row) {
        if self.navigator.is_filtered() {
            return;
        }
        if self.navigator.is_open(row, &self.connections) {
            self.navigator.collapse(row, &self.connections);
        } else {
            self.navigator.select_parent(row, &self.connections);
        }
    }

    /// Selects a console of the connection, or it asks for one.
    pub(super) fn open_console(&mut self, id: ConnectionId) {
        match self
            .tabs
            .consoles
            .iter()
            .position(|console| console.connection == Some(id))
        {
            Some(index) => {
                self.select_console(index);
                self.leave_navigator();
            }
            None => self.request_console(id),
        }
    }

    pub(in crate::app) fn leave_navigator(&mut self) {
        self.navigator_focused = false;
        self.console_mut().focus = Focus::Editor;
    }
}
