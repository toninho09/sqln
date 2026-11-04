//! Commands shared by the palette, the panel page, and the hint row.

use super::App;
use crate::console::Focus;
use crate::input::{Action, Region};
use crate::navigator::{Navigator, RowKind};
use crate::table::TableFocus;
use crate::ui::console::fit;
use crate::ui::text;
use crate::ui::which_key::KeyGroup;

impl App {
    pub(super) fn editor_has_focus(&self) -> bool {
        !self.navigator_focused
            && !self.home()
            && self.console_on_screen()
            && self.console().focus == Focus::Editor
    }

    pub(super) fn context_region(&self) -> Option<Region> {
        if self.navigator_focused {
            return Some(Region::Navigator);
        }
        if self.home() {
            return None;
        }
        if self.diagram_active.is_some() {
            return Some(Region::Diagram);
        }
        if self.editor_has_focus() {
            return Some(Region::Editor);
        }
        if self
            .table()
            .is_some_and(|tab| tab.focus != TableFocus::Grid)
        {
            return None;
        }
        if self.console_on_screen() && self.console().focus == Focus::Parameters {
            return None;
        }
        Some(Region::Grid)
    }

    /// The explicit explorer page can act on its row from another panel.
    pub(super) fn navigator_command_available(&self, action: Action) -> bool {
        use Action::*;
        let row = self.navigator.current(&self.connections);
        let item = row.as_ref().map(|row| &self.connections[row.connection]);
        let on_kind = |kind| row.as_ref().is_some_and(|row| row.kind == kind);
        let on_connection = on_kind(RowKind::Connection);
        let on_console = on_kind(RowKind::Console);
        let on_object = row
            .as_ref()
            .is_some_and(|row| matches!(row.kind, RowKind::Group(_) | RowKind::Object(_)));
        let filtered = self.navigator.is_filtered();
        match action {
            NavigatorAdd | NavigatorScripts => true,
            NavigatorOpen => row.as_ref().is_some_and(|row| row.kind != RowKind::Column),
            NavigatorNewConsole | NavigatorRefresh | NavigatorCopyName => row.is_some(),
            NavigatorExpand => {
                !filtered
                    && row.as_ref().is_some_and(|row| {
                        Navigator::is_branch(row) && !self.navigator.is_open(row, &self.connections)
                    })
            }
            NavigatorCollapse => {
                !filtered
                    && row.as_ref().is_some_and(|row| {
                        self.navigator.is_open(row, &self.connections) || !Navigator::is_branch(row)
                    })
            }
            NavigatorEdit => on_connection,
            NavigatorDelete => on_connection || on_console,
            NavigatorDisconnect => {
                on_connection
                    && item.is_some_and(|item| {
                        self.manager
                            .as_ref()
                            .is_some_and(|manager| manager.is_connected(item.id))
                    })
            }
            NavigatorDeletePassword => {
                on_connection && item.is_some_and(|item| self.saved_identity(item).is_some())
            }
            NavigatorSessions => self.can_open_sessions(),
            NavigatorStructure => {
                !on_console
                    && !on_kind(RowKind::Consoles)
                    && !on_object
                    && item.is_some_and(|item| item.dialect != crate::sql::Dialect::Redis)
            }
            NavigatorDiagram => self.can_open_diagram(),
            NavigatorCompare => self.can_compare_row(),
            NavigatorGenerate => self.can_generate(),
            NavigatorImport => self.can_import_row(),
            NavigatorFilter => !self.navigator.collapsed,
            NavigatorClearFilter => self.navigator.filter_input().is_some(),
            _ => row.is_some(),
        }
    }

    pub(super) fn grid_command_available(&self, action: Action) -> bool {
        use Action::*;
        if self.diagram_active.is_some() {
            return false;
        }
        if self.compare_active.is_some() && !self.compare_command_available(action) {
            return false;
        }
        if action.is_compare_only() {
            return self.compare_command_available(action);
        }
        let result = self.active_result();
        let rows = result.is_some_and(|result| !result.table.rows.is_empty());
        let columns = result.is_some_and(|result| !result.table.columns.is_empty());
        let tab = self.table();
        let pending = tab.is_some_and(|tab| !tab.result.edits.is_empty() || tab.edit_uncertain);
        if pending
            && matches!(
                action,
                GridCopy
                    | GridExport
                    | GridImport
                    | GridApply
                    | GridClearFilters
                    | GridSortColumn
                    | GridLoadMore
                    | GridPreviousResult
                    | GridNextResult
                    | GridReload
                    | GridFollowKey
            )
        {
            return false;
        }
        match action {
            GridEditCell | GridSetNull => self.can_edit_cell(),
            GridReviewEdits | GridSaveEdits => {
                pending && tab.is_some_and(|tab| !tab.is_busy() && !tab.edit_uncertain)
            }
            GridDiscardEdits => pending && tab.is_some_and(|tab| !tab.is_busy()),
            GridExport => self.can_export(),
            GridImport => self.can_import(),
            GridOpenValue if tab.is_some_and(|tab| tab.pending) => {
                tab.is_some_and(|tab| !tab.is_busy())
            }
            GridOpenValue | GridOpenRow | GridCopy | GridFind | GridSelectAll
            | GridToggleSelection => rows,
            GridClearSelection => result.is_some_and(|result| result.view.anchor.is_some()),
            GridNarrow | GridWiden | GridAutoWidth => columns,
            GridCloseResult => {
                self.console_on_screen()
                    && !self.console().showing_messages()
                    && !self.has_export(self.console().id)
                    && self.console().can_close_result()
            }
            GridPreviousResult | GridNextResult => match tab {
                Some(tab) => {
                    !tab.is_busy()
                        && (tab.section().is_some()
                            || if action == GridPreviousResult {
                                tab.page() > 0
                            } else {
                                tab.page() + 1 < tab.loaded_pages() || tab.has_more
                            })
                }
                None => self.compare_active.is_none() && !self.console().results.is_empty(),
            },
            GridApply => tab.is_some_and(|tab| tab.filters && !tab.is_busy()),
            GridClearFilters => tab.is_some_and(|tab| {
                tab.filters
                    && !tab.is_busy()
                    && (!tab.filter.text().is_empty()
                        || !tab.order.text().is_empty()
                        || !tab.applied_filter().is_empty()
                        || !tab.applied_order().is_empty())
            }),
            GridFocusFilter | GridFocusOrder => tab.is_some_and(|tab| tab.filters),
            GridSortColumn => rows && tab.is_some_and(|tab| tab.filters && !tab.is_busy()),
            GridLoadMore => tab
                .is_some_and(|tab| tab.has_more && !tab.is_busy() && !tab.is_blocked(&self.budget)),
            GridCountRows => tab.is_some_and(|tab| tab.filters && tab.has_more && !tab.is_busy()),
            GridStructure => tab.is_some_and(|tab| tab.filters),
            GridReload => {
                self.compare().is_some_and(|tab| !tab.is_busy())
                    || tab.is_some_and(|tab| !tab.filters && !tab.is_busy())
            }
            GridCancelQuery | GridEndSession => self.can_signal_session(),
            GridFollowKey => self.can_follow_key(),
            _ => rows,
        }
    }

    /// Lists functional actions in a stable order, with the common actions first.
    pub(super) fn context_commands(&self, region: Region) -> Vec<Action> {
        let mut actions: Vec<_> = Action::in_region(region)
            .filter(|action| action.is_command() && self.command_available(*action))
            .collect();
        actions.sort_by_key(|action| command_priority(*action));
        actions
    }

    pub(super) fn explorer_commands(&self) -> Vec<Action> {
        self.context_commands(Region::Navigator)
    }

    pub(super) fn command_label(&self, action: Action) -> &'static str {
        use Action::*;
        if action == GridOpenValue && self.table().is_some_and(|tab| tab.pending) {
            return text::COMMAND_LOAD_GRID;
        }
        if action == NavigatorOpen
            && let Some(row) = self.navigator.current(&self.connections)
        {
            return match row.kind {
                RowKind::Connection | RowKind::Console => text::COMMAND_OPEN_CONSOLE,
                RowKind::Table | RowKind::View => text::COMMAND_OPEN_DATA,
                RowKind::Object(_) => text::COMMAND_OPEN_DDL,
                _ => text::COMMAND_TOGGLE_ROW,
            };
        }
        if action.region() == Region::Navigator {
            let console = self
                .navigator
                .current(&self.connections)
                .is_some_and(|row| row.kind == RowKind::Console);
            match action {
                NavigatorRefresh if console => return text::MENU_RENAME_CONSOLE,
                NavigatorDelete if console => return text::MENU_REMOVE_CONSOLE,
                _ => {}
            }
        }
        if matches!(action, GridPreviousResult | GridNextResult) {
            return text::grid_action_text(
                action,
                self.table().is_some(),
                self.table().is_some_and(|tab| tab.section().is_some()),
            );
        }
        text::command_text(action)
    }

    pub(super) fn command_target(&self, action: Action) -> Option<String> {
        if action.region() == Region::Navigator
            && !matches!(action, Action::NavigatorAdd | Action::NavigatorScripts)
        {
            return self
                .navigator
                .current(&self.connections)
                .map(|row| fit(&crate::grid::printable(row.name(&self.connections)), 48).0);
        }
        let target = if action.region() == Region::Grid || action == Action::CloseTab {
            if let Some(tab) = self.table() {
                tab.label()
            } else if let Some(tab) = self.diagram() {
                tab.label()
            } else if let Some(tab) = self.compare() {
                tab.label()
            } else if action.region() == Region::Grid {
                text::command_result_target(&self.console().name, self.active_result()?.number)
            } else {
                self.console().name.clone()
            }
        } else if matches!(
            action,
            Action::RenameConsole | Action::ChangeConnection | Action::ChooseSchema
        ) {
            self.console().name.clone()
        } else {
            return None;
        };
        Some(fit(&crate::grid::printable(&target), 48).0)
    }

    pub fn context_menu(&self) -> Vec<KeyGroup> {
        let Some(region) = self.context_region() else {
            return Vec::new();
        };
        if region == Region::Navigator {
            return self.explorer_menu();
        }
        let mut groups: Vec<KeyGroup> = Vec::new();
        for action in self.context_commands(region) {
            let Some(key) = self.shown_key(action) else {
                continue;
            };
            let title = command_group(action);
            let label =
                if action == Action::GridOpenValue && self.table().is_some_and(|tab| tab.pending) {
                    text::MENU_LOAD_GRID
                } else {
                    text::panel_command_text(action)
                };
            if let Some(group) = groups.iter_mut().find(|group| group.title == title) {
                group.keys.push((key, label));
            } else {
                groups.push(KeyGroup {
                    title,
                    keys: vec![(key, label)],
                });
            }
        }
        groups
    }

    pub(super) fn context_menu_title(&self) -> String {
        let panel = match self.context_region() {
            Some(Region::Editor) => text::REGION_EDITOR,
            Some(Region::Diagram) => text::REGION_DIAGRAM,
            _ if self.compare_active.is_some() => text::REGION_COMPARE,
            _ if self.table_active.is_some() => text::REGION_TABLE,
            _ => text::REGION_GRID,
        };
        let target = if let Some(tab) = self.table() {
            Some(tab.label())
        } else if self.editor_has_focus() {
            Some(self.console().name.clone())
        } else {
            self.command_target(Action::GridCopy)
        };
        match target {
            Some(target) => format!(
                " {} · {panel} · {} ",
                text::TITLE_PREFIX,
                fit(&crate::grid::printable(&target), 36).0
            ),
            None => format!(" {} · {panel} ", text::TITLE_PREFIX),
        }
    }

    pub(super) fn palette_actions(&self) -> Vec<Action> {
        use Action::*;
        let region = self.context_region();
        let mut actions: Vec<_> = region
            .map(|region| self.context_commands(region))
            .unwrap_or_default();
        // These commands do not depend on the row of the explorer.
        actions.extend([NavigatorAdd, NavigatorScripts]);
        if !self.navigator_focused {
            actions.extend([GridExport, GridImport, GridApply, GridClearFilters]);
        }
        actions.extend(Action::in_region(Region::Global).filter(|action| {
            *action != OpenPalette
                && self.fits_cursor(*action)
                && !matches!(
                    (region, action),
                    (
                        Some(Region::Navigator),
                        OpenDiagram | OpenCompare | OpenSessions
                    ) | (Some(Region::Grid), FollowKey)
                )
        }));
        let mut kept = Vec::new();
        for action in actions {
            if self.command_available(action) && !kept.contains(&action) {
                kept.push(action);
            }
        }
        kept.sort_by_key(|action| {
            let first = match action {
                Stop | Commit | Rollback => 0,
                action
                    if Some(action.region()) == region
                        || (region == Some(Region::Editor)
                            && matches!(
                                action,
                                Run | RunScript
                                    | Explain
                                    | ExplainAnalyze
                                    | FormatSql
                                    | ToggleComment
                                    | OpenHistory
                            )) =>
                {
                    1
                }
                _ => 2,
            };
            (first, command_priority(*action))
        });
        kept
    }
}

fn command_priority(action: Action) -> u8 {
    use Action::*;
    match action {
        Stop => 0,
        Commit => 1,
        Rollback => 2,
        NavigatorClearFilter | GridClearSelection => 3,
        Run | NavigatorOpen | DiagramOpen | CompareScript => 4,
        RunScript | NavigatorNewConsole | DiagramStructure | CompareCopy => 5,
        GridEditCell => 5,
        GridSaveEdits | GridReviewEdits | GridDiscardEdits => 3,
        GridCopy => 6,
        GridOpenValue => 7,
        GridFind => 8,
        GridFocusFilter => 9,
        GridSortColumn => 10,
        GridOpenRow => 11,
        GridFollowKey => 12,
        GridLoadMore => 13,
        NavigatorStructure => 7,
        NavigatorExpand | NavigatorCollapse => 8,
        NavigatorRefresh => 9,
        NavigatorEdit => 10,
        NavigatorFilter => 11,
        NavigatorSessions => 12,
        NavigatorCopyName => 13,
        NavigatorGenerate => 14,
        EditorFind | EditorSuggestions | DiagramKeysOnly => 6,
        EditorSave => 30,
        EditorReplace | DiagramCopy => 7,
        EditorUndo | DiagramReload => 8,
        EditorRedo => 9,
        NavigatorDelete
        | NavigatorDeletePassword
        | NavigatorDisconnect
        | GridCancelQuery
        | GridEndSession => 100,
        _ => 50,
    }
}

fn command_group(action: Action) -> &'static str {
    use Action::*;
    match action {
        EditorSave | EditorFind | EditorReplace | EditorSuggestions => text::GROUP_CONSOLE,
        EditorUndo | EditorRedo | EditorSelectAll | EditorCopy | EditorCut | EditorPaste
        | GridSelectAll | GridToggleSelection | GridClearSelection | GridNarrow | GridWiden
        | GridAutoWidth => text::GROUP_SELECTION_VIEW,
        GridFocusFilter | GridFocusOrder | GridSortColumn | GridLoadMore | GridCountRows
        | GridPreviousResult | GridNextResult | GridReload | GridCancelQuery | GridEndSession => {
            text::GROUP_TABLE_RESULTS
        }
        _ => text::GROUP_ROW,
    }
}
