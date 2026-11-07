//! Coordination of pending table changes, review, authorization, and safe close.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use super::auth::Use;
use super::{App, DialogAction};
use crate::console::Phase;
use crate::data_edit::form::{Body, EditForm};
use crate::data_edit::{DeleteToggle, Job, Outcome, Restore, SavedRow};
use crate::exec::{ConsoleId, ExecEvent, ExecutionEnd};
use crate::grid::Value;
use crate::input::{Chord, Lookup, Region};
use crate::sql::literal::is_number;
use crate::table::EditAfter;
use crate::ui::text;
use crate::ui::wrap::WrappedText;

impl App {
    /// The reasons that block every edit of the grid of a table tab.
    fn edit_base_refusal(&self) -> Option<String> {
        let tab = self.table()?;
        if tab.is_busy() {
            return Some(text::TABLE_BUSY.to_owned());
        }
        if tab.edit_uncertain {
            return Some(text::EDIT_COMMIT_UNKNOWN.to_owned());
        }
        if tab.is_view
            || tab.structure.is_some()
            || !tab.filters
            || self.is_read_only(tab.connection)
            || !self.connection(tab.connection).is_some_and(|connection| {
                connection.capabilities.data_edit && connection.capabilities.transactions
            })
        {
            return Some(text::EDIT_UNAVAILABLE.to_owned());
        }
        None
    }

    /// The reasons that block a new pending insert row.
    pub(super) fn new_row_refusal(&self) -> Option<String> {
        if self.table().is_none() {
            return Some(text::EDIT_UNAVAILABLE.to_owned());
        }
        if let Some(reason) = self.edit_base_refusal() {
            return Some(reason);
        }
        let tab = self.table()?;
        if let Some(metadata) = &tab.edit_metadata
            && !metadata.matches(&tab.result.table.columns)
        {
            return Some(text::EDIT_SCHEMA_CHANGED.to_owned());
        }
        None
    }

    /// The reasons that block the delete mark of the row of the cursor.
    pub(super) fn delete_row_refusal(&self) -> Option<String> {
        if self.table().is_none() {
            return Some(text::EDIT_UNAVAILABLE.to_owned());
        }
        if let Some(reason) = self.edit_base_refusal() {
            return Some(reason);
        }
        let tab = self.table()?;
        if tab.result.table.rows.is_empty() {
            return Some(text::EDIT_NO_CELL.to_owned());
        }
        if let Some(metadata) = &tab.edit_metadata
            && !metadata.matches(&tab.result.table.columns)
        {
            return Some(text::EDIT_SCHEMA_CHANGED.to_owned());
        }
        None
    }

    pub(super) fn edit_refusal(&self) -> Option<String> {
        if let Some(reason) = self.edit_base_refusal() {
            return Some(reason);
        }
        let tab = self.table()?;
        if tab.result.table.rows.is_empty() {
            return Some(text::EDIT_NO_CELL.to_owned());
        }
        let row = tab.result.view.row;
        if tab.result.edits.is_insert(row) {
            if let Some(metadata) = &tab.edit_metadata {
                return metadata
                    .editable_insert(&tab.result.table.columns, tab.result.view.col)
                    .err();
            }
            return None;
        }
        if tab.result.edits.deletes.contains_key(&row) {
            return Some(text::EDIT_ROW_DELETED.to_owned());
        }
        if let Some(metadata) = &tab.edit_metadata {
            let row = tab.result.table.rows.get(row)?;
            if let Err(reason) =
                metadata.editable(&tab.result.table.columns, row, tab.result.view.col)
            {
                return Some(reason);
            }
        }
        None
    }

    pub(super) fn can_edit_cell(&self) -> bool {
        self.table().is_some() && self.edit_refusal().is_none()
    }

    pub(super) fn pending_edit_guard(&mut self) -> bool {
        if self
            .table()
            .is_some_and(|tab| !tab.result.edits.is_empty() || tab.edit_uncertain)
        {
            self.status = Some(text::EDIT_RESOLVE.to_owned());
            return true;
        }
        false
    }

    pub(super) fn edit_cell(&mut self, null: bool) {
        if self.config_blocked() {
            return;
        }
        if let Some(reason) = self.edit_refusal() {
            self.status = Some(reason.to_owned());
            return;
        }
        let Some(tab) = self.table() else {
            self.status = Some(text::EDIT_UNAVAILABLE.to_owned());
            return;
        };
        let (id, connection, row, col) = (
            tab.id,
            tab.connection,
            tab.result.view.row,
            tab.result.view.col,
        );
        if tab.edit_metadata.is_none() {
            let Some(dialect) = self.connection(connection).map(|item| item.dialect) else {
                return;
            };
            let job = Job::Inspect {
                relation: tab.relation.clone(),
                dialect,
            };
            self.tables
                .iter_mut()
                .find(|tab| tab.id == id)
                .unwrap()
                .edit_after = Some(EditAfter::Cell { row, col, null });
            self.authorize(connection, Use::Edit { console: id, job });
        } else {
            self.open_cell_editor(id, row, col, null);
        }
    }

    /// Appends one pending insert row, after the metadata read when needed.
    pub(super) fn new_row(&mut self) {
        if self.config_blocked() {
            return;
        }
        if let Some(reason) = self.new_row_refusal() {
            self.status = Some(reason.to_owned());
            return;
        }
        let Some(tab) = self.table() else {
            self.status = Some(text::EDIT_UNAVAILABLE.to_owned());
            return;
        };
        let (id, connection) = (tab.id, tab.connection);
        if tab.edit_metadata.is_none() {
            let Some(dialect) = self.connection(connection).map(|item| item.dialect) else {
                return;
            };
            let job = Job::Inspect {
                relation: tab.relation.clone(),
                dialect,
            };
            self.tables
                .iter_mut()
                .find(|tab| tab.id == id)
                .unwrap()
                .edit_after = Some(EditAfter::NewRow);
            self.authorize(connection, Use::Edit { console: id, job });
        } else {
            self.append_new_row(id);
        }
    }

    fn append_new_row(&mut self, id: ConsoleId) {
        if self.table().is_none_or(|tab| tab.id != id) {
            return;
        }
        let budget = self.budget.clone();
        let Some(index) = self.tables.iter().position(|tab| tab.id == id) else {
            return;
        };
        let tab = &mut self.tables[index];
        match tab.result.edits.new_row(&mut tab.result.table, &budget) {
            Ok(row) => {
                let col = tab.result.view.col;
                tab.result.view.move_to(row, col, &tab.result.table, false);
                self.reset_table_search(index);
                self.status = Some(text::EDIT_ROW_ADDED.to_owned());
            }
            Err(reason) => self.status = Some(reason),
        }
    }

    /// Toggles the delete mark of the row of the cursor, after the metadata read when
    /// needed.
    pub(super) fn toggle_delete_row(&mut self) {
        if self.config_blocked() {
            return;
        }
        if let Some(reason) = self.delete_row_refusal() {
            self.status = Some(reason.to_owned());
            return;
        }
        let Some(tab) = self.table() else {
            self.status = Some(text::EDIT_UNAVAILABLE.to_owned());
            return;
        };
        let (id, connection, row) = (tab.id, tab.connection, tab.result.view.row);
        if tab.edit_metadata.is_none() {
            let Some(dialect) = self.connection(connection).map(|item| item.dialect) else {
                return;
            };
            let job = Job::Inspect {
                relation: tab.relation.clone(),
                dialect,
            };
            self.tables
                .iter_mut()
                .find(|tab| tab.id == id)
                .unwrap()
                .edit_after = Some(EditAfter::Delete { row });
            self.authorize(connection, Use::Edit { console: id, job });
        } else {
            self.toggle_delete_row_at(id, row);
        }
    }

    fn toggle_delete_row_at(&mut self, id: ConsoleId, row: usize) {
        if self.table().is_none_or(|tab| tab.id != id) {
            return;
        }
        let budget = self.budget.clone();
        let Some(index) = self.tables.iter().position(|tab| tab.id == id) else {
            return;
        };
        let tab = &mut self.tables[index];
        let result = tab
            .result
            .edits
            .toggle_delete(&mut tab.result.table, row, &budget);
        let note = match result {
            Ok(DeleteToggle::Marked) => Some(text::EDIT_ROW_MARKED),
            Ok(DeleteToggle::Unmarked) => Some(text::EDIT_ROW_UNMARKED),
            Ok(DeleteToggle::RemovedInsert) => {
                let col = tab.result.view.col;
                let at = tab.result.view.row;
                tab.result.view.move_to(at, col, &tab.result.table, false);
                Some(text::EDIT_INSERT_REMOVED)
            }
            Err(reason) => {
                self.status = Some(reason);
                None
            }
        };
        if let Some(note) = note {
            self.reset_table_search(index);
            self.status = Some(note.to_owned());
        }
    }

    /// A new search reads the new values instead of retaining old matches.
    fn reset_table_search(&mut self, index: usize) {
        let tab = &mut self.tables[index];
        if let Some(search) = &mut tab.result.search {
            let query = search.query().to_owned();
            *search = crate::grid::RowSearch::new((tab.result.view.row, tab.result.view.col));
            search.push_str(&query);
        }
    }

    fn open_cell_editor(&mut self, id: ConsoleId, row: usize, col: usize, null: bool) {
        if self.table().is_none_or(|tab| tab.id != id) {
            return;
        }
        let tab = self.table().unwrap();
        let Some(metadata) = &tab.edit_metadata else {
            return;
        };
        let Some(values) = tab.result.table.rows.get(row) else {
            return;
        };
        let check = if tab.result.edits.is_insert(row) {
            metadata.editable_insert(&tab.result.table.columns, col)
        } else {
            metadata.editable(&tab.result.table.columns, values, col)
        };
        if let Err(reason) = check {
            self.status = Some(reason);
            return;
        }
        if null {
            self.set_cell_value(id, row, col, None);
            return;
        }
        let value = values.text(col).unwrap_or_default();
        let title = format!(
            "{} · {}.{}",
            text::EDIT_TITLE,
            tab.label(),
            tab.result.table.columns[col]
        );
        let mut editor = self.new_editor(&value, metadata.dialect);
        editor.use_literal_text(&value);
        self.edit_form = Some(EditForm {
            console: id,
            title,
            body: Body::Cell {
                row,
                col,
                editor: Box::new(editor),
            },
            focus: 0,
            keys: Vec::new(),
            note: None,
        });
    }

    fn set_cell_value(
        &mut self,
        id: ConsoleId,
        row: usize,
        col: usize,
        value: Option<&str>,
    ) -> bool {
        let budget = self.budget.clone();
        let Some(index) = self.tables.iter().position(|tab| tab.id == id) else {
            return false;
        };
        let tab = &self.tables[index];
        let insert = tab.result.edits.is_insert(row);
        let result = (|| {
            let metadata = tab.edit_metadata.as_ref().ok_or(text::EDIT_UNAVAILABLE)?;
            let values = tab.result.table.rows.get(row).ok_or(text::EDIT_NO_CELL)?;
            if insert {
                metadata
                    .editable_insert(&tab.result.table.columns, col)
                    .map_err(|_| text::EDIT_PROTECTED)?;
            } else {
                metadata
                    .editable(&tab.result.table.columns, values, col)
                    .map_err(|_| text::EDIT_PROTECTED)?;
            }
            if value.is_none() && !metadata.table.columns[col].nullable {
                return Err(text::EDIT_NOT_NULL);
            }
            Ok(())
        })();
        if let Err(reason) = result {
            self.edit_error(reason.to_owned());
            return false;
        }
        let tab = &mut self.tables[index];
        let insert = tab.result.edits.is_insert(row);
        let value = match value {
            None => Value::Null,
            Some(value)
                if (if insert {
                    tab.edit_metadata
                        .as_ref()
                        .is_some_and(|metadata| metadata.numeric(col))
                } else {
                    matches!(tab.result.table.rows[row].get(col), Some(Value::Number(_)))
                }) && is_number(value) =>
            {
                Value::Number(value)
            }
            Some(value) => Value::Text(value),
        };
        let result = if insert {
            tab.result
                .edits
                .set_insert(&mut tab.result.table, row, col, value, &budget)
        } else {
            tab.result
                .edits
                .set(&mut tab.result.table, row, col, value, &budget)
        };
        match result {
            Ok(()) => {
                // A new search reads the new values instead of retaining old matches.
                if let Some(search) = &mut tab.result.search {
                    let query = search.query().to_owned();
                    *search = crate::grid::RowSearch::new((row, col));
                    search.push_str(&query);
                }
                true
            }
            Err(reason) => {
                self.edit_error(reason);
                false
            }
        }
    }

    fn edit_error(&mut self, reason: String) {
        if let Some(form) = &mut self.edit_form {
            form.note = Some(reason);
        } else {
            self.status = Some(reason);
        }
    }

    pub(super) fn review_edits(&mut self) {
        let Some(tab) = self.table() else { return };
        if tab.is_busy() || tab.edit_uncertain || tab.result.edits.is_empty() {
            self.status = Some(
                if tab.edit_uncertain {
                    text::EDIT_COMMIT_UNKNOWN
                } else {
                    text::EDIT_NO_CHANGES
                }
                .to_owned(),
            );
            return;
        }
        let Some(metadata) = tab.edit_metadata.clone() else {
            return;
        };
        let id = tab.id;
        let changes = tab.result.edits.changes();
        self.start_edit_job(id, Job::Review { metadata, changes });
    }

    fn request_apply_edits(&mut self, id: ConsoleId) {
        let Some(index) = self.tables.iter().position(|tab| tab.id == id) else {
            return;
        };
        if self.tables[index].edit_uncertain
            || self.tables[index].is_busy()
            || self.config_blocked()
        {
            return;
        }
        let connection = self.tables[index].connection;
        if self.is_read_only(connection) {
            self.status = Some(text::EDIT_UNAVAILABLE.to_owned());
            return;
        }
        self.edit_form = None;
        if let Some(item) = self
            .connection(connection)
            .filter(|item| item.is_production())
        {
            let body = text::edit_production(&self.tables[index].label(), item.plain_name());
            self.show_dialog(
                text::EDIT_REVIEW,
                body,
                vec![
                    (text::OPTION_CANCEL, DialogAction::CancelEdits(id)),
                    (text::EDIT_APPLY, DialogAction::ApplyEdits(id)),
                ],
            );
        } else {
            self.apply_edits(id);
        }
    }

    pub(super) fn apply_edits(&mut self, id: ConsoleId) {
        if self.config_blocked() {
            self.cancel_edits(id);
            return;
        }
        let Some(tab) = self.tables.iter().find(|tab| tab.id == id) else {
            return;
        };
        if tab.is_busy() || tab.edit_uncertain || self.is_read_only(tab.connection) {
            return;
        }
        let Some(metadata) = tab.edit_metadata.clone() else {
            return;
        };
        let changes = tab.result.edits.changes();
        if changes.is_empty() {
            return;
        }
        let connection = tab.connection;
        let snapshot = |index: usize| {
            tab.result
                .edits
                .rows
                .get(&index)
                .map(|pending| pending.current.clone())
                .or_else(|| {
                    tab.result
                        .table
                        .rows
                        .get(index)
                        .and_then(|row| SavedRow::reserve(row.clone(), &self.budget).ok())
                })
        };
        let restore = snapshot(tab.result.view.row).map(|cursor| Restore {
            view: tab.result.view,
            cursor,
            anchor: tab.result.view.anchor.and_then(|(row, _)| snapshot(row)),
            columns: tab.result.table.columns.clone(),
            primary: metadata.table.primary.clone(),
        });
        self.tables
            .iter_mut()
            .find(|tab| tab.id == id)
            .unwrap()
            .edit_restore = restore;
        self.authorize(
            connection,
            Use::Edit {
                console: id,
                job: Job::Apply { metadata, changes },
            },
        );
    }

    pub(super) fn start_edit_job(&mut self, id: ConsoleId, job: Job) {
        let Some(index) = self.tables.iter().position(|tab| tab.id == id) else {
            return;
        };
        let review = matches!(&job, Job::Review { .. });
        if self.tables[index].edit_uncertain
            || (!review && self.is_read_only(self.tables[index].connection))
        {
            self.tables[index].phase = Phase::Idle;
            self.tables[index].edit_close = false;
            self.exiting = false;
            self.status = Some(text::EDIT_UNAVAILABLE.to_owned());
            return;
        }
        let Some(manager) = &self.manager else { return };
        let tab = &mut self.tables[index];
        let execution = manager.edit(id, job);
        tab.edit_execution = Some(execution);
        tab.phase = Phase::Waiting;
        tab.run.start();
    }

    pub(super) fn on_edit_event(&mut self, event: &mut ExecEvent) -> bool {
        let Some(id) = event.console() else {
            return false;
        };
        let Some(index) = self.tables.iter().position(|tab| tab.id == id) else {
            return matches!(event, ExecEvent::EditFinished { .. });
        };
        let Some(execution) = self.tables[index].edit_execution else {
            return matches!(event, ExecEvent::EditFinished { .. });
        };
        if event.execution() != Some(execution) {
            return false;
        }
        match event {
            ExecEvent::Started { .. } => self.tables[index].phase = Phase::Running,
            ExecEvent::EditFinished { result, .. } => {
                let result = std::mem::replace(
                    result,
                    Err(crate::data_edit::Failure {
                        message: String::new(),
                        uncertain: false,
                    }),
                );
                let can_show_review = self.table_active == Some(index)
                    && !self.modal_is_open()
                    && self.form.is_none();
                let tab = &mut self.tables[index];
                tab.edit_execution = None;
                tab.phase = Phase::Idle;
                tab.run.finish(if result.is_ok() {
                    ExecutionEnd::Completed
                } else {
                    ExecutionEnd::Failed
                });
                match result {
                    Ok(Outcome::Metadata(metadata)) => {
                        tab.edit_metadata = Some(metadata);
                        match tab.edit_after.take() {
                            Some(EditAfter::Cell { row, col, null }) => {
                                self.open_cell_editor(id, row, col, null);
                            }
                            Some(EditAfter::NewRow) => self.append_new_row(id),
                            Some(EditAfter::Delete { row }) => {
                                self.toggle_delete_row_at(id, row);
                            }
                            None => {}
                        }
                    }
                    Ok(Outcome::Review(contents)) => {
                        if can_show_review {
                            self.edit_form = Some(EditForm {
                                console: id,
                                title: format!("{} · {}", text::EDIT_REVIEW, tab.label()),
                                body: Body::Review {
                                    contents: WrappedText::new(contents),
                                    scroll: 0,
                                },
                                focus: 0,
                                keys: Vec::new(),
                                note: None,
                            });
                        } else {
                            tab.edit_close = false;
                            self.exiting = false;
                            self.status = Some(text::EDIT_REVIEW_AGAIN.to_owned());
                        }
                    }
                    Ok(Outcome::Applied) => {
                        tab.result.edits = Default::default();
                        self.status = Some(text::EDIT_SAVED.to_owned());
                        let close = tab.edit_close;
                        tab.edit_close = false;
                        if close {
                            self.close_table(index);
                            if self.exiting {
                                self.continue_exit();
                            }
                        } else {
                            tab.pending = true;
                            tab.edit_reload = true;
                            if self.table_active == Some(index) {
                                self.reload_saved_table();
                            }
                        }
                    }
                    Err(error) => {
                        tab.edit_restore = None;
                        tab.edit_uncertain = error.uncertain;
                        tab.edit_after = None;
                        tab.edit_close = false;
                        self.exiting = false;
                        self.status =
                            Some(format!("{} {}", error.message, text::EDIT_KEEP_PENDING));
                    }
                }
            }
            ExecEvent::ExecutionFinished { message, .. } => {
                let tab = &mut self.tables[index];
                tab.edit_execution = None;
                tab.edit_after = None;
                tab.edit_restore = None;
                tab.phase = Phase::Idle;
                tab.edit_close = false;
                tab.run.finish(ExecutionEnd::Failed);
                self.exiting = false;
                self.status = Some(
                    message
                        .take()
                        .unwrap_or_else(|| text::EDIT_STOPPED.to_owned()),
                );
            }
            _ => {}
        }
        true
    }

    pub(super) fn discard_edits(&mut self, id: ConsoleId) {
        let budget = self.budget.clone();
        let Some(index) = self.tables.iter().position(|tab| tab.id == id) else {
            return;
        };
        let tab = &mut self.tables[index];
        if tab.is_busy() {
            self.status = Some(text::TABLE_BUSY.to_owned());
            return;
        }
        match tab.result.edits.discard(&mut tab.result.table, &budget) {
            Err(reason) => self.status = Some(reason),
            Ok(()) => {
                tab.result.search = None;
                tab.edit_restore = None;
                let reload = tab.edit_uncertain;
                tab.edit_uncertain = false;
                tab.edit_metadata = None;
                let close = tab.edit_close;
                tab.edit_close = false;
                if close {
                    self.close_table(index);
                    if self.exiting {
                        self.continue_exit();
                    }
                } else if reload {
                    tab.pending = true;
                    if self.table_active == Some(index) {
                        self.reload_saved_table();
                    }
                }
            }
        }
    }

    pub(super) fn cancel_edits(&mut self, id: ConsoleId) {
        self.edit_form = None;
        self.exiting = false;
        if let Some(tab) = self.tables.iter_mut().find(|tab| tab.id == id) {
            tab.edit_close = false;
        }
    }

    pub(super) fn confirm_discard_edits(&mut self) {
        let Some(tab) = self.table() else { return };
        self.show_dialog(
            text::EDIT_REVIEW,
            text::EDIT_DISCARD_QUESTION.to_owned(),
            vec![
                (text::OPTION_CANCEL, DialogAction::CancelEdits(tab.id)),
                (text::EDIT_DISCARD, DialogAction::DiscardEdits(tab.id)),
            ],
        );
    }

    pub(super) fn edit_close_guard(&mut self, index: usize) -> bool {
        let tab = &self.tables[index];
        if tab.edit_execution.is_some() || (tab.is_busy() && !tab.result.edits.is_empty()) {
            self.status = Some(text::TABLE_BUSY.to_owned());
            self.exiting = false;
            return true;
        }
        if tab.result.edits.is_empty() {
            return false;
        }
        let id = tab.id;
        let uncertain = tab.edit_uncertain;
        let body = format!(
            "{}\n{}",
            tab.label(),
            if uncertain {
                text::EDIT_COMMIT_UNKNOWN
            } else {
                text::EDIT_CLOSE_QUESTION
            }
        );
        self.tables[index].edit_close = true;
        let mut options = vec![(text::OPTION_CANCEL, DialogAction::CancelEdits(id))];
        if !uncertain {
            options.push((text::EDIT_APPLY, DialogAction::ReviewEdits(id)));
        }
        options.push((text::EDIT_DISCARD, DialogAction::DiscardEdits(id)));
        self.show_dialog(text::EDIT_REVIEW, body, options);
        true
    }

    pub(super) fn on_edit_key(&mut self, key: KeyEvent) {
        let Some(form) = &self.edit_form else { return };
        let id = form.console;
        if key.code == KeyCode::Esc && !form.keys.is_empty() {
            self.edit_form.as_mut().unwrap().keys.clear();
            return;
        }
        let review = matches!(form.body, Body::Review { .. });
        let escape = key.code == KeyCode::Esc
            && match &form.body {
                Body::Cell { editor, .. } => !self.vim() || !editor.is_inserting(),
                _ => true,
            };
        if escape {
            self.cancel_edits(id);
            return;
        }
        if key.code == KeyCode::Tab || key.code == KeyCode::BackTab {
            let count = if review { 3 } else { 4 };
            let form = self.edit_form.as_mut().unwrap();
            form.focus = (form.focus
                + if key.code == KeyCode::BackTab {
                    count - 1
                } else {
                    1
                })
                % count;
            return;
        }
        if key.code == KeyCode::F(2)
            || (key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            self.edit_button(1);
            return;
        }
        if key.code == KeyCode::Enter && form.focus > 0 {
            self.edit_button(form.focus);
            return;
        }
        if let Some(form) = &mut self.edit_form {
            match &mut form.body {
                Body::Cell { editor, .. } if form.focus == 0 => {
                    let text_key = editor.is_inserting()
                        && (key.modifiers - KeyModifiers::SHIFT).is_empty()
                        && matches!(key.code, KeyCode::Char(_));
                    if text_key && form.keys.is_empty() {
                        editor.on_key(key, None);
                    } else {
                        let mut keys = std::mem::take(&mut form.keys);
                        keys.push(Chord::from_event(key));
                        match self.keymap.sequence(Region::Editor, &keys) {
                            Lookup::Action(action) => editor.on_command(action, None),
                            Lookup::Sequence => form.keys = keys,
                            Lookup::None => editor.on_key(key, None),
                        }
                    }
                }
                Body::Review { scroll, .. } => match key.code {
                    KeyCode::Down => *scroll = scroll.saturating_add(1),
                    KeyCode::Up => *scroll = scroll.saturating_sub(1),
                    KeyCode::PageDown => *scroll = scroll.saturating_add(10),
                    KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                    _ => {}
                },
                _ => {}
            }
        }
    }

    fn edit_button(&mut self, button: usize) {
        let Some(form) = &self.edit_form else { return };
        let id = form.console;
        match &form.body {
            Body::Review { .. } => {
                if button == 1 {
                    self.request_apply_edits(id);
                } else {
                    self.cancel_edits(id);
                }
            }
            Body::Cell { row, col, editor } => {
                let (row, col) = (*row, *col);
                let value = editor.text();
                if button == 3 {
                    self.cancel_edits(id);
                } else if self.set_cell_value(id, row, col, (button == 1).then_some(value.as_str()))
                {
                    self.edit_form = None;
                }
            }
        }
    }

    pub(super) fn on_edit_mouse(&mut self, mouse: MouseEvent) {
        let position = Position::new(mouse.column, mouse.row);
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && self.layout.edit.controls.contains(position)
        {
            if let Some((button, _)) = self
                .layout
                .edit
                .buttons
                .iter()
                .find(|(_, rect)| rect.contains(position))
            {
                self.edit_button(*button);
            }
            return;
        }
        if let Some(form) = &mut self.edit_form {
            match &mut form.body {
                Body::Cell { editor, .. } if self.layout.edit.contents.contains(position) => {
                    form.focus = 0;
                    editor.on_mouse(mouse);
                }
                Body::Review { scroll, .. } => match mouse.kind {
                    MouseEventKind::ScrollDown => *scroll = scroll.saturating_add(3),
                    MouseEventKind::ScrollUp => *scroll = scroll.saturating_sub(3),
                    _ => {}
                },
                _ => {}
            }
        }
    }
}
