//! Keys, in the priority of `docs/KEYMAP.md`, and the actions of the shell.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, DialogAction, Overlay};
use crate::connector::{TransactionCommand, TransactionState};
use crate::console::{AfterTransaction, Focus, GridAction, Phase};
use crate::editor::{KeyboardMode, RunRefusal, RunScope};
use crate::exec::RunOptions;
use crate::form::FormRequest;
use crate::input::{Action, Chord, Lookup, Region};
use crate::ui::text;
use crate::ui::wrap::WrappedText;

impl App {
    fn vim(&self) -> bool {
        self.keyboard == KeyboardMode::Vim
    }

    /// Returns the panel that receives keys, without a text field.
    fn panel(&self) -> Option<Region> {
        if self.navigator_focused {
            return Some(Region::Navigator);
        }
        let console = self.console();
        (console.focus == Focus::Results && !console.search_is_open()).then_some(Region::Grid)
    }

    /// Applies the key priority of `docs/KEYMAP.md`.
    pub(super) fn on_key(&mut self, key: KeyEvent) {
        self.status = None;
        if !self.dialogs.is_empty() {
            self.on_dialog_key(key);
            return;
        }
        if self.picker.is_some() {
            self.on_picker_key(key);
            return;
        }
        if self.overlay.is_some() {
            self.on_overlay_key(key);
            return;
        }
        if self.form.is_some() {
            self.on_form_key(key);
            return;
        }
        let in_editor = !self.navigator_focused && self.console().focus == Focus::Editor;
        if in_editor && self.console_mut().editor.on_list_key(key) {
            return;
        }
        let chord = Chord::from_event(key);
        if let Some(first) = self.pending_key.take() {
            self.on_second_key(first, chord);
            return;
        }
        let panel = self.panel();
        if let Some(region) = panel
            && self.on_chord(region, chord)
        {
            return;
        }
        if !self.is_text_key(key, panel) && self.on_chord(Region::Global, chord) {
            return;
        }
        match panel {
            Some(Region::Grid) => self.on_shifted_grid_key(chord),
            Some(_) => {}
            None if self.console().focus == Focus::Results => self.console_mut().on_search_key(key),
            None => self.on_editor_key(key),
        }
    }

    /// During text input, a character is always text.
    fn is_text_key(&self, key: KeyEvent, panel: Option<Region>) -> bool {
        let plain = (key.modifiers - KeyModifiers::SHIFT).is_empty();
        panel.is_none() && plain && matches!(key.code, KeyCode::Char(_))
    }

    /// Returns true when the region has a meaning for the key.
    fn on_chord(&mut self, region: Region, chord: Chord) -> bool {
        match self.keymap.lookup(region, chord) {
            Lookup::Action(action) => {
                self.perform(action, false);
                true
            }
            Lookup::Sequence => {
                self.pending_key = Some(chord);
                true
            }
            Lookup::None => false,
        }
    }

    fn on_second_key(&mut self, first: Chord, second: Chord) {
        let panel = self.panel();
        let action = panel
            .and_then(|region| self.keymap.after(region, first, second))
            .or_else(|| self.keymap.after(Region::Global, first, second));
        if let Some(action) = action {
            self.perform(action, false);
        }
    }

    /// Shift changes a move of the grid into a selection.
    fn on_shifted_grid_key(&mut self, chord: Chord) {
        let Some(plain) = chord.without_shift() else { return };
        if let Lookup::Action(action) = self.keymap.lookup(Region::Grid, plain)
            && action.extends_selection()
        {
            self.perform(action, true);
        }
    }

    fn on_editor_key(&mut self, key: KeyEvent) {
        self.ensure_active_catalog();
        let index = self.active;
        let connection = self.catalog_for(index);
        // The catalog and the console are separate fields of the application.
        let App { consoles, connections, fallback_catalog, .. } = self;
        let catalog = match connection {
            Some(id) => connections.iter().find(|item| item.id == id).and_then(|item| item.catalog.as_ref()),
            None if consoles[index].connection.is_none() => fallback_catalog.as_ref(),
            None => None,
        };
        consoles[index].editor.on_key(key, catalog);
        self.request_missing_columns(index);
    }

    pub(super) fn perform(&mut self, action: Action, extend: bool) {
        match action {
            Action::Run => self.run(RunScope::Default),
            Action::RunScript => self.run(RunScope::Script),
            Action::Stop => self.stop(self.active),
            Action::ToggleMode => self.toggle_mode(),
            Action::Commit => self.transaction(TransactionCommand::Commit),
            Action::Rollback => self.transaction(TransactionCommand::Rollback),
            Action::Help => self.open_keys(),
            Action::CloseTab => self.close_step(self.console().id),
            Action::Quit => self.request_exit(),
            Action::NextTab => self.switch_console(true),
            Action::PreviousTab => self.switch_console(false),
            Action::ToggleNavigator => self.toggle_navigator(),
            Action::ChooseSchema => self.open_picker(),
            Action::FocusLeft => self.focus_navigator(),
            Action::FocusRight => self.navigator_focused = false,
            Action::FocusUp => self.focus_console(Focus::Editor),
            Action::FocusDown => self.focus_console(Focus::Results),
            Action::NavigatorUp
            | Action::NavigatorDown
            | Action::NavigatorCollapse
            | Action::NavigatorExpand
            | Action::NavigatorOpen
            | Action::NavigatorNewConsole
            | Action::NavigatorRefresh
            | Action::NavigatorDisconnect
            | Action::NavigatorEdit
            | Action::NavigatorDelete
            | Action::NavigatorAdd => self.on_navigator_action(action),
            _ => self.on_grid_action(action, extend),
        }
    }

    fn on_grid_action(&mut self, action: Action, extend: bool) {
        let budget = self.budget.clone();
        match self.console_mut().on_results_action(action, extend) {
            GridAction::None => {}
            GridAction::Copy(tsv) => self.copy(&tsv),
            GridAction::OpenValue { title, text, footer } => {
                let contents = WrappedText::new(text);
                self.overlay = Some(Overlay { title, contents, footer, scroll: 0 });
            }
            GridAction::CloseResult => {
                if !self.console_mut().close_result(&budget) {
                    self.status = Some(text::RESULT_READING.to_owned());
                }
            }
        }
    }

    fn focus_navigator(&mut self) {
        if !self.navigator.collapsed {
            self.console_mut().editor.close_suggestions();
            self.navigator_focused = true;
        }
    }

    fn focus_console(&mut self, focus: Focus) {
        self.navigator_focused = false;
        let console = self.console_mut();
        if focus == Focus::Results {
            console.editor.close_suggestions();
        }
        console.focus = focus;
    }

    fn toggle_navigator(&mut self) {
        self.navigator.collapsed = !self.navigator.collapsed;
        if self.navigator.collapsed {
            self.navigator_focused = false;
        }
    }

    fn switch_console(&mut self, forward: bool) {
        let count = self.consoles.len();
        self.console_mut().editor.close_suggestions();
        self.active = (self.active + if forward { 1 } else { count - 1 }) % count;
    }

    pub(super) fn select_console(&mut self, index: usize) {
        if index < self.consoles.len() {
            self.console_mut().editor.close_suggestions();
            self.active = index;
        }
    }

    fn open_keys(&mut self) {
        let (region, name) = match self.panel() {
            Some(Region::Navigator) => (Some(Region::Navigator), text::REGION_NAVIGATOR),
            Some(_) => (Some(Region::Grid), text::REGION_GRID),
            None => (None, text::REGION_EDITOR),
        };
        let mut rows: Vec<String> = Vec::new();
        match region {
            Some(region) => rows.extend(self.key_rows(region)),
            None => rows.push(text::EDITOR_KEYS.to_owned()),
        }
        rows.push(String::new());
        rows.extend(self.key_rows(Region::Global));
        let mode = if self.vim() { text::MODE_VIM } else { text::MODE_CONVENTIONAL };
        self.overlay = Some(Overlay {
            title: text::keys_title(mode, name),
            contents: WrappedText::new(rows.join("\n")),
            footer: String::new(),
            scroll: 0,
        });
    }

    fn key_rows(&self, region: Region) -> Vec<String> {
        self.keymap
            .actions(region)
            .filter_map(|action| {
                let keys = self.keymap.keys(action);
                (!keys.is_empty()).then(|| text::keys_row(&keys.join(", "), action))
            })
            .collect()
    }

    fn run(&mut self, scope: RunScope) {
        let console = self.console();
        if console.connection.is_none() {
            self.status = Some(text::NO_CONNECTION.to_owned());
            return;
        }
        if console.is_busy() {
            self.status = Some(text::EXECUTION_ACTIVE.to_owned());
            return;
        }
        if console.transaction == TransactionState::Unknown {
            let id = console.id;
            self.show_dialog(
                text::TITLE_UNKNOWN,
                text::BODY_UNKNOWN.to_owned(),
                vec![
                    (text::OPTION_CANCEL, DialogAction::Dismiss),
                    (text::OPTION_CONTINUE, DialogAction::RunAnyway(id, scope)),
                ],
            );
            return;
        }
        self.start_run(self.active, scope);
    }

    pub(super) fn start_run(&mut self, index: usize, scope: RunScope) {
        let console = &mut self.consoles[index];
        let Some(manager) = &self.manager else { return };
        match console.editor.plan_run(scope) {
            Ok(plan) => {
                let options = RunOptions { manual: console.manual, schema: console.schema.clone() };
                let execution = manager.run(console.id, plan.statements.clone(), options);
                console.begin_execution(execution, &plan, &self.budget);
            }
            Err(RunRefusal::CursorOutside) => {
                self.status = Some(text::CURSOR_OUTSIDE.to_owned());
            }
            Err(RunRefusal::NoStatement) => self.status = Some(text::NO_STATEMENT.to_owned()),
        }
    }

    pub(super) fn stop(&mut self, index: usize) {
        let console = &mut self.consoles[index];
        if !console.is_busy() {
            return;
        }
        if let Some(manager) = &self.manager {
            manager.stop(console.id);
            console.phase = Phase::Stopping;
        }
    }

    fn transaction(&mut self, command: TransactionCommand) {
        let console = self.console();
        if console.is_busy() {
            self.status = Some(text::EXECUTION_ACTIVE.to_owned());
            return;
        }
        if !console.in_transaction() {
            self.status = Some(text::NO_TRANSACTION.to_owned());
            return;
        }
        if let Some(manager) = &self.manager {
            manager.transaction(console.id, command);
        }
    }

    fn toggle_mode(&mut self) {
        let console = self.console_mut();
        if console.is_busy() {
            self.status = Some(text::EXECUTION_ACTIVE.to_owned());
            return;
        }
        if !console.manual {
            console.manual = true;
            return;
        }
        if !console.in_transaction() {
            console.manual = false;
            return;
        }
        let id = console.id;
        let after = AfterTransaction::UseAutocommit;
        let rollback = DialogAction::Transaction(id, TransactionCommand::Rollback, after);
        let commit = DialogAction::Transaction(id, TransactionCommand::Commit, after);
        self.show_dialog(
            text::TITLE_TRANSACTION,
            text::BODY_TRANSACTION_MODE.to_owned(),
            vec![
                (text::OPTION_CANCEL, DialogAction::Dismiss),
                (text::OPTION_ROLLBACK_AUTO, rollback),
                (text::OPTION_COMMIT_AUTO, commit),
            ],
        );
    }

    fn on_dialog_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        let Some(dialog) = self.dialogs.front_mut() else { return };
        let count = dialog.options.len();
        match key.code {
            KeyCode::Tab | KeyCode::Right => dialog.focus = (dialog.focus + 1) % count,
            KeyCode::BackTab | KeyCode::Left => dialog.focus = (dialog.focus + count - 1) % count,
            KeyCode::Char('l') if vim => dialog.focus = (dialog.focus + 1) % count,
            KeyCode::Char('h') if vim => dialog.focus = (dialog.focus + count - 1) % count,
            KeyCode::Enter => {
                let action = dialog.options[dialog.focus].1;
                self.dialogs.pop_front();
                self.on_dialog_action(action);
            }
            KeyCode::Esc => {
                // The first option never changes the database or the disk.
                let action = dialog.options[0].1;
                self.dialogs.pop_front();
                self.on_dialog_action(action);
            }
            _ => {}
        }
    }

    fn on_picker_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        let Some(picker) = &mut self.picker else { return };
        let last = picker.items.len();
        match key.code {
            KeyCode::Esc => self.picker = None,
            KeyCode::Enter => self.accept_picker(),
            KeyCode::Up => picker.cursor = picker.cursor.saturating_sub(1),
            KeyCode::Down => picker.cursor = (picker.cursor + 1).min(last),
            KeyCode::Char('k') if vim => picker.cursor = picker.cursor.saturating_sub(1),
            KeyCode::Char('j') if vim => picker.cursor = (picker.cursor + 1).min(last),
            _ => {}
        }
    }

    fn on_form_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        let Some(form) = &mut self.form else { return };
        match form.on_key(key, vim) {
            FormRequest::None => {}
            FormRequest::Cancel => self.form = None,
            FormRequest::Test => self.test_form(),
            FormRequest::Save => self.save_form(),
            FormRequest::Disconnect => self.disconnect_form(),
        }
    }

    fn on_overlay_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(overlay) = &mut self.overlay else { return };
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.overlay = None,
            KeyCode::Up | KeyCode::Char('k') => overlay.scroll = overlay.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => overlay.scroll = overlay.scroll.saturating_add(1),
            KeyCode::Char('c') if ctrl => {
                let contents = overlay.contents.text().to_owned();
                self.copy(&contents);
            }
            _ => {}
        }
    }

    pub(super) fn copy(&mut self, text: &str) {
        use crate::clipboard::CopyOutcome;
        let outcome = self.clipboard.borrow_mut().copy(text);
        self.status = Some(
            match outcome {
                CopyOutcome::Copied => text::COPIED,
                CopyOutcome::SentToTerminal => text::COPY_SENT_TO_TERMINAL,
                CopyOutcome::Failed => text::COPY_FAILED,
            }
            .to_owned(),
        );
    }

    pub(super) fn hint(&self) -> &'static str {
        let console = self.console();
        if !self.dialogs.is_empty() {
            text::HINT_DIALOG
        } else if self.picker.is_some() {
            text::HINT_PICKER
        } else if self.overlay.is_some() {
            text::HINT_OVERLAY
        } else if self.form.is_some() {
            text::HINT_FORM
        } else if self.pending_key.is_some_and(Chord::is_prefix) {
            text::HINT_PREFIX
        } else if self.navigator_focused {
            text::HINT_NAVIGATOR
        } else if console.focus == Focus::Results && console.search_is_open() {
            text::HINT_GRID_SEARCH
        } else if console.focus == Focus::Results {
            text::HINT_GRID
        } else if console.editor.suggestions().is_some() {
            text::HINT_LIST
        } else if console.editor.search().is_some() {
            text::HINT_SEARCH
        } else if console.is_busy() {
            text::HINT_RUNNING
        } else {
            text::HINT_EDITOR
        }
    }
}
