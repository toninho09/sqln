//! Keys, in the priority of `docs/KEYMAP.md`, and the actions of the shell.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::compare::compare_has;
use super::guard::short;
use super::tables::has_sessions;
use super::{App, DialogAction, Overlay};
use crate::connector::{Namespace, TransactionCommand, TransactionState};
use crate::console::{AfterTransaction, Focus, GridAction, Phase};
use crate::editor::{FormatOutcome, KeyboardMode, RunPlan, RunRefusal, RunScope};
use crate::exec::{ConnectionId, RunOptions};
use crate::form::FormRequest;
use crate::grid::printable;
use crate::help::Help;
use crate::input::{Action, Chord, Lookup, Region};
use crate::navigator::RowKind;
use crate::palette::{Command, Palette, PaletteRequest};
use crate::plan::Explain;
use crate::settings::SettingsRequest;
use crate::sql::{Dialect, first_keyword, is_plain_read};
use crate::table::TableFocus;
use crate::ui::console::fit;
use crate::ui::name::NameAction;
use crate::ui::text;
use crate::ui::welcome::Welcome;
use crate::ui::which_key::KeyGroup;
use crate::ui::wrap::WrappedText;

/// The actions of one row of the `Ctrl+W` menu, with its label.
type MenuRow = (&'static [Action], &'static str);
/// The title of one group of the `Ctrl+W` menu, with its rows.
type MenuGroup = (&'static str, &'static [MenuRow]);

/// The longest name of a row in the title of the explorer page.
const EXPLORER_TITLE_NAME: usize = 48;

/// What a run starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RunTarget {
    Statements(RunScope),
    /// The plan of one statement. With `analyze`, the database also runs it.
    Plan {
        analyze: bool,
    },
}

impl App {
    pub(super) fn vim(&self) -> bool {
        self.keyboard == KeyboardMode::Vim
    }

    /// Returns the panel that receives keys, without a text field.
    fn panel(&self) -> Option<Region> {
        if self.navigator_focused {
            return (!self.navigator.filter_is_editing()).then_some(Region::Navigator);
        }
        if self.diagram_active.is_some() {
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
        if self.navigator_focused || self.diagram_active.is_some() {
            return false;
        }
        if self.compare_active.is_some() {
            return true;
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
        if self.on_ai_key(key) {
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
        if self.history.is_some() {
            self.on_history_key(key);
            return;
        }
        if self.rename.is_some() {
            self.on_name_key(key);
            return;
        }
        if self.browser.is_some() {
            self.on_browser_key(key);
            return;
        }
        if self.settings_form.is_some() {
            self.on_settings_key(key);
            return;
        }
        if self.export_form.is_some() {
            self.on_export_key(key);
            return;
        }
        if self.edit_form.is_some() {
            self.on_edit_key(key);
            return;
        }
        if self.import_form.is_some() {
            self.on_import_key(key);
            return;
        }
        if self.compare_form.is_some() {
            self.on_compare_key(key);
            return;
        }
        if self.picker.is_some() {
            self.on_picker_key(key);
            return;
        }
        if let Some(help) = &mut self.help {
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                let contents = help.text();
                self.copy(&contents);
            } else if help.on_key(key) {
                self.help = None;
            }
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
        if let Some(region) = self.command_page.take() {
            self.on_command_page_key(region, chord);
            return;
        }
        if let Some((region, mut keys)) = self.pending_key.take() {
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
            Chord::parse("ctrl+y").unwrap()
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
            None if self.compare_active.is_some() => {
                if let Some(tab) = self.compare_mut() {
                    tab.result.on_search_key(key);
                }
            }
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
                self.pending_key = Some((region, vec![chord]));
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
        if chord == Chord::parse("esc").unwrap() {
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
                self.pending_key = Some((region, vec![chord]))
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
        if keys.last() == Some(&Chord::parse("esc").unwrap()) {
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
            Lookup::Sequence => self.pending_key = Some((region, keys)),
            Lookup::None => {}
        }
    }

    pub(super) fn pending_prefix(&self) -> bool {
        self.pending_key
            .as_ref()
            .is_some_and(|(_, keys)| keys.len() == 1 && keys[0].is_prefix())
    }

    fn open_command_page(&mut self, region: Option<Region>) {
        self.console_mut().editor.close_suggestions();
        if let Some(tab) = self.table_mut() {
            tab.list = None;
        }
        self.command_page = region;
    }

    fn perform_editor_command(&mut self, action: Action) {
        if !self.editor_has_focus() {
            return;
        }
        self.ensure_active_catalog();
        let index = self.active;
        let connection = self.catalog_for(index);
        let App {
            consoles,
            connections,
            fallback_catalog,
            ..
        } = self;
        let catalog = match connection {
            Some(id) => connections
                .iter()
                .find(|item| item.id == id)
                .and_then(|item| item.catalog.as_ref()),
            None if consoles[index].connection.is_none() => fallback_catalog.as_ref(),
            None => None,
        };
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
        let index = self.active;
        let connection = self.catalog_for(index);
        // The catalog and the console are separate fields of the application.
        let App {
            consoles,
            connections,
            fallback_catalog,
            ..
        } = self;
        let catalog = match connection {
            Some(id) => connections
                .iter()
                .find(|item| item.id == id)
                .and_then(|item| item.catalog.as_ref()),
            None if consoles[index].connection.is_none() => fallback_catalog.as_ref(),
            None => None,
        };
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
        if self.home() && !home_allows(action) {
            return;
        }
        if self.diagram_active.is_some() && self.perform_on_diagram(action) {
            return;
        }
        if self.compare_active.is_some() && self.perform_on_compare(action) {
            return;
        }
        if self.table_active.is_some() && self.perform_on_table(action) {
            return;
        }
        match action {
            Action::EditorAiComplete => self.start_ai_with(None),
            Action::EditorAiCompleteRequest => self.open_ai_request(),
            Action::EditorAiAccept => self.decide_ghost(true),
            Action::EditorAiReject => self.decide_ghost(false),
            Action::EditorAiCancel => self.cancel_ai(),
            Action::AgentSessions => self.open_agent_sessions(),
            Action::AgentReview => self.open_agent_proposals(),
            Action::AgentDirect => self.toggle_agent_direct(),
            Action::AgentStop => self.stop_agent_tasks(),
            Action::AgentSqlToConsole => self.agent_sql_to_console(),
            Action::AgentSqlClear => self.agent_sql_clear(),
            Action::AgentSession => self.open_tab_agent_session(),
            Action::AgentTasks => self.open_agent_tasks(),
            Action::Run => self.run(RunTarget::Statements(RunScope::Default)),
            Action::RunScript => self.run(RunTarget::Statements(RunScope::Script)),
            Action::Explain => self.run(RunTarget::Plan { analyze: false }),
            Action::ExplainAnalyze => self.run(RunTarget::Plan { analyze: true }),
            Action::FormatSql => self.format_console(),
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
            Action::OpenDiagram => self.open_diagram_here(),
            Action::OpenCompare => self.open_compare_here(),
            // A table tab follows the key before this match.
            Action::FollowKey => self.status = Some(text::FOLLOW_REFUSED.to_owned()),
            Action::ExplorerCommands => self.open_command_page(Some(Region::Navigator)),
            Action::ContextCommands => self.open_command_page(self.context_region()),
            Action::EditorSave => self.save_now(),
            action if action.region() == Region::Editor => self.perform_editor_command(action),
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
            Action::GridExport => self.open_export(),
            Action::GridImport => self.open_import(),
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
            | Action::NavigatorDeletePassword
            | Action::NavigatorStructure
            | Action::NavigatorDiagram
            | Action::NavigatorCompare
            | Action::NavigatorSessions
            | Action::NavigatorGenerate
            | Action::NavigatorImport
            | Action::NavigatorCopyName
            | Action::NavigatorFilter
            | Action::NavigatorClearFilter => self.on_navigator_action(action),
            _ => self.on_grid_action(action, extend),
        }
    }

    fn on_grid_action(&mut self, action: Action, extend: bool) {
        let budget = self.budget.clone();
        let vim = self.vim();
        let outcome = match (self.compare_active, self.table_active) {
            (Some(index), _) => self.compares[index].result.on_action(action, extend, vim),
            (None, Some(index)) => self.tables[index].result.on_action(action, extend, vim),
            (None, None) => self.console_mut().on_results_action(action, extend),
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
                let original = (text, footer);
                let formatted = self.persist.as_ref().is_none_or(|item| item.format_json);
                let ((shown, footer), other) = match json {
                    Some(json) if formatted => (json, Some(original)),
                    other => (original, other),
                };
                self.overlay = Some(Overlay {
                    title,
                    contents: WrappedText::new(shown),
                    footer,
                    scroll: 0,
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
        self.overlay = Some(Overlay {
            title: window.title,
            contents: WrappedText::with_indent(window.text, window.indent),
            footer: window.footer,
            scroll,
            other: None,
            row: Some(row),
        });
    }

    /// Shows the row before or after the row of the window. It stops at the loaded rows,
    /// and the cursor of the grid goes with it. The window keeps its scroll, so that
    /// the same columns stay in sight.
    fn step_row_window(&mut self, forward: bool) {
        let Some((row, scroll)) = self
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
        let result = match (self.compare_active, self.table_active) {
            (Some(index), _) => Some(&mut self.compares[index].result),
            (None, Some(index)) => Some(&mut self.tables[index].result),
            (None, None) => {
                let console = &mut self.consoles[self.active];
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
        if let Some(tab) = self.diagram_mut() {
            tab.run.seen();
            return;
        }
        if let Some(tab) = self.compare_mut() {
            tab.run.seen();
            return;
        }
        match self.table_mut() {
            Some(tab) => tab.run.seen(),
            None => self.console_mut().run.seen(),
        }
    }

    pub(super) fn select_console(&mut self, index: usize) {
        if index < self.consoles.len() {
            self.console_mut().editor.close_suggestions();
            if !self.console_on_screen() || self.active != index {
                self.leave_tab();
            }
            self.active = index;
            self.table_active = None;
            self.diagram_active = None;
            self.compare_active = None;
            self.sync_command_history();
        }
    }

    fn open_keys(&mut self) {
        let in_table = self.table_active.is_some() || self.compare_active.is_some();
        // The explorer is the only panel of the home screen.
        let panel = if self.navigator_focused || self.home() {
            Some(Region::Navigator)
        } else if self.context_region() == Some(Region::Review) {
            Some(Region::Review)
        } else {
            self.panel()
        };
        let (region, name) = match panel {
            Some(Region::Navigator) => (Some(Region::Navigator), text::REGION_NAVIGATOR),
            Some(Region::Diagram) => (Some(Region::Diagram), text::REGION_DIAGRAM),
            Some(Region::Review) => (Some(Region::Review), text::REGION_REVIEW),
            Some(_) if self.compare_active.is_some() => (Some(Region::Grid), text::REGION_COMPARE),
            Some(_) if in_table => (Some(Region::Grid), text::REGION_TABLE),
            Some(_) => (Some(Region::Grid), text::REGION_GRID),
            None if in_table => (None, text::REGION_FILTER),
            None => (None, text::REGION_EDITOR),
        };
        let keys = match region {
            Some(region) => self.help_keys(region),
            None => {
                let keys = if in_table {
                    text::FILTER_KEYS
                } else {
                    text::EDITOR_KEYS
                };
                keys.iter()
                    .map(|(key, label)| ((*key).to_owned(), *label))
                    .collect()
            }
        };
        let mut keys = keys;
        if region.is_none() && !in_table {
            keys.splice(0..0, self.help_keys(Region::Editor));
        }
        let mut groups = vec![KeyGroup { title: name, keys }];
        for (action, keys, label) in self.help_actions(Region::Global) {
            use Action::*;
            let title = match action {
                Run | RunScript | Explain | ExplainAnalyze | Stop | FormatSql | ToggleComment
                | OpenHistory => text::GROUP_RUN,
                ToggleMode | Commit | Rollback => text::GROUP_TRANSACTION,
                ChooseSchema | ChangeConnection | OpenSessions | OpenDiagram | OpenCompare
                | FollowKey => text::GROUP_CONNECTION,
                NextTab | PreviousTab | CloseTab | RenameConsole => text::GROUP_TABS,
                FocusLeft | FocusRight | FocusUp | FocusDown | ToggleNavigator
                | ExplorerCommands | ContextCommands => text::GROUP_PANELS,
                _ => text::GROUP_APPLICATION,
            };
            if let Some(group) = groups.iter_mut().find(|group| group.title == title) {
                group.keys.push((keys, label));
            } else {
                groups.push(KeyGroup {
                    title,
                    keys: vec![(keys, label)],
                });
            }
        }
        let mode = if self.vim() {
            text::MODE_VIM
        } else {
            text::MODE_CONVENTIONAL
        };
        self.help = Some(Help::new(text::keys_title(mode, name), groups));
    }

    fn help_keys(&self, region: Region) -> Vec<(String, &'static str)> {
        self.help_actions(region)
            .into_iter()
            .map(|(_, key, label)| (key, label))
            .collect()
    }

    fn help_actions(&self, region: Region) -> Vec<(Action, String, &'static str)> {
        let in_table = self.table_active.is_some() || self.compare_active.is_some();
        let sections = self.table().is_some_and(|tab| tab.section().is_some());
        let in_compare = self.compare_active.is_some();
        self.keymap
            .actions(region)
            .filter(|action| {
                // A compare shows only the grid keys that it has.
                if in_compare && action.region() == Region::Grid {
                    compare_has(*action)
                } else {
                    !action.is_compare_only()
                }
            })
            .filter(|action| {
                if in_table {
                    !action.is_console_only()
                        && (!action.is_table_only() || self.command_available(*action))
                } else {
                    !action.is_table_only()
                }
            })
            .filter_map(|action| {
                let keys = self.keymap.keys(action);
                let label = text::grid_action_text(action, in_table, sections);
                (!keys.is_empty()).then(|| (action, keys.join(", "), label))
            })
            .collect()
    }

    fn run(&mut self, target: RunTarget) {
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
                    (text::OPTION_CONTINUE, DialogAction::RunAnyway(id, target)),
                ],
            );
            return;
        }
        self.start_run(self.active, target);
    }

    pub(super) fn start_run(&mut self, index: usize, target: RunTarget) {
        self.plan_and_run(index, target, true);
    }

    /// Runs after the answer to the question of `run_question`.
    pub(super) fn run_confirmed(&mut self, index: usize, target: RunTarget) {
        self.plan_and_run(index, target, false);
    }

    /// Returns the plan that a target asks for. A plan needs a source with plans and one
    /// statement, and the error is the reason for the status line.
    fn explain_of(
        &self,
        connection: ConnectionId,
        target: RunTarget,
        plan: &RunPlan,
    ) -> Result<Option<Explain>, &'static str> {
        let RunTarget::Plan { analyze } = target else {
            return Ok(None);
        };
        let dialect = self.connection(connection).map(|item| item.dialect);
        let explain = dialect.and_then(|dialect| Explain::new(dialect, analyze));
        let Some(explain) = explain else {
            let sqlite = analyze && dialect == Some(Dialect::Sqlite);
            return Err(if sqlite {
                text::PLAN_ANALYZE_REFUSED
            } else {
                text::PLAN_REFUSED
            });
        };
        match plan.statements.as_slice() {
            [statement]
                if first_keyword(statement, explain.dialect)
                    .is_some_and(|word| word.eq_ignore_ascii_case("EXPLAIN")) =>
            {
                Err(text::PLAN_HAS_EXPLAIN)
            }
            [_] => Ok(Some(explain)),
            _ => Err(text::PLAN_ONE_STATEMENT),
        }
    }

    /// Returns the question before a run. A plan does not run its statement, so it needs
    /// no question. A real run of a statement that can write always asks.
    fn run_question(
        &self,
        connection: ConnectionId,
        target: RunTarget,
        plan: &RunPlan,
    ) -> Option<String> {
        if target == (RunTarget::Plan { analyze: false }) {
            return None;
        }
        let warning = self.run_warning(connection, plan);
        let dialect = self.connection(connection)?.dialect;
        let statement = plan.statements.first()?;
        let writes = matches!(target, RunTarget::Plan { .. })
            && !is_plain_read(statement, dialect)
            && !self.is_read_only(connection);
        if !writes {
            return warning;
        }
        let details = warning.unwrap_or_else(|| short(statement));
        Some(format!("{}\n\n{details}", text::PLAN_ANALYZE_RUNS))
    }

    fn plan_and_run(&mut self, index: usize, target: RunTarget, ask: bool) {
        let scope = match target {
            RunTarget::Statements(scope) => scope,
            RunTarget::Plan { .. } => RunScope::Default,
        };
        let console = &mut self.consoles[index];
        match console.editor.plan_run(scope) {
            Ok(plan) => {
                let Some(connection) = console.connection else {
                    return;
                };
                let dialect = self.consoles[index].editor.dialect();
                let full_text = self.consoles[index].editor.text();
                self.consoles[index]
                    .parameters
                    .update_names(crate::sql::parameters::names(&full_text, dialect));
                let parameters = match self.consoles[index]
                    .parameters
                    .values(&plan.statements, dialect)
                {
                    Ok(parameters) => parameters,
                    Err(reason) => {
                        self.navigator_focused = false;
                        self.consoles[index].focus = Focus::Parameters;
                        self.status = Some(reason);
                        return;
                    }
                };
                if !parameters.is_empty()
                    && !self
                        .connection(connection)
                        .is_some_and(|item| item.capabilities.parameters)
                {
                    self.status =
                        Some("This connector does not support query parameters.".to_owned());
                    return;
                }
                let explain = match self.explain_of(connection, target, &plan) {
                    Ok(explain) => explain,
                    Err(reason) => {
                        self.status = Some(reason.to_owned());
                        return;
                    }
                };
                if ask && let Some(body) = self.run_question(connection, target, &plan) {
                    let id = self.consoles[index].id;
                    self.show_dialog(
                        text::TITLE_CONFIRM_RUN,
                        body,
                        vec![
                            (text::OPTION_CANCEL, DialogAction::Dismiss),
                            (text::OPTION_RUN, DialogAction::RunConfirmed(id, target)),
                        ],
                    );
                    return;
                }
                let console = &self.consoles[index];
                // An agent tab runs in Auto mode: an open transaction would block the agent.
                let manual = console.manual && !self.agent.owns(console.id);
                let schema = console.schema.clone();
                // A console can keep its manual mode when it moves to another source.
                let options = RunOptions {
                    manual: manual && self.has_transactions(connection),
                    schema,
                    only_schema: false,
                    row_cap: None,
                    export: None,
                    untimed: false,
                    parameters,
                };
                let console = &self.consoles[index];
                let revision = console.editor.revision();
                let console = console.id;
                self.authorize(
                    connection,
                    super::auth::Use::Run {
                        console,
                        connection,
                        plan,
                        options,
                        revision,
                        explain,
                    },
                );
            }
            Err(RunRefusal::CursorOutside) => {
                self.status = Some(text::CURSOR_OUTSIDE.to_owned());
            }
            Err(RunRefusal::NoStatement) => self.status = Some(text::NO_STATEMENT.to_owned()),
        }
    }

    fn format_console(&mut self) {
        if !self.command_available(Action::FormatSql) {
            return;
        }
        let message = match self.console_mut().editor.format_sql() {
            Ok(FormatOutcome::Changed) => return,
            Ok(FormatOutcome::Unchanged) => text::FORMAT_UNCHANGED,
            Ok(FormatOutcome::Refused) => text::FORMAT_REFUSED,
            Err(RunRefusal::CursorOutside) => text::CURSOR_OUTSIDE,
            Err(RunRefusal::NoStatement) => text::NO_STATEMENT,
        };
        self.status = Some(message.to_owned());
    }

    fn comment_console(&mut self) {
        if self.command_available(Action::ToggleComment) {
            self.console_mut().editor.toggle_comment();
        }
    }

    pub(super) fn stop(&mut self, index: usize) {
        let id = self.consoles[index].id;
        if self.cancel_agent_console(id) {
            if self.consoles[index].closing && !self.consoles[index].is_busy() {
                self.close_step(id);
            }
            return;
        }
        if self.stop_export(id) {
            return;
        }
        if self.cancel_auth_console(id) {
            if self.consoles[index].closing {
                self.close_step(id);
            }
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
        let Some(dialog) = self.dialogs.front_mut() else {
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
        let Some(picker) = &mut self.picker else {
            return;
        };
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
        let Some(prompt) = &mut self.rename else {
            return;
        };
        match prompt.on_key(key) {
            NameAction::None => {}
            NameAction::Cancel => self.rename = None,
            NameAction::Accept(name) => self.accept_name(name),
        }
    }

    fn on_settings_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        let Some(form) = &mut self.settings_form else {
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
        if let Some(form) = &mut self.export_form {
            let request = form.on_key(key, vim);
            self.on_export_request(request);
        }
    }

    fn on_import_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        if let Some(form) = &mut self.import_form {
            let action = form.on_key(key, vim);
            self.on_import_action(action);
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
            FormRequest::Browse => self.open_form_window(),
        }
    }

    fn on_overlay_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') && self.pending_edit_guard() {
            return;
        }
        let Some(overlay) = &mut self.overlay else {
            return;
        };
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.overlay = None,
            KeyCode::Up | KeyCode::Char('k') => overlay.scroll = overlay.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => overlay.scroll = overlay.scroll.saturating_add(1),
            KeyCode::Char('f') if !ctrl => overlay.switch_form(),
            KeyCode::Left | KeyCode::Char('h') => self.step_row_window(false),
            KeyCode::Right | KeyCode::Char('l') => self.step_row_window(true),
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
    pub(super) fn shown_key(&self, action: Action) -> Option<String> {
        let keys = self.keymap.keys(action);
        let steady = keys
            .iter()
            .find(|key| !matches!(key.as_str(), "ctrl+enter" | "shift+enter"));
        steady.or(keys.first()).map(|key| text::key_label(key))
    }

    /// Joins the second keys of `Ctrl+W` for a group of actions.
    fn prefix_keys(&self, actions: &[Action]) -> String {
        let second = |action: &Action| {
            let keys = self.keymap.keys(*action).iter();
            keys.filter_map(|key| key.strip_prefix("ctrl+w "))
                .map(text::key_label)
                .next()
        };
        actions.iter().filter_map(second).collect()
    }

    /// Returns the title of the explorer page, with the row that its commands act on.
    pub(super) fn explorer_menu_title(&self) -> String {
        let row = self.navigator.current(&self.connections);
        let name = row.as_ref().map(|row| {
            let name = printable(row.name(&self.connections));
            fit(&name, EXPLORER_TITLE_NAME).0
        });
        text::explorer_menu_title(name.as_deref())
    }

    /// Returns the groups of the explorer page. Each command keeps its key of the explorer.
    pub fn explorer_menu(&self) -> Vec<KeyGroup> {
        use Action::*;
        let groups: [MenuGroup; 3] = [
            (
                text::GROUP_CONSOLE,
                &[
                    (&[NavigatorNewConsole], text::MENU_NEW_CONSOLE),
                    (&[NavigatorScripts], text::MENU_SCRIPTS),
                    (&[NavigatorRefresh], text::MENU_REFRESH),
                ],
            ),
            (
                text::GROUP_CONNECTION,
                &[
                    (&[NavigatorEdit], text::MENU_EDIT_CONNECTION),
                    (&[NavigatorAdd], text::MENU_ADD_CONNECTION),
                    (&[NavigatorDelete], text::MENU_REMOVE_CONNECTION),
                    (&[NavigatorDisconnect], text::MENU_DISCONNECT),
                    (&[NavigatorDeletePassword], text::MENU_DELETE_PASSWORD),
                    (&[NavigatorSessions], text::MENU_SESSIONS),
                ],
            ),
            (
                text::GROUP_ROW,
                &[
                    (&[NavigatorOpen], text::MENU_OPEN),
                    (&[NavigatorExpand], text::MENU_EXPAND),
                    (&[NavigatorCollapse], text::MENU_COLLAPSE),
                    (&[NavigatorStructure], text::MENU_STRUCTURE),
                    (&[NavigatorDiagram], text::MENU_DIAGRAM),
                    (&[NavigatorCompare], text::MENU_COMPARE),
                    (&[NavigatorGenerate], text::MENU_GENERATE),
                    (&[NavigatorImport], text::MENU_IMPORT),
                    (&[NavigatorCopyName], text::MENU_COPY_NAME),
                    (&[NavigatorFilter], text::MENU_FILTER),
                    (&[NavigatorClearFilter], text::MENU_CLEAR_FILTER),
                ],
            ),
        ];
        let acting = self.explorer_commands();
        let on_console = self
            .navigator
            .current(&self.connections)
            .is_some_and(|row| row.kind == RowKind::Console);
        groups
            .iter()
            .map(|(title, rows)| {
                let shown = rows.iter().filter_map(|(actions, label)| {
                    let action = actions.first().filter(|item| acting.contains(item))?;
                    // On a console row, these two keys act on the console.
                    let label = match action {
                        NavigatorRefresh if on_console => text::MENU_RENAME_CONSOLE,
                        NavigatorDelete if on_console => text::MENU_REMOVE_CONSOLE,
                        _ => *label,
                    };
                    Some((self.shown_key(*action)?, label))
                });
                KeyGroup {
                    title,
                    keys: shown.collect(),
                }
            })
            .collect()
    }

    /// Returns true when `Ctrl+W k` or `Ctrl+W j` moves the focus from the panel of the
    /// cursor to another panel of the tab on screen.
    fn focus_moves(&self, up: bool) -> bool {
        if self.navigator_focused || self.diagram_active.is_some() || self.compare_active.is_some()
        {
            return false;
        }
        let console = self.console();
        match self.table() {
            Some(tab) if up => tab.filters && tab.focus == TableFocus::Grid,
            Some(tab) => tab.focus != TableFocus::Grid,
            None if up => console.focus == Focus::Results,
            None => console.focus == Focus::Editor && !console.results.is_empty(),
        }
    }

    /// Returns true when a command of the `Ctrl+W` menu has an effect now and belongs to
    /// the panel of the cursor. The key of a command outside the menu keeps its action.
    pub(super) fn fits_cursor(&self, action: Action) -> bool {
        use Action::*;
        let console = self.console();
        let in_console = !self.navigator_focused && self.console_on_screen();
        let in_editor = in_console && console.focus == Focus::Editor;
        let available = self.command_available(action);
        match action {
            // Only the palette leaves itself out of its list.
            OpenPalette => true,
            Run | RunScript => {
                in_console && console.focus != Focus::Results && available && console.can_run()
            }
            Explain | ExplainAnalyze | FormatSql | ToggleComment => {
                in_editor && available && !console.editor.is_blank()
            }
            OpenHistory => in_editor && available,
            RenameConsole | ToggleMode | ChooseSchema => in_console && available,
            ChangeConnection => {
                in_console
                    && available
                    && !console.is_busy()
                    && !console.in_transaction()
                    && !self.connections.is_empty()
            }
            // A failed transaction can only end with a rollback.
            Commit => available && console.transaction != TransactionState::Failed,
            NextTab | PreviousTab => available && self.tab_order().len() > 1,
            FocusUp => self.focus_moves(true),
            FocusDown => self.focus_moves(false),
            _ => available,
        }
    }

    /// Returns the groups of the `Ctrl+W` menu, with the commands of the panel of the
    /// cursor. A removed key leaves its group, and a row keeps only the keys that act.
    pub fn prefix_menu(&self) -> Vec<KeyGroup> {
        use Action::*;
        let groups: [MenuGroup; 6] = [
            (
                text::GROUP_PANELS,
                &[
                    (
                        &[FocusLeft, FocusDown, FocusUp, FocusRight],
                        text::MENU_FOCUS,
                    ),
                    (&[ToggleNavigator], text::MENU_NAVIGATOR),
                    (&[FocusParameters], text::PARAMETERS_TITLE),
                    (&[ToggleParameters], text::PARAMETERS_TOGGLE),
                    (&[ExplorerCommands], text::MENU_EXPLORER),
                    (&[ContextCommands], text::MENU_CONTEXT),
                ],
            ),
            (
                text::GROUP_TABS,
                &[
                    (&[NextTab], text::MENU_NEXT_TAB),
                    (&[PreviousTab], text::MENU_PREVIOUS_TAB),
                    (&[RenameConsole], text::MENU_RENAME),
                    (&[CloseTab], text::MENU_CLOSE_TAB),
                ],
            ),
            (
                text::GROUP_RUN,
                &[
                    (&[Run], text::MENU_RUN),
                    (&[RunScript], text::MENU_SCRIPT),
                    (&[Explain], text::MENU_EXPLAIN),
                    (&[ExplainAnalyze], text::MENU_EXPLAIN_ANALYZE),
                    (&[Stop], text::MENU_STOP),
                    (&[FormatSql], text::MENU_FORMAT),
                    (&[ToggleComment], text::MENU_COMMENT),
                    (&[OpenHistory], text::MENU_HISTORY),
                ],
            ),
            (
                text::GROUP_TRANSACTION,
                &[
                    (&[ToggleMode], text::MENU_MODE),
                    (&[Commit], text::MENU_COMMIT),
                    (&[Rollback], text::MENU_ROLLBACK),
                ],
            ),
            (
                text::GROUP_CONNECTION,
                &[
                    (&[ChooseSchema], text::MENU_SCHEMA),
                    (&[ChangeConnection], text::MENU_CHANGE),
                    (&[OpenSessions], text::MENU_SESSIONS),
                    (&[OpenDiagram], text::MENU_DIAGRAM),
                    (&[OpenCompare], text::MENU_COMPARE),
                    (&[FollowKey], text::MENU_FOLLOW),
                ],
            ),
            (
                text::GROUP_APPLICATION,
                &[
                    (&[OpenPalette], text::MENU_COMMANDS),
                    (&[Help], text::MENU_KEYS),
                    (&[OpenSettings], text::MENU_SETTINGS),
                    (&[ReloadConfig], text::MENU_RELOAD),
                    (&[Quit], text::MENU_QUIT),
                ],
            ),
        ];
        groups
            .iter()
            .map(|(title, rows)| {
                let shown = rows.iter().filter_map(|(actions, label)| {
                    let mut fitting = actions.to_vec();
                    fitting.retain(|action| self.fits_cursor(*action));
                    let keys = self.prefix_keys(&fitting);
                    (!keys.is_empty()).then_some((keys, *label))
                });
                KeyGroup {
                    title,
                    keys: shown.collect(),
                }
            })
            .collect()
    }

    fn has_transactions(&self, connection: ConnectionId) -> bool {
        self.connection(connection)
            .is_none_or(|item| item.capabilities.transactions)
    }

    /// Returns true when an action has an effect in the current state.
    pub(super) fn command_available(&self, action: Action) -> bool {
        if matches!(
            action,
            Action::EditorAiComplete
                | Action::EditorAiCompleteRequest
                | Action::EditorAiAccept
                | Action::EditorAiReject
                | Action::EditorAiCancel
        ) {
            return self.ai_action_available(action);
        }
        if action == Action::EditorSuggestions && self.ai_eligible() {
            return true;
        }
        if action.region() == Region::Review {
            return self.review_command_available(action);
        }
        if self.review_blocks(action) {
            return false;
        }
        if self.home() && !home_allows(action) {
            return false;
        }
        match action.region() {
            Region::Navigator => return self.navigator_command_available(action),
            Region::Grid => return self.grid_command_available(action),
            Region::Editor => {
                return self.editor_has_focus() && self.console().editor.command_available(action);
            }
            Region::Diagram => return self.diagram_command_available(action),
            _ => {}
        }
        let console = self.console();
        let in_table = !self.console_on_screen();
        let idle = !console.is_busy();
        let connection = self.active_connection();
        let transactions = connection.is_none_or(|item| item.capabilities.transactions);
        let namespace =
            connection.is_some_and(|item| item.capabilities.namespace != Namespace::None);
        match action {
            Action::AgentSessions => self.agent.has_sessions(),
            Action::AgentReview => self.agent.has_proposals(),
            Action::AgentDirect => !in_table && !self.home(),
            Action::AgentStop => self.agent.active_tasks() != 0,
            Action::AgentSqlToConsole | Action::AgentSqlClear => {
                !in_table && self.agent.owns(console.id) && !console.editor.is_blank()
            }
            Action::AgentSession => !in_table && self.agent.tab_session(console.id).is_some(),
            Action::AgentTasks => !in_table && self.agent.tab_tasks(console.id) != 0,
            Action::OpenPalette => false,
            Action::ContextCommands => self.context_region().is_some(),
            Action::Run | Action::RunScript => {
                !in_table && idle && !self.association_pending(console.id)
            }
            Action::Explain | Action::ExplainAnalyze => {
                let analyze = action == Action::ExplainAnalyze;
                let plans =
                    connection.is_some_and(|item| Explain::new(item.dialect, analyze).is_some());
                !in_table && idle && plans
            }
            Action::Stop => match (self.compare(), self.diagram(), self.table()) {
                (Some(tab), _, _) => tab.is_busy(),
                (None, Some(tab), _) => tab.is_busy(),
                (None, None, Some(tab)) => tab.is_busy(),
                (None, None, None) => !idle,
            },
            Action::ToggleMode => !in_table && idle && transactions && !self.agent.owns(console.id),
            Action::Commit => {
                !in_table
                    && idle
                    && console.in_transaction()
                    && console.transaction != TransactionState::Failed
            }
            Action::Rollback => !in_table && idle && console.in_transaction(),
            Action::ChooseSchema => !in_table && idle && namespace && !console.in_transaction(),
            Action::OpenDiagram => self.can_open_diagram_here(),
            Action::OpenCompare => self.can_open_compare_here(),
            Action::FollowKey => self.can_follow_key(),
            Action::OpenSessions => self.sessions_connection().is_some_and(|id| {
                let dialect = self.connection(id).map(|item| item.dialect);
                dialect.is_some_and(has_sessions)
            }),
            Action::RenameConsole | Action::ChangeConnection => !in_table,
            Action::FocusParameters | Action::ToggleParameters => {
                self.console_on_screen() && !console.parameters.fields.is_empty()
            }
            Action::FormatSql => !in_table && console.editor.dialect() != Dialect::Redis,
            Action::ToggleComment => !in_table,
            Action::OpenHistory => !in_table && self.history_is_open_to(connection),
            Action::FocusLeft => !self.navigator.collapsed && !self.navigator_focused,
            Action::FocusRight => self.navigator_focused,
            _ => true,
        }
    }

    fn open_palette(&mut self) {
        self.console_mut().editor.close_suggestions();
        let commands = self
            .palette_actions()
            .into_iter()
            .map(|action| {
                let label = self.command_label(action);
                let label = match self.command_target(action) {
                    Some(target) => format!("{label} — {target}"),
                    None => label.to_owned(),
                };
                let key_works = action.region() == Region::Global
                    || Some(action.region()) == self.context_region();
                Command {
                    action,
                    label,
                    key: if key_works {
                        self.shown_key(action)
                    } else {
                        None
                    },
                }
            })
            .collect();
        self.palette = Some(Palette::new(commands));
    }

    fn on_palette_key(&mut self, key: KeyEvent) {
        let Some(palette) = &mut self.palette else {
            return;
        };
        let request = palette.on_key(key);
        self.on_palette_request(request);
    }

    pub(super) fn on_palette_request(&mut self, request: PaletteRequest) {
        match request {
            PaletteRequest::None => {}
            PaletteRequest::Cancel => self.palette = None,
            PaletteRequest::Run(action) => {
                self.palette = None;
                if self.command_available(action) {
                    self.perform(action, false);
                }
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
            rows.extend(self.explorer_row(Action::NavigatorAdd, text::WELCOME_ADD));
            text::WELCOME_NO_CONNECTION
        } else {
            rows.extend(keyed(Action::Run, text::WELCOME_RUN));
            rows.extend(keyed(Action::RunScript, text::WELCOME_SCRIPT));
            rows.push((
                text::WELCOME_SUGGESTIONS_KEY.to_owned(),
                text::WELCOME_SUGGESTIONS,
            ));
            text::WELCOME_WRITE
        };
        rows.extend(keyed(Action::OpenPalette, text::WELCOME_COMMANDS));
        rows.extend(keyed(Action::Help, text::WELCOME_KEYS));
        Some(Welcome {
            logo: false,
            note,
            rows,
        })
    }

    /// Returns the screen of the application without a tab.
    pub(super) fn home_screen(&self) -> Welcome {
        let keyed = |action, label| self.shown_key(action).map(|key| (key, label));
        let mut rows = Vec::new();
        if self.navigator.collapsed {
            rows.extend(keyed(Action::ToggleNavigator, text::HOME_EXPLORER));
        }
        rows.extend(self.explorer_row(Action::NavigatorAdd, text::WELCOME_ADD));
        let note = if self.connections.is_empty() {
            text::WELCOME_NO_CONNECTION
        } else {
            rows.extend(self.explorer_row(Action::NavigatorNewConsole, text::HOME_CONSOLE));
            text::HOME_NO_CONSOLE
        };
        rows.extend(self.explorer_row(Action::NavigatorScripts, text::HOME_SCRIPTS));
        rows.extend(keyed(Action::OpenSettings, text::HOME_SETTINGS));
        rows.extend(keyed(Action::OpenPalette, text::WELCOME_COMMANDS));
        rows.extend(keyed(Action::Help, text::WELCOME_KEYS));
        rows.extend(keyed(Action::Quit, text::HOME_QUIT));
        Welcome {
            logo: true,
            note,
            rows,
        }
    }

    /// Returns the key that moves to the explorer, then the key of an explorer command,
    /// with its label. That path acts from any panel.
    fn explorer_row(&self, action: Action, label: &'static str) -> Option<(String, &'static str)> {
        let focus = self.shown_key(Action::FocusLeft)?;
        let key = self.shown_key(action)?;
        Some((format!("{focus} {key}"), label))
    }
}

/// Returns true for an action that acts on the home screen: a command of the explorer,
/// or a command that needs no tab.
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
                | FocusLeft
                | ExplorerCommands
                | ContextCommands
                | OpenSessions
                | OpenDiagram
                | OpenCompare
        )
}
