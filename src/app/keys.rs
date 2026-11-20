//! Keys, in the priority of `docs/KEYMAP.md`, and the actions of the shell.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::modal::Modal;
use super::run_sql::RunTarget;
use super::tables::Tabs;
use super::{App, Overlay, Screen};
use crate::catalog::Catalog;
use crate::connection::Connection;
use crate::connector::TransactionCommand;
use crate::console::{Focus, GridAction};
use crate::editor::{KeyboardMode, RunScope};
use crate::exec::ConnectionId;
use crate::form::FormRequest;
use crate::input::{Action, Chord, Lookup, Region};
use crate::settings::SettingsRequest;
use crate::store::config::ExplorerSide;
use crate::table::TableFocus;
use crate::ui::kit::prompt::TextAction;
use crate::ui::text;
use crate::ui::wrap::WrappedText;

/// Returns the catalog of the console: the catalog of its connection, or the fallback
/// catalog when the console has no connection. The borrowed fields serve the call of
/// the editor with the catalog at hand.
fn editor_catalog<'a>(
    connections: &'a [Connection],
    connection: Option<ConnectionId>,
    console_connection: Option<ConnectionId>,
    fallback: Option<&'a Catalog>,
) -> Option<&'a Catalog> {
    match connection {
        Some(id) => App::catalog_of(connections, id),
        None if console_connection.is_none() => fallback,
        None => None,
    }
}

impl App {
    pub(super) fn vim(&self) -> bool {
        self.keyboard == KeyboardMode::Vim
    }

    /// Returns the panel that receives keys, without a text field.
    pub(super) fn panel(&self) -> Option<Region> {
        if self.navigator_focused {
            return (!self.navigator.filter_is_editing()).then_some(Region::Navigator);
        }
        if self.diagram_index().is_some() {
            return Some(Region::Diagram);
        }
        if let Some(tab) = self.compare() {
            return tab.result.search.is_none().then_some(Region::Grid);
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
        if self.navigator_focused || self.diagram_index().is_some() {
            return false;
        }
        if self.compare_index().is_some() {
            return true;
        }
        match self.table() {
            Some(tab) => tab.focus == TableFocus::Grid,
            None => self.console().focus == Focus::Results,
        }
    }

    /// Applies the key priority of `docs/KEYMAP.md`.
    /// Gives a key to a window over the screen.
    fn on_modal_key(&mut self, modal: Modal, key: KeyEvent) {
        match modal {
            Modal::Dialog => self.on_dialog_key(key),
            Modal::Password => self.on_password_key(key),
            Modal::Conflicts => self.on_conflicts_key(key),
            Modal::Palette => self.on_palette_key(key),
            Modal::History => self.on_history_key(key),
            Modal::Rename => self.on_name_key(key),
            Modal::Browser => self.on_browser_key(key),
            Modal::Settings => self.on_settings_key(key),
            Modal::Export => self.on_export_key(key),
            Modal::Edit => self.on_edit_key(key),
            Modal::Import => self.on_import_key(key),
            Modal::Compare => self.on_compare_key(key),
            Modal::Picker => self.on_picker_key(key),
            Modal::Help => self.on_help_key(key),
            Modal::Overlay => self.on_overlay_key(key),
            Modal::Form => self.on_form_key(key),
        }
    }

    pub(super) fn on_key(&mut self, key: KeyEvent) {
        // The AI prompts get the keys under the question dialog and the password.
        if let Some(modal @ (Modal::Dialog | Modal::Password)) = self.top_modal() {
            self.on_modal_key(modal, key);
            return;
        }
        if self.on_ai_key(key) {
            return;
        }
        if let Some(modal) = self.top_modal() {
            self.on_modal_key(modal, key);
            return;
        }
        let in_editor = !self.navigator_focused
            && self.console_on_screen()
            && !self.home()
            && self.console().focus == Focus::Editor;
        let keys = self.suggestion_keys;
        if (in_editor && self.console_mut().editor.on_list_key(key, keys))
            || self.on_table_list_key(key)
        {
            return;
        }
        let chord = Chord::from_event(key);
        if let Some(region) = self.input.command_page.take() {
            self.on_command_page_key(region, chord);
            return;
        }
        if let Some((region, mut keys)) = self.input.pending_key.take() {
            keys.push(chord);
            self.on_sequence(region, keys);
            return;
        }
        if self.on_review_key(key) {
            return;
        }
        if !self.navigator_focused
            && self.console_on_screen()
            && self.console().focus == Focus::Parameters
        {
            let plain = (key.modifiers - KeyModifiers::SHIFT).is_empty();
            if key.code == KeyCode::Esc {
                self.console_mut().focus = Focus::Editor;
                return;
            }
            // Field navigation and plain text must not trigger global navigation bindings.
            if plain
                && matches!(
                    key.code,
                    KeyCode::Char(_)
                        | KeyCode::Tab
                        | KeyCode::Enter
                        | KeyCode::Left
                        | KeyCode::Right
                        | KeyCode::Up
                        | KeyCode::Down
                        | KeyCode::Home
                        | KeyCode::End
                        | KeyCode::PageUp
                        | KeyCode::PageDown
                        | KeyCode::Backspace
                        | KeyCode::Delete
                )
                || key.code == KeyCode::BackTab
            {
                self.console_mut().parameters.on_key(key);
                return;
            }
            if !self.on_chord(Region::Global, chord) {
                self.console_mut().parameters.on_key(key);
            }
            return;
        }
        let panel = self.panel();
        let editor_chord = if !self.vim()
            && matches!(key.code, KeyCode::Char('z' | 'Z'))
            && key
                .modifiers
                .contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT)
        {
            Chord::ctrl('y')
        } else {
            chord
        };
        let editor_key = self.editor_has_focus()
            && !self.is_text_key(key, panel)
            && self.console().editor.search().is_none();
        if editor_key && self.on_chord(Region::Editor, editor_chord) {
            return;
        }
        if let Some(region) = panel
            && self.on_chord(region, chord)
        {
            return;
        }
        if !self.is_text_key(key, panel) && self.on_chord(Region::Global, chord) {
            return;
        }
        // Do not send a removed application binding to the editor's built-in key handler.
        if editor_key
            && Action::in_region(Region::Editor).any(|action| {
                action
                    .default_keys(self.keyboard)
                    .iter()
                    .any(|key| Chord::parse(key).ok() == Some(editor_chord))
            })
        {
            return;
        }
        match panel {
            Some(Region::Grid) => self.on_shifted_grid_key(chord),
            Some(_) => {}
            None if self.navigator_focused => self.on_tree_filter_key(key),
            // The home screen has no text field.
            None if self.home() => {}
            None if self.compare_index().is_some() => {
                if let Some(tab) = self.compare_mut() {
                    tab.result.on_search_key(key);
                }
            }
            None if self.table_index().is_some() => self.on_table_text_key(key),
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
                if region == Region::Editor
                    && self.vim()
                    && self.console().editor.is_inserting()
                    && action == Action::EditorRedo
                {
                    self.console_mut().editor.close_suggestions();
                } else {
                    self.perform(action, false);
                }
                true
            }
            Lookup::Sequence => {
                self.input.pending_key = Some((region, vec![chord]));
                true
            }
            Lookup::None => false,
        }
    }

    /// Runs a panel command. A key without a command closes the page.
    fn on_command_page_key(&mut self, region: Region, chord: Chord) {
        if region != Region::Navigator && self.context_region() != Some(region) {
            return;
        }
        if chord == Chord::ESC {
            return;
        }
        match self.keymap.lookup(region, chord) {
            Lookup::Action(action) if self.context_commands(region).contains(&action) => {
                self.perform(action, false)
            }
            Lookup::Sequence
                if self.context_commands(region).iter().any(|action| {
                    self.keymap.keys(*action).iter().any(|key| {
                        key.split_whitespace()
                            .next()
                            .and_then(|key| Chord::parse(key).ok())
                            == Some(chord)
                    })
                }) =>
            {
                self.input.pending_key = Some((region, vec![chord]))
            }
            _ => {}
        }
    }

    fn on_sequence(&mut self, region: Region, keys: Vec<Chord>) {
        if !matches!(region, Region::Global | Region::Navigator)
            && self.context_region() != Some(region)
        {
            return;
        }
        if keys.last() == Some(&Chord::ESC) {
            return;
        }
        let mut found = self.keymap.sequence(region, &keys);
        if found == Lookup::None && region != Region::Global {
            found = self.keymap.sequence(Region::Global, &keys);
        }
        match found {
            Lookup::Action(action)
                if region == Region::Global || self.command_available(action) =>
            {
                self.perform(action, false)
            }
            Lookup::Action(_) => {}
            Lookup::Sequence => self.input.pending_key = Some((region, keys)),
            Lookup::None => {}
        }
    }

    pub(super) fn pending_prefix(&self) -> bool {
        self.input
            .pending_key
            .as_ref()
            .is_some_and(|(_, keys)| keys.len() == 1 && keys[0].is_prefix())
    }

    fn open_command_page(&mut self, region: Option<Region>) {
        self.console_mut().editor.close_suggestions();
        if let Some(tab) = self.table_mut() {
            tab.list = None;
        }
        self.input.command_page = region;
    }

    fn perform_editor_command(&mut self, action: Action) {
        if !self.editor_has_focus() {
            return;
        }
        self.ensure_active_catalog();
        let index = self.tabs.active;
        let connection = self.catalog_for(index);
        let App {
            connections,
            fallback_catalog,
            tabs: Tabs { consoles, .. },
            ..
        } = self;
        let catalog = editor_catalog(
            connections,
            connection,
            consoles[index].connection,
            fallback_catalog.as_ref(),
        );
        consoles[index].editor.on_command(action, catalog);
        self.request_missing_columns(index);
    }

    /// Shift changes a move of the grid into a selection.
    fn on_shifted_grid_key(&mut self, chord: Chord) {
        let Some(plain) = chord.without_shift() else {
            return;
        };
        if let Lookup::Action(action) = self.keymap.lookup(Region::Grid, plain)
            && action.extends_selection()
        {
            self.perform(action, true);
        }
    }

    fn on_editor_key(&mut self, key: KeyEvent) {
        self.ensure_active_catalog();
        let index = self.tabs.active;
        let connection = self.catalog_for(index);
        // The catalog and the console are separate fields of the application.
        let App {
            connections,
            fallback_catalog,
            tabs: Tabs { consoles, .. },
            ..
        } = self;
        let catalog = editor_catalog(
            connections,
            connection,
            consoles[index].connection,
            fallback_catalog.as_ref(),
        );
        consoles[index].editor.on_key(key, catalog);
        self.request_missing_columns(index);
    }

    pub(super) fn perform(&mut self, action: Action, extend: bool) {
        if action.region() == Region::Review {
            self.perform_review(action);
            return;
        }
        if self.review_blocks(action) {
            self.refuse_review_edit();
            return;
        }
        if self
            .table()
            .is_some_and(|tab| !tab.result.edits.is_empty() || tab.edit_uncertain)
            && matches!(
                action,
                Action::GridCopy
                    | Action::GridExport
                    | Action::GridImport
                    | Action::GridApply
                    | Action::GridClearFilters
                    | Action::GridSortColumn
                    | Action::GridLoadMore
                    | Action::GridPreviousResult
                    | Action::GridNextResult
                    | Action::GridReload
                    | Action::GridFollowKey
                    | Action::FollowKey
            )
            && self.pending_edit_guard()
        {
            return;
        }
        if self.home() && !self.home_allows(action) {
            return;
        }
        if matches!(
            action,
            Action::GridProfile
                | Action::GridChart
                | Action::GridKeepPrevious
                | Action::GridComparePrevious
        ) {
            self.perform_analysis(action);
            return;
        }
        let done = match self.tabs.screen {
            Screen::Diagram(_) => self.perform_on_diagram(action),
            Screen::Compare(_) => self.perform_on_compare(action),
            Screen::Table(_) => self.perform_on_table(action),
            Screen::Console => false,
        };
        if done {
            return;
        }
        match action {
            Action::EditorAiComplete => self.start_ai_with(None),
            Action::EditorAiCompleteRequest => self.open_ai_request(),
            Action::AiExplainError => self.start_ai_explain_error(),
            Action::AiExplainPlan => self.start_ai_explain_plan(),
            Action::EditorAiAccept => self.decide_ghost(true),
            Action::EditorAiReject => self.decide_ghost(false),
            Action::EditorAiCancel => self.cancel_ai(),
            Action::AgentSessions => self.open_agent_sessions(),
            Action::AgentReview => self.open_agent_proposals(),
            Action::AgentDirect => self.toggle_agent_direct(),
            Action::AgentStop => self.stop_agent_tasks(),
            Action::AgentSqlToConsole => self.agent_sql_to_console(),
            Action::AgentSqlClear => self.agent_sql_clear(),
            Action::AutoRefresh => self.open_auto_refresh(),
            Action::AgentSession => self.open_tab_agent_session(),
            Action::AgentTasks => self.open_agent_tasks(),
            Action::Run => self.run(RunTarget::Statements(RunScope::Default)),
            Action::RunScript => self.run(RunTarget::Statements(RunScope::Script)),
            Action::RunSubquery => {
                if self.command_available(action) {
                    self.run(RunTarget::Statements(RunScope::Subquery));
                } else {
                    self.status = Some(text::SUBQUERY_UNAVAILABLE.to_owned());
                }
            }
            Action::Explain => self.run(RunTarget::Plan { analyze: false }),
            Action::ExplainAnalyze => self.run(RunTarget::Plan { analyze: true }),
            Action::FormatSql => self.format_console(),
            Action::SqlDiagnostics => self.show_sql_diagnostics(),
            Action::FocusParameters => {
                self.navigator_focused = false;
                let console = self.console_mut();
                console.parameters.collapsed = false;
                console.editor.close_suggestions();
                console.focus = Focus::Parameters;
            }
            Action::ToggleParameters => {
                let console = self.console_mut();
                console.parameters.collapsed = !console.parameters.collapsed;
                if console.parameters.collapsed && console.focus == Focus::Parameters {
                    console.focus = Focus::Editor;
                }
            }
            Action::ToggleComment => self.comment_console(),
            Action::OpenHistory => self.open_history(),
            Action::OpenSessions => self.open_sessions(),
            Action::OpenSqlFile => self.start_open_sql_file(),
            Action::OpenDiagram => self.open_diagram_here(),
            Action::OpenCompare => self.open_compare_here(),
            // A table tab follows the key before this match.
            Action::FollowKey => self.status = Some(text::FOLLOW_REFUSED.to_owned()),
            Action::ExplorerCommands => self.open_command_page(Some(Region::Navigator)),
            Action::ContextCommands => self.open_command_page(self.context_region()),
            Action::EditorSave => self.save_now(),
            action if action.region() == Region::Editor => self.perform_editor_command(action),
            Action::Stop => self.stop(self.tabs.active),
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
            Action::GridExport => self.open_export(),
            Action::GridImport => self.open_import(),
            Action::FocusLeft | Action::FocusRight if action == self.explorer_entry() => {
                self.focus_navigator();
            }
            Action::FocusLeft | Action::FocusRight => self.navigator_focused = false,
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
            | Action::NavigatorDeletePassword
            | Action::NavigatorStructure
            | Action::NavigatorDiagram
            | Action::NavigatorCompare
            | Action::NavigatorSessions
            | Action::NavigatorGenerate
            | Action::NavigatorImport
            | Action::NavigatorCopyName
            | Action::NavigatorMoveUp
            | Action::NavigatorMoveDown
            | Action::NavigatorMoveToFolder
            | Action::NavigatorFilter
            | Action::NavigatorClearFilter => self.on_navigator_action(action),
            _ => self.on_grid_action(action, extend),
        }
    }

    fn on_grid_action(&mut self, action: Action, extend: bool) {
        let budget = self.budget.clone();
        let vim = self.vim();
        let outcome = match self.tabs.screen {
            Screen::Compare(index) => self.tabs.compares[index]
                .result
                .on_action(action, extend, vim),
            Screen::Table(index) => self.tabs.tables[index]
                .result
                .on_action(action, extend, vim),
            Screen::Console | Screen::Diagram(_) => {
                self.console_mut().on_results_action(action, extend)
            }
        };
        match outcome {
            GridAction::None => {}
            GridAction::Copy(tsv) => self.copy(&tsv),
            GridAction::OpenValue {
                title,
                text,
                footer,
                json,
            } => {
                let formatted = self.persist.as_ref().is_none_or(|item| item.format_json);
                let (mut contents, footer, other) = match json {
                    Some((json, json_footer)) if formatted => (
                        WrappedText::new(json),
                        json_footer,
                        Some((text.source().to_text(), footer)),
                    ),
                    Some((json, json_footer)) => (
                        WrappedText::from_source(text.source()),
                        footer,
                        Some((json, json_footer)),
                    ),
                    None => (WrappedText::from_source(text.source()), footer, None),
                };
                if other.is_some() {
                    contents.highlight_json();
                }
                self.modals.overlay = Some(Overlay {
                    _analysis_memory: None,
                    _ai_memory: None,
                    title,
                    contents,
                    footer,
                    scroll: 0,
                    dragging: false,
                    other,
                    row: None,
                });
            }
            GridAction::OpenRow(row) => self.show_row(row, 0),
            GridAction::CloseResult => {
                let held = self.has_export(self.console().id);
                if held || !self.console_mut().close_result(&budget) {
                    self.status = Some(text::RESULT_READING.to_owned());
                }
            }
        }
    }

    /// Opens the window of a row of the grid on screen.
    fn show_row(&mut self, row: usize, scroll: usize) {
        let window = self.active_result().and_then(|item| item.row_window(row));
        let Some(window) = window else { return };
        self.modals.overlay = Some(Overlay {
            _analysis_memory: None,
            _ai_memory: None,
            title: window.title,
            contents: WrappedText::with_indent(window.text, window.indent),
            footer: window.footer,
            scroll,
            dragging: false,
            other: None,
            row: Some(row),
        });
    }

    /// Shows the row before or after the row of the window. It stops at the loaded rows,
    /// and the cursor of the grid goes with it. The window keeps its scroll, so that
    /// the same columns stay in sight.
    fn step_row_window(&mut self, forward: bool) {
        let Some((row, scroll)) = self
            .modals
            .overlay
            .as_ref()
            .and_then(|overlay| Some((overlay.row?, overlay.scroll)))
        else {
            return;
        };
        let next = if forward {
            row + 1
        } else {
            row.saturating_sub(1)
        };
        let result = match self.tabs.screen {
            Screen::Compare(index) => Some(&mut self.tabs.compares[index].result),
            Screen::Table(index) => Some(&mut self.tabs.tables[index].result),
            Screen::Console | Screen::Diagram(_) => {
                let console = &mut self.tabs.consoles[self.tabs.active];
                console.results.get_mut(console.active)
            }
        };
        let Some(result) = result.filter(|item| next != row && next < item.table.rows.len()) else {
            return;
        };
        result
            .view
            .move_to(next, result.view.col, &result.table, false);
        self.show_row(next, scroll);
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

    /// Returns the focus key toward the side of the explorer. The other focus key
    /// leaves the explorer.
    pub(super) fn explorer_entry(&self) -> Action {
        match self.explorer_side {
            ExplorerSide::Left => Action::FocusLeft,
            ExplorerSide::Right => Action::FocusRight,
        }
    }

    /// Returns true for an action that acts on the home screen: a command of the
    /// explorer, the focus key toward the explorer, or a command that needs no tab.
    pub(super) fn home_allows(&self, action: Action) -> bool {
        action == self.explorer_entry() || home_allows(action)
    }

    fn toggle_navigator(&mut self) {
        self.navigator.collapsed = !self.navigator.collapsed;
        if self.navigator.collapsed {
            self.navigator_focused = false;
        } else if self.home() {
            self.focus_navigator();
        }
    }

    /// The user saw the end of the run of the tab on screen, so its mark goes.
    pub(super) fn leave_tab(&mut self) {
        match self.screen_item_mut() {
            Some(tab) => tab.run_mut().seen(),
            None => self.console_mut().run.seen(),
        }
    }

    pub(super) fn select_console(&mut self, index: usize) {
        if index < self.tabs.consoles.len() {
            self.console_mut().editor.close_suggestions();
            if !self.console_on_screen() || self.tabs.active != index {
                self.leave_tab();
            }
            self.tabs.active = index;
            self.tabs.screen = Screen::Console;
            self.sync_command_history();
        }
    }

    fn on_dialog_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        let Some(dialog) = self.modals.dialogs.front_mut() else {
            return;
        };
        let count = dialog.options.len();
        match key.code {
            KeyCode::Tab | KeyCode::Right => dialog.focus = (dialog.focus + 1) % count,
            KeyCode::BackTab | KeyCode::Left => dialog.focus = (dialog.focus + count - 1) % count,
            KeyCode::Char('l') if vim => dialog.focus = (dialog.focus + 1) % count,
            KeyCode::Char('h') if vim => dialog.focus = (dialog.focus + count - 1) % count,
            KeyCode::Enter => {
                let action = dialog.options[dialog.focus].1.clone();
                self.modals.dialogs.pop_front();
                self.on_dialog_action(action);
            }
            KeyCode::Esc => {
                // The first option never changes the database or the disk.
                let action = dialog.options[0].1.clone();
                self.modals.dialogs.pop_front();
                self.on_dialog_action(action);
            }
            _ => {}
        }
    }

    fn on_picker_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        let Some(picker) = &mut self.modals.picker else {
            return;
        };
        let last = picker.items.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc => self.modals.picker = None,
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
        let Some(prompt) = &mut self.modals.rename else {
            return;
        };
        match prompt.on_key(key) {
            TextAction::None => {}
            TextAction::Cancel => self.modals.rename = None,
            TextAction::Accept(name) => self.accept_name(name),
        }
    }

    fn on_settings_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        let Some(form) = &mut self.modals.settings_form else {
            return;
        };
        match form.on_key(key, vim) {
            SettingsRequest::None => {}
            SettingsRequest::Cancel => self.close_settings(),
            SettingsRequest::AiTest => self.test_ai_connection(),
            SettingsRequest::Save => self.save_settings(),
        }
    }

    fn on_export_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        if let Some(form) = &mut self.modals.export_form {
            let request = form.on_key(key, vim);
            self.on_export_request(request);
        }
    }

    fn on_import_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        if let Some(form) = &mut self.modals.import_form {
            let action = form.on_key(key, vim);
            self.on_import_action(action);
        }
    }

    fn on_form_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        let Some(form) = &mut self.modals.form else {
            return;
        };
        match form.on_key(key, vim) {
            FormRequest::None => {}
            FormRequest::Cancel => self.modals.form = None,
            FormRequest::Test => self.test_form(),
            FormRequest::Save => self.save_form(),
            FormRequest::Disconnect => self.disconnect_form(),
            FormRequest::Browse => self.open_form_window(),
        }
    }

    pub(super) fn continue_overlay_search(&mut self, deadline: std::time::Instant) {
        if let Some(overlay) = &mut self.modals.overlay
            && let Some(row) = overlay.contents.continue_search(deadline)
        {
            overlay.scroll = row;
        }
    }

    fn on_overlay_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') && self.pending_edit_guard() {
            return;
        }
        let mut find = ctrl && key.code == KeyCode::Char('f');
        let searching = self
            .modals
            .overlay
            .as_ref()
            .is_some_and(|overlay| overlay.contents.search().is_some());
        if !searching && !find {
            let mut keys = self
                .input
                .pending_key
                .take()
                .map_or_else(Vec::new, |(_, keys)| keys);
            if key.code == KeyCode::Esc && !keys.is_empty() {
                return;
            }
            keys.push(Chord::from_event(key));
            let mut partial = false;
            for binding in self.keymap.keys(Action::EditorFind) {
                let Ok(binding) = binding
                    .split_whitespace()
                    .map(Chord::parse)
                    .collect::<Result<Vec<_>, _>>()
                else {
                    continue;
                };
                if binding == keys {
                    find = true;
                    break;
                }
                partial |= binding.starts_with(&keys);
            }
            if partial && !find {
                self.input.pending_key = Some((Region::Editor, keys));
                return;
            }
        }
        let Some(overlay) = &mut self.modals.overlay else {
            return;
        };
        if find {
            self.input.pending_key = None;
            overlay.dragging = false;
            overlay.contents.open_search();
            return;
        }
        if overlay.contents.on_search_key(key) {
            return;
        }
        if key.modifiers.contains(KeyModifiers::SHIFT) {
            let direction = match key.code {
                KeyCode::Left => Some((-1, 0)),
                KeyCode::Right => Some((1, 0)),
                KeyCode::Up => Some((0, -1)),
                KeyCode::Down => Some((0, 1)),
                _ => None,
            };
            if let Some((horizontal, vertical)) = direction {
                let row = overlay.contents.extend_selection(horizontal, vertical);
                let height = usize::from(self.layout.overlay.height).max(1);
                if row < overlay.scroll {
                    overlay.scroll = row;
                } else if row >= overlay.scroll + height {
                    overlay.scroll = row + 1 - height;
                }
                return;
            }
        }
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.modals.overlay = None;
                self.analysis_job = None;
            }
            KeyCode::Up | KeyCode::Char('k') => overlay.scroll = overlay.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => overlay.scroll = overlay.scroll.saturating_add(1),
            KeyCode::PageUp => overlay.scroll = overlay.scroll.saturating_sub(10),
            KeyCode::PageDown => overlay.scroll = overlay.scroll.saturating_add(10),
            KeyCode::Char('f') if !ctrl => overlay.switch_form(),
            KeyCode::Left | KeyCode::Char('h') => self.step_row_window(false),
            KeyCode::Right | KeyCode::Char('l') => self.step_row_window(true),
            KeyCode::Char('c') if ctrl => {
                let contents = overlay.contents.copy_text();
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
}

/// Returns true for a command of the explorer, or a command that needs no tab.
fn home_allows(action: Action) -> bool {
    if matches!(action, Action::AgentSessions | Action::AgentStop) {
        return true;
    }
    use Action::*;
    action.region() == Region::Navigator
        || matches!(
            action,
            Help | OpenPalette
                | Quit
                | ToggleNavigator
                | ReloadConfig
                | OpenSettings
                | ExplorerCommands
                | ContextCommands
                | OpenSessions
                | OpenDiagram
                | OpenCompare
                | OpenSqlFile
        )
}
