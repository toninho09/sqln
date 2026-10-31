//! Keys, in the priority of `docs/KEYMAP.md`, and the actions of the shell.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, DialogAction, Overlay, PickerKind};
use crate::connector::{Namespace, TransactionCommand, TransactionState};
use crate::console::{AfterTransaction, Focus, GridAction, Phase};
use crate::editor::{KeyboardMode, RunRefusal, RunScope};
use crate::exec::RunOptions;
use crate::form::FormRequest;
use crate::input::{Action, Chord, Lookup, Region};
use crate::palette::{Command, Palette, PaletteRequest};
use crate::settings::SettingsRequest;
use crate::table::TableFocus;
use crate::ui::name::NameAction;
use crate::ui::text;
use crate::ui::welcome::Welcome;
use crate::ui::which_key::KeyGroup;
use crate::ui::wrap::WrappedText;

/// The actions of one row of the `Ctrl+W` menu, with its label.
type MenuRow = (&'static [Action], &'static str);
/// The title of one group of the `Ctrl+W` menu, with its rows.
type MenuGroup = (&'static str, &'static [MenuRow]);

impl App {
    pub(super) fn vim(&self) -> bool {
        self.keyboard == KeyboardMode::Vim
    }

    /// Returns the panel that receives keys, without a text field.
    fn panel(&self) -> Option<Region> {
        if self.navigator_focused {
            return Some(Region::Navigator);
        }
        if let Some(tab) = self.table() {
            let in_grid = tab.focus == TableFocus::Grid && tab.result.search.is_none();
            return in_grid.then_some(Region::Grid);
        }
        let console = self.console();
        (console.focus == Focus::Results && !console.search_is_open()).then_some(Region::Grid)
    }

    /// Returns true when the grid of the tab on screen has the focus.
    pub(super) fn grid_has_focus(&self) -> bool {
        if self.navigator_focused {
            return false;
        }
        match self.table() {
            Some(tab) => tab.focus == TableFocus::Grid,
            None => self.console().focus == Focus::Results,
        }
    }

    /// Applies the key priority of `docs/KEYMAP.md`.
    pub(super) fn on_key(&mut self, key: KeyEvent) {
        if !self.dialogs.is_empty() {
            self.on_dialog_key(key);
            return;
        }
        if self.auth.prompt.is_some() {
            self.on_password_key(key);
            return;
        }
        if self.conflicts.is_some() {
            self.on_conflicts_key(key);
            return;
        }
        if self.palette.is_some() {
            self.on_palette_key(key);
            return;
        }
        if self.rename.is_some() {
            self.on_name_key(key);
            return;
        }
        if self.settings_form.is_some() {
            self.on_settings_key(key);
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
        let in_editor = !self.navigator_focused
            && self.table_active.is_none()
            && self.console().focus == Focus::Editor;
        if (in_editor && self.console_mut().editor.on_list_key(key)) || self.on_table_list_key(key) {
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
            None if self.table_active.is_some() => self.on_table_text_key(key),
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
        // `Ctrl+S` writes the text at once, in both keyboard modes.
        if key.code == KeyCode::Char('s') && key.modifiers == KeyModifiers::CONTROL {
            self.save_now();
            return;
        }
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
        if self.table_active.is_some() && self.perform_on_table(action) {
            return;
        }
        match action {
            Action::Run => self.run(RunScope::Default),
            Action::RunScript => self.run(RunScope::Script),
            Action::Stop => self.stop(self.active),
            Action::ToggleMode => self.toggle_mode(),
            Action::Commit => self.transaction(TransactionCommand::Commit),
            Action::Rollback => self.transaction(TransactionCommand::Rollback),
            Action::Help => self.open_keys(),
            Action::OpenPalette => self.open_palette(),
            Action::CloseTab => self.close_step(self.console().id),
            Action::Quit => self.request_exit(),
            Action::NextTab => self.switch_tab(true),
            Action::PreviousTab => self.switch_tab(false),
            Action::ToggleNavigator => self.toggle_navigator(),
            Action::ChooseSchema => self.open_picker(),
            Action::ReloadConfig => self.reload_config(),
            Action::RenameConsole => self.start_rename(),
            Action::ChangeConnection => self.start_connection_change(),
            Action::OpenSettings => self.open_settings(),
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
            | Action::NavigatorAdd
            | Action::NavigatorScripts
            | Action::NavigatorDeletePassword => self.on_navigator_action(action),
            _ => self.on_grid_action(action, extend),
        }
    }

    fn on_grid_action(&mut self, action: Action, extend: bool) {
        let budget = self.budget.clone();
        let vim = self.vim();
        let outcome = match self.table_mut() {
            Some(tab) => tab.result.on_action(action, extend, vim),
            None => self.console_mut().on_results_action(action, extend),
        };
        match outcome {
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

    pub(super) fn focus_navigator(&mut self) {
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

    pub(super) fn select_console(&mut self, index: usize) {
        if index < self.consoles.len() {
            self.console_mut().editor.close_suggestions();
            self.active = index;
            self.table_active = None;
        }
    }

    fn open_keys(&mut self) {
        let in_table = self.table_active.is_some();
        let (region, name) = match self.panel() {
            Some(Region::Navigator) => (Some(Region::Navigator), text::REGION_NAVIGATOR),
            Some(_) if in_table => (Some(Region::Grid), text::REGION_TABLE),
            Some(_) => (Some(Region::Grid), text::REGION_GRID),
            None if in_table => (None, text::REGION_FILTER),
            None => (None, text::REGION_EDITOR),
        };
        let mut rows: Vec<String> = Vec::new();
        match region {
            Some(region) => rows.extend(self.key_rows(region)),
            None if in_table => rows.push(text::FILTER_KEYS.to_owned()),
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
        let in_table = self.table_active.is_some();
        self.keymap
            .actions(region)
            .filter(|action| if in_table { !action.is_console_only() } else { !action.is_table_only() })
            .filter_map(|action| {
                let keys = self.keymap.keys(action);
                (!keys.is_empty()).then(|| text::keys_row(&keys.join(", "), action))
            })
            .collect()
    }

    fn run(&mut self, scope: RunScope) {
        let console = self.console();
        if console.connection.is_none() {
            if console.file.is_some() {
                // A script without a connection needs a choice before it runs.
                self.start_association();
            } else {
                self.status = Some(text::NO_CONNECTION.to_owned());
            }
            return;
        }
        if self.config_blocked() {
            return;
        }
        let console = self.console();
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
        match console.editor.plan_run(scope) {
            Ok(plan) => {
                let options = RunOptions {
                    manual: console.manual,
                    schema: console.schema.clone(),
                    row_cap: None,
                };
                let Some(connection) = console.connection else { return };
                let revision = console.editor.revision();
                let console = console.id;
                self.authorize(connection, super::auth::Use::Run { console, connection, plan, options, revision });
            }
            Err(RunRefusal::CursorOutside) => {
                self.status = Some(text::CURSOR_OUTSIDE.to_owned());
            }
            Err(RunRefusal::NoStatement) => self.status = Some(text::NO_STATEMENT.to_owned()),
        }
    }

    pub(super) fn stop(&mut self, index: usize) {
        let id = self.consoles[index].id;
        if self.cancel_auth_console(id) {
            if self.consoles[index].closing { self.close_step(id); }
            return;
        }
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
                let action = dialog.options[dialog.focus].1.clone();
                self.dialogs.pop_front();
                self.on_dialog_action(action);
            }
            KeyCode::Esc => {
                // The first option never changes the database or the disk.
                let action = dialog.options[0].1.clone();
                self.dialogs.pop_front();
                self.on_dialog_action(action);
            }
            _ => {}
        }
    }

    fn on_picker_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        let Some(picker) = &mut self.picker else { return };
        let last = picker.items.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc => self.picker = None,
            KeyCode::Enter => self.accept_picker(),
            KeyCode::Up => picker.cursor = picker.cursor.saturating_sub(1),
            KeyCode::Down => picker.cursor = (picker.cursor + 1).min(last),
            KeyCode::Char('k') if vim => picker.cursor = picker.cursor.saturating_sub(1),
            KeyCode::Char('j') if vim => picker.cursor = (picker.cursor + 1).min(last),
            KeyCode::Delete => self.picker_delete(),
            KeyCode::Char('x') if vim => self.picker_delete(),
            KeyCode::Char('r') => self.picker_rename(),
            _ => {}
        }
    }

    fn on_name_key(&mut self, key: KeyEvent) {
        let Some(prompt) = &mut self.rename else { return };
        match prompt.on_key(key) {
            NameAction::None => {}
            NameAction::Cancel => self.rename = None,
            NameAction::Accept(name) => self.accept_name(name),
        }
    }

    fn on_settings_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        let Some(form) = &mut self.settings_form else { return };
        match form.on_key(key, vim) {
            SettingsRequest::None => {}
            SettingsRequest::Cancel => self.settings_form = None,
            SettingsRequest::Save => self.save_settings(),
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

    /// Returns the key to show for an action. `Ctrl+Enter` and `Shift+Enter` depend on
    /// the terminal, so another key goes first. A removed key shows nothing.
    fn shown_key(&self, action: Action) -> Option<String> {
        let keys = self.keymap.keys(action);
        let steady = keys.iter().find(|key| !matches!(key.as_str(), "ctrl+enter" | "shift+enter"));
        steady.or(keys.first()).map(|key| text::key_label(key))
    }

    fn hint_of(&self, items: &[(Action, &str)]) -> String {
        self.hint_parts(items).join(" · ")
    }

    fn hint_parts(&self, items: &[(Action, &str)]) -> Vec<String> {
        let shown = items.iter().filter_map(|(action, label)| Some(format!("{} {label}", self.shown_key(*action)?)));
        shown.collect()
    }

    /// Joins the second keys of `Ctrl+W` for a group of actions.
    fn prefix_keys(&self, actions: &[Action]) -> String {
        let second = |action: &Action| {
            self.keymap.keys(*action).iter().find_map(|key| key.strip_prefix("ctrl+w ")?.chars().next().map(String::from))
        };
        actions.iter().filter_map(second).collect()
    }

    /// Returns the groups of the `Ctrl+W` menu. A removed key leaves its group.
    pub fn prefix_menu(&self) -> Vec<KeyGroup> {
        use Action::*;
        let groups: [MenuGroup; 6] = [
            (text::GROUP_PANELS, &[
                (&[FocusLeft, FocusDown, FocusUp, FocusRight], text::MENU_FOCUS),
                (&[ToggleNavigator], text::MENU_NAVIGATOR),
            ]),
            (text::GROUP_TABS, &[
                (&[NextTab], text::MENU_NEXT_TAB),
                (&[PreviousTab], text::MENU_PREVIOUS_TAB),
                (&[RenameConsole], text::MENU_RENAME),
                (&[CloseTab], text::MENU_CLOSE_TAB),
            ]),
            (text::GROUP_RUN, &[
                (&[Run], text::MENU_RUN),
                (&[RunScript], text::MENU_SCRIPT),
                (&[Stop], text::MENU_STOP),
            ]),
            (text::GROUP_TRANSACTION, &[
                (&[ToggleMode], text::MENU_MODE),
                (&[Commit], text::MENU_COMMIT),
                (&[Rollback], text::MENU_ROLLBACK),
            ]),
            (text::GROUP_CONNECTION, &[
                (&[ChooseSchema], text::MENU_SCHEMA),
                (&[ChangeConnection], text::MENU_CHANGE),
            ]),
            (text::GROUP_APPLICATION, &[
                (&[OpenPalette], text::MENU_COMMANDS),
                (&[Help], text::MENU_KEYS),
                (&[OpenSettings], text::MENU_SETTINGS),
                (&[ReloadConfig], text::MENU_RELOAD),
                (&[Quit], text::MENU_QUIT),
            ]),
        ];
        groups
            .iter()
            .map(|(title, rows)| {
                let shown = rows.iter().filter_map(|(actions, label)| {
                    let keys = self.prefix_keys(actions);
                    (!keys.is_empty()).then_some((keys, *label))
                });
                KeyGroup { title, keys: shown.collect() }
            })
            .collect()
    }

    /// Returns true when an action has an effect in the current state.
    fn command_available(&self, action: Action) -> bool {
        let console = self.console();
        let in_table = self.table_active.is_some();
        let idle = !console.is_busy();
        let connection = self.active_connection();
        let transactions = connection.is_none_or(|item| item.capabilities.transactions);
        let namespace = connection.is_some_and(|item| item.capabilities.namespace != Namespace::None);
        match action {
            Action::OpenPalette => false,
            Action::Run | Action::RunScript => !in_table && idle,
            Action::Stop => self.table().map_or(!idle, |tab| tab.is_busy()),
            Action::ToggleMode => !in_table && idle && transactions,
            Action::Commit | Action::Rollback => !in_table && idle && console.in_transaction(),
            Action::ChooseSchema => !in_table && idle && namespace && !console.in_transaction(),
            Action::RenameConsole | Action::ChangeConnection => !in_table,
            Action::FocusLeft => !self.navigator.collapsed && !self.navigator_focused,
            Action::FocusRight => self.navigator_focused,
            _ => true,
        }
    }

    fn open_palette(&mut self) {
        self.console_mut().editor.close_suggestions();
        let global = self.keymap.actions(Region::Global).map(|action| (action, true));
        let explorer = [Action::NavigatorAdd, Action::NavigatorScripts].map(|action| (action, self.navigator_focused));
        let commands = global
            .chain(explorer)
            .filter(|(action, _)| self.command_available(*action))
            .map(|(action, key_works)| Command {
                action,
                label: text::command_text(action),
                // A key of the explorer works only there, so another panel does not show it.
                key: if key_works { self.shown_key(action) } else { None },
            })
            .collect();
        self.palette = Some(Palette::new(commands));
    }

    fn on_palette_key(&mut self, key: KeyEvent) {
        let Some(palette) = &mut self.palette else { return };
        let request = palette.on_key(key);
        self.on_palette_request(request);
    }

    pub(super) fn on_palette_request(&mut self, request: PaletteRequest) {
        match request {
            PaletteRequest::None => {}
            PaletteRequest::Cancel => self.palette = None,
            PaletteRequest::Run(action) => {
                self.palette = None;
                self.perform(action, false);
            }
        }
    }

    /// Returns the first screen of a console without text.
    pub(super) fn welcome(&self) -> Option<Welcome> {
        if !self.console().editor.is_blank() {
            return None;
        }
        let keyed = |action, label| self.shown_key(action).map(|key| (key, label));
        let mut rows = Vec::new();
        let note = if self.connections.is_empty() {
            let add = self.shown_key(Action::FocusLeft).zip(self.shown_key(Action::NavigatorAdd));
            rows.extend(add.map(|(focus, add)| (format!("{focus} {add}"), text::WELCOME_ADD)));
            text::WELCOME_NO_CONNECTION
        } else {
            rows.extend(keyed(Action::Run, text::WELCOME_RUN));
            rows.extend(keyed(Action::RunScript, text::WELCOME_SCRIPT));
            rows.push((text::WELCOME_SUGGESTIONS_KEY.to_owned(), text::WELCOME_SUGGESTIONS));
            text::WELCOME_WRITE
        };
        rows.extend(keyed(Action::OpenPalette, text::WELCOME_COMMANDS));
        rows.extend(keyed(Action::Help, text::WELCOME_KEYS));
        Some(Welcome { note, rows })
    }

    fn editor_hint(&self) -> String {
        let start = self.hint_of(&[(Action::Run, "run"), (Action::RunScript, "script")]);
        let help = self.hint_of(&[(Action::Help, "keys")]);
        [start.as_str(), text::HINT_CTRL_SPACE, help.as_str()]
            .iter()
            .filter(|part| !part.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" · ")
    }

    fn running_hint(&self) -> String {
        let stop = self.hint_of(&[(Action::Stop, "stop")]);
        if stop.is_empty() { text::HINT_EDITOR_STAYS.to_owned() } else { format!("{stop} · {}", text::HINT_EDITOR_STAYS) }
    }

    /// Returns the text of the hint bar, with the keys of the active key map.
    pub fn hint(&self) -> String {
        let console = self.console();
        let fixed = if !self.dialogs.is_empty() {
            text::HINT_DIALOG
        } else if self.auth.prompt.is_some() {
            text::AUTH_PASSWORD_HINT
        } else if self.rename.is_some() {
            text::HINT_NAME
        } else if self.settings_form.is_some() {
            text::HINT_SETTINGS
        } else if self.conflicts.is_some() {
            text::HINT_CONFLICTS
        } else if let Some(picker) = &self.picker {
            match picker.kind {
                PickerKind::Consoles { .. } | PickerKind::Orphans { .. } => text::HINT_PICKER,
                _ => text::HINT_CHOOSE,
            }
        } else if self.overlay.is_some() {
            text::HINT_OVERLAY
        } else if self.form.is_some() {
            text::HINT_FORM
        } else if self.palette.is_some() {
            text::HINT_PALETTE
        } else if self.pending_key.is_some_and(Chord::is_prefix) {
            text::HINT_PREFIX
        } else if self.persist.as_ref().is_some_and(|persist| persist.error.is_some()) {
            text::HINT_CONFIG_ERROR
        } else if self.navigator_focused {
            return self
                .hint_parts(&[
                    (Action::NavigatorOpen, "open"),
                    (Action::NavigatorNewConsole, "new"),
                    (Action::NavigatorScripts, "scripts"),
                    (Action::NavigatorAdd, "add"),
                    (Action::NavigatorEdit, "edit"),
                    (Action::NavigatorRefresh, "refresh"),
                    (Action::NavigatorDisconnect, "disc"),
                    (Action::NavigatorDelete, "drop"),
                    (Action::NavigatorDeletePassword, "pwd"),
                ])
                .join("·");
        } else if let Some(tab) = self.table() {
            match tab.focus {
                _ if tab.list.is_some() => text::HINT_LIST,
                TableFocus::Grid if tab.result.search.is_some() => text::HINT_GRID_SEARCH,
                TableFocus::Grid => {
                    return self.hint_of(&[
                        (Action::GridFocusFilter, "where"),
                        (Action::GridFocusOrder, "order by"),
                        (Action::GridSortColumn, "sort"),
                        (Action::GridLoadMore, "more"),
                        (Action::GridCopy, "copy"),
                        (Action::GridOpenValue, "value"),
                        (Action::Help, "keys"),
                    ]);
                }
                _ => text::HINT_FILTER,
            }
        } else if console.focus == Focus::Results && console.search_is_open() {
            text::HINT_GRID_SEARCH
        } else if console.focus == Focus::Results {
            return self.hint_of(&[
                (Action::FocusUp, "editor"),
                (Action::GridCopy, "copy"),
                (Action::GridOpenValue, "value"),
                (Action::GridFind, "find"),
                (Action::GridNextResult, "tabs"),
                (Action::Help, "keys"),
            ]);
        } else if console.editor.suggestions().is_some() {
            text::HINT_LIST
        } else if console.editor.search().is_some() {
            text::HINT_SEARCH
        } else if console.is_busy() {
            return self.running_hint();
        } else {
            return self.editor_hint();
        };
        fixed.to_owned()
    }
}
