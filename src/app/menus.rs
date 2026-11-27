//! The command menus, the palette, and the home screen.

use crossterm::event::KeyEvent;

use super::{App, Screen};
use crate::connector::{Namespace, TransactionState};
use crate::console::Focus;
use crate::exec::ConnectionId;
use crate::grid::printable;
use crate::input::{Action, Region};
use crate::navigator::RowKind;
use crate::palette::{Command, Palette, PaletteRequest, Target};
use crate::plan::Explain;
use crate::structure::{has_locks, has_sessions};
use crate::table::TableFocus;
use crate::ui::kit::measure::fit;
use crate::ui::text;
use crate::ui::welcome::Welcome;
use crate::ui::which_key::KeyGroup;

/// The actions of one row of the `Ctrl+W` menu, with its label.
type MenuRow = (&'static [Action], &'static str);
/// The title of one group of the `Ctrl+W` menu, with its rows.
type MenuGroup = (&'static str, &'static [MenuRow]);

/// The longest name of a row in the title of the explorer page.
const EXPLORER_TITLE_NAME: usize = 48;

impl App {
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
    pub(super) fn prefix_keys(&self, actions: &[Action]) -> String {
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
                    (&[NavigatorLocks], text::MENU_LOCKS),
                    (&[NavigatorMoveUp], text::MENU_MOVE_UP),
                    (&[NavigatorMoveDown], text::MENU_MOVE_DOWN),
                    (&[NavigatorMoveToFolder], text::MENU_MOVE_TO_FOLDER),
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
        let kind = self
            .navigator
            .current(&self.connections)
            .map(|row| row.kind);
        let on_console = kind == Some(RowKind::Console);
        let on_folder = kind == Some(RowKind::Folder);
        groups
            .iter()
            .map(|(title, rows)| {
                let shown = rows.iter().filter_map(|(actions, label)| {
                    let action = actions.first().filter(|item| acting.contains(item))?;
                    // On a console row, these two keys act on the console.
                    let label = match action {
                        NavigatorRefresh if on_console => text::MENU_RENAME_CONSOLE,
                        NavigatorDelete if on_console => text::MENU_REMOVE_CONSOLE,
                        NavigatorEdit if on_folder => text::MENU_RENAME_FOLDER,
                        NavigatorDelete if on_folder => text::MENU_REMOVE_FOLDER,
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
    pub(super) fn focus_moves(&self, up: bool) -> bool {
        if self.navigator_focused
            || self.diagram_index().is_some()
            || self.compare_index().is_some()
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
            SqlDiagnostics | RunSubquery => in_editor && available,
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
                    (&[RunSubquery], text::MENU_SUBQUERY),
                    (&[Explain], text::MENU_EXPLAIN),
                    (&[ExplainAnalyze], text::MENU_EXPLAIN_ANALYZE),
                    (&[AiExplainError], text::MENU_AI_EXPLAIN_ERROR),
                    (&[AiExplainPlan], text::MENU_AI_EXPLAIN_PLAN),
                    (&[Stop], text::MENU_STOP),
                    (&[FormatSql], text::MENU_FORMAT),
                    (&[SqlDiagnostics], text::SYNTAX_TITLE),
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
                    (&[OpenLocks], text::MENU_LOCKS),
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
                    let label = if actions.first() == Some(&Run) {
                        self.command_label(Run)
                    } else {
                        *label
                    };
                    (!keys.is_empty()).then_some((keys, label))
                });
                KeyGroup {
                    title,
                    keys: shown.collect(),
                }
            })
            .collect()
    }

    pub(super) fn has_transactions(&self, connection: ConnectionId) -> bool {
        self.connection(connection)
            .is_none_or(|item| item.capabilities.transactions)
    }

    /// Returns true when an action has an effect in the current state.
    pub(super) fn command_available(&self, action: Action) -> bool {
        if matches!(
            action,
            Action::EditorAiComplete
                | Action::EditorAiCompleteRequest
                | Action::AiExplainError
                | Action::AiExplainPlan
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
        if self.home() && !self.home_allows(action) {
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
            Action::RunSubquery => {
                self.editor_has_focus()
                    && idle
                    && !self.association_pending(console.id)
                    && !console.editor.dialect().is_command_line()
            }
            Action::Explain | Action::ExplainAnalyze => {
                let analyze = action == Action::ExplainAnalyze;
                let plans =
                    connection.is_some_and(|item| Explain::new(item.dialect, analyze).is_some());
                !in_table && idle && plans
            }
            Action::Stop => match self.tabs.screen {
                Screen::Compare(index) => self.tabs.compares[index].is_busy(),
                Screen::Diagram(index) => self.tabs.diagrams[index].is_busy(),
                Screen::Table(index) => self.tabs.tables[index].is_busy(),
                Screen::Console => !idle || self.ai_explanation_running(console.id),
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
            Action::OpenLocks => self.sessions_connection().is_some_and(|id| {
                let dialect = self.connection(id).map(|item| item.dialect);
                dialect.is_some_and(has_locks)
            }),
            Action::RenameConsole | Action::ChangeConnection => !in_table,
            Action::OpenSqlFile => !self.connections.is_empty(),
            Action::FocusParameters | Action::ToggleParameters => {
                self.console_on_screen() && !console.parameters.fields.is_empty()
            }
            Action::SqlDiagnostics => {
                self.console_on_screen()
                    && console.editor.syntax_report().is_some_and(|report| {
                        !report.diagnostics.is_empty() || report.note.is_some()
                    })
            }
            Action::FormatSql => !in_table && !console.editor.dialect().is_command_line(),
            Action::ToggleComment => !in_table,
            Action::OpenHistory => !in_table && self.history_is_open_to(connection),
            Action::FocusLeft | Action::FocusRight if action == self.explorer_entry() => {
                !self.navigator.collapsed && !self.navigator_focused
            }
            Action::FocusLeft | Action::FocusRight => self.navigator_focused,
            _ => true,
        }
    }

    pub(super) fn open_palette(&mut self) {
        self.console_mut().editor.close_suggestions();
        let mut commands: Vec<Command> = self
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
                let key = if key_works {
                    self.shown_key(action)
                } else {
                    None
                };
                Command::action(action, label, key)
            })
            .collect();
        // Each help page is a row after the actions. The filter reads its full text.
        if self.command_available(Action::Help) {
            let help = self.help();
            commands.extend(
                help.page_titles()
                    .into_iter()
                    .enumerate()
                    .map(|(page, title)| Command {
                        target: Target::Help(page),
                        label: text::help_command(title),
                        key: None,
                        words: help.page_text(page),
                    }),
            );
        }
        self.modals.palette = Some(Palette::new(commands));
    }

    pub(super) fn on_palette_key(&mut self, key: KeyEvent) {
        let Some(palette) = &mut self.modals.palette else {
            return;
        };
        let request = palette.on_key(key);
        self.on_palette_request(request);
    }

    pub(super) fn on_palette_request(&mut self, request: PaletteRequest) {
        match request {
            PaletteRequest::None => {}
            PaletteRequest::Cancel => self.modals.palette = None,
            PaletteRequest::Run(action) => {
                self.modals.palette = None;
                if self.command_available(action) {
                    self.perform(action, false);
                }
            }
            PaletteRequest::Help(page) => {
                self.modals.palette = None;
                let mut help = self.help();
                help.open_page(page);
                self.modals.help = Some(help);
            }
        }
    }

    /// Returns the first screen of a console without text.
    pub(super) fn welcome(&self) -> Option<Welcome> {
        if !self.console().editor.is_blank() {
            return None;
        }
        let keyed = |action, label| self.welcome_command(action, label);
        let mut rows = Vec::new();
        let note = if self.connections.is_empty() {
            rows.extend(self.explorer_row(Action::NavigatorAdd, text::WELCOME_ADD));
            text::WELCOME_NO_CONNECTION
        } else {
            rows.extend(keyed(Action::Run, text::WELCOME_RUN));
            rows.extend(keyed(Action::RunScript, text::WELCOME_SCRIPT));
            rows.extend(keyed(Action::EditorSuggestions, text::WELCOME_SUGGESTIONS));
            rows.extend(keyed(Action::OpenSqlFile, text::HOME_OPEN_SQL));
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
        let keyed = |action, label| self.welcome_command(action, label);
        let mut rows = Vec::new();
        if self.navigator.collapsed {
            rows.extend(keyed(Action::ToggleNavigator, text::HOME_EXPLORER));
        }
        rows.push((String::new(), text::HOME_CONNECT));
        rows.extend(self.explorer_row(Action::NavigatorAdd, text::WELCOME_ADD));
        let note = if self.connections.is_empty() {
            rows.push((String::new(), text::HOME_SQL_NEEDS_CONNECTION));
            text::WELCOME_NO_CONNECTION
        } else {
            if self
                .navigator
                .current(&self.connections)
                .is_some_and(|row| matches!(row.kind, RowKind::Connection | RowKind::Console))
            {
                rows.extend(self.explorer_row(Action::NavigatorOpen, text::HOME_CONSOLE));
            }
            rows.extend(keyed(Action::OpenSqlFile, text::HOME_OPEN_SQL));
            text::HOME_NO_CONSOLE
        };
        rows.push((String::new(), text::HOME_SAVED));
        rows.extend(self.explorer_row(Action::NavigatorNewConsole, text::HOME_SAVED_CONSOLES));
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

    /// Shows a direct shortcut, or the palette followed by the command label.
    pub(super) fn welcome_command(
        &self,
        action: Action,
        label: &'static str,
    ) -> Option<(String, &'static str)> {
        let available = if action.region() == Region::Navigator {
            self.navigator_command_available(action)
        } else {
            self.command_available(action)
        };
        if !available && action != Action::OpenPalette {
            return None;
        }
        match self.shown_key(action) {
            Some(key) => Some((key, label)),
            None => Some((
                format!("{} →", self.shown_key(Action::OpenPalette)?),
                self.command_label(action),
            )),
        }
    }

    /// Returns the path to an explorer command from any panel.
    pub(super) fn explorer_row(
        &self,
        action: Action,
        label: &'static str,
    ) -> Option<(String, &'static str)> {
        let focus = self.shown_key(self.explorer_entry())?;
        let (key, label) = self.welcome_command(action, label)?;
        Some((format!("{focus} {key}"), label))
    }
}
