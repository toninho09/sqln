//! The hint row: the commands that act in the state on screen, the most relevant first.
//!
//! The row keeps the commands that fit, so the order of a list decides what a small
//! screen shows. A command stays out of a list when its key does nothing in that state.

use super::App;
use crate::connector::TransactionState;
use crate::console::Focus;
use crate::export::form as export_form;
use crate::form::FieldStyle;
use crate::input::{Action, Chord};
use crate::navigator::{Navigator, RowKind};
use crate::settings;
use crate::sql::Dialect;
use crate::table::{TableFocus, TableTab};
use crate::ui::statusline::{HintItem, hint_text};
use crate::ui::text;

/// A command, its label, and the fact that makes it act now.
type Candidate = (Action, &'static str, bool);

impl App {
    /// Returns the text of the hint bar, with each command of the state.
    pub fn hint(&self) -> String {
        hint_text(&self.hint_items())
    }

    /// Returns the commands of the hint bar, with the keys of the active key map.
    pub(super) fn hint_items(&self) -> Vec<HintItem> {
        if let Some(parts) = self.window_hint() {
            return HintItem::of(parts);
        }
        if self.pending_key.is_some_and(Chord::is_prefix) {
            return HintItem::of(text::HINT_PREFIX);
        }
        if self
            .persist
            .as_ref()
            .is_some_and(|persist| persist.error.is_some())
        {
            let mut items = HintItem::of(text::HINT_CONFIG_ERROR);
            items.extend(self.hint_parts(&[(Action::ReloadConfig, "loads it again")]));
            return items;
        }
        if self.navigator_focused && self.navigator.filter_is_editing() {
            return HintItem::of(text::HINT_TREE_FILTER);
        }
        if self.navigator_focused {
            return self.navigator_hint();
        }
        if let Some(tab) = self.table() {
            return self.table_hint(tab);
        }
        self.console_hint()
    }

    /// Returns the commands of the window that receives the keys. The order of the
    /// checks is the order of `on_key`, so the row names the keys that act.
    fn window_hint(&self) -> Option<&'static [text::HintPart]> {
        if !self.dialogs.is_empty() {
            return Some(text::HINT_DIALOG);
        }
        if self.auth.prompt.is_some() {
            return Some(text::AUTH_PASSWORD_HINT);
        }
        if self.conflicts.is_some() {
            return Some(text::HINT_CONFLICTS);
        }
        if self.palette.is_some() {
            return Some(text::HINT_PALETTE);
        }
        if self.history.is_some() {
            return Some(text::HINT_HISTORY);
        }
        if self.rename.is_some() {
            return Some(text::HINT_NAME);
        }
        if self.browser.is_some() {
            return Some(text::HINT_BROWSER);
        }
        if let Some(form) = &self.settings_form {
            return Some(match form.focus {
                settings::Field::Sections => text::HINT_SETTINGS_SECTIONS,
                settings::Field::Save | settings::Field::Cancel => text::HINT_FORM_BUTTON,
                field if form.input(field).is_some() => text::HINT_SETTINGS_TEXT,
                _ => text::HINT_FORM,
            });
        }
        if let Some(form) = &self.export_form {
            return Some(match form.focus {
                export_form::Field::Formats => text::HINT_EXPORT_FORMATS,
                export_form::Field::File => text::HINT_FORM_PATH,
                export_form::Field::Export | export_form::Field::Cancel => text::HINT_FORM_BUTTON,
                field if form.input(field).is_some() => text::HINT_FORM_TEXT,
                _ => text::HINT_FORM,
            });
        }
        if self.picker.is_some() {
            let on_script = self.picker_script().is_some();
            return Some(if on_script {
                text::HINT_PICKER
            } else {
                text::HINT_CHOOSE
            });
        }
        if let Some(overlay) = &self.overlay {
            return Some(if overlay.other.is_some() {
                text::HINT_OVERLAY_JSON
            } else if overlay.row.is_some() {
                text::HINT_OVERLAY_ROW
            } else {
                text::HINT_OVERLAY
            });
        }
        self.form.as_ref().map(|form| match form.focus.style() {
            _ if form.focus.is_path() => text::HINT_FORM_PATH,
            FieldStyle::Text | FieldStyle::Secret => text::HINT_FORM_TEXT,
            FieldStyle::Button => text::HINT_FORM_BUTTON,
            FieldStyle::Choice | FieldStyle::Flag => text::HINT_FORM,
        })
    }

    fn hint_parts(&self, items: &[(Action, &'static str)]) -> Vec<HintItem> {
        let shown = items.iter().filter_map(|(action, label)| {
            Some(HintItem {
                key: self.shown_key(*action)?,
                label,
            })
        });
        shown.collect()
    }

    /// Lists the commands that act now, in their order.
    fn hints_when(&self, items: &[Candidate]) -> Vec<HintItem> {
        let acting: Vec<_> = items
            .iter()
            .filter(|item| item.2)
            .map(|item| (item.0, item.1))
            .collect();
        self.hint_parts(&acting)
    }

    fn list_hint(&self, others: (text::HintPart, text::HintPart)) -> Vec<HintItem> {
        HintItem::of(&text::hint_list(self.suggestion_keys, others))
    }

    /// The commands that end a run or a transaction. They lead the row of a console,
    /// because the session stays occupied until the user acts.
    fn ending_commands(&self) -> [Candidate; 3] {
        let healthy = self.console().transaction != TransactionState::Failed;
        let available = |action| self.command_available(action);
        [
            (Action::Stop, "stop", available(Action::Stop)),
            (
                Action::Commit,
                "commit",
                available(Action::Commit) && healthy,
            ),
            (Action::Rollback, "rollback", available(Action::Rollback)),
        ]
    }

    fn console_hint(&self) -> Vec<HintItem> {
        let console = self.console();
        if console.focus == Focus::Results {
            return if console.search_is_open() {
                HintItem::of(text::HINT_GRID_SEARCH)
            } else {
                self.results_hint()
            };
        }
        if console.editor.suggestions().is_some() {
            return self.list_hint(text::LIST_EDITOR_KEYS);
        }
        if console.editor.search().is_some() {
            return HintItem::of(text::HINT_SEARCH);
        }
        if console.is_busy() {
            let mut items = self.hint_parts(&[(Action::Stop, "stop")]);
            items.extend(HintItem::of(text::HINT_EDITOR_STAYS));
            return items;
        }
        self.editor_hint()
    }

    /// A console without a connection and without a file has nothing to run.
    fn editor_hint(&self) -> Vec<HintItem> {
        let console = self.console();
        let can_run = console.connection.is_some() || console.file.is_some();
        let mut commands = self.ending_commands().to_vec();
        commands.extend([
            (Action::Run, "run", can_run),
            (Action::RunScript, "script", can_run),
            (
                Action::ChangeConnection,
                "connection",
                !can_run && !self.connections.is_empty(),
            ),
            (Action::FocusLeft, "navigator", !can_run),
            (Action::FocusDown, "results", !console.results.is_empty()),
            (
                Action::FormatSql,
                "format",
                !console.editor.is_blank() && self.command_available(Action::FormatSql),
            ),
            (
                Action::ToggleComment,
                "comment",
                !console.editor.is_blank() && self.command_available(Action::ToggleComment),
            ),
        ]);
        let mut items = self.hints_when(&commands);
        if console.editor.is_inserting() {
            items.extend(HintItem::of(text::HINT_SUGGESTIONS));
        }
        items.extend(HintItem::of(text::HINT_FIND));
        items.extend(self.hint_parts(&[(Action::Help, "keys")]));
        items
    }

    fn results_hint(&self) -> Vec<HintItem> {
        let console = self.console();
        let rows = console
            .results
            .get(console.active)
            .is_some_and(|result| !result.table.rows.is_empty());
        let mut commands = self.ending_commands().to_vec();
        commands.extend([
            (Action::FocusUp, "editor", true),
            (Action::GridCopy, "copy", rows),
            (
                Action::GridExport,
                "export",
                !console.showing_messages() && self.command_available(Action::GridExport),
            ),
            (Action::GridOpenValue, "value", rows),
            (Action::GridOpenRow, "row", rows),
            (Action::GridFind, "find", rows),
            (Action::GridNextResult, "tab", !console.results.is_empty()),
            (Action::Help, "keys", true),
        ]);
        self.hints_when(&commands)
    }

    fn table_hint(&self, tab: &TableTab) -> Vec<HintItem> {
        if tab.list.is_some() {
            return self.list_hint(text::LIST_FILTER_KEYS);
        }
        let busy = tab.is_busy();
        if tab.focus != TableFocus::Grid {
            let mut items = self.hints_when(&[(Action::Stop, "stop", busy)]);
            items.extend(HintItem::of(text::HINT_FILTER));
            return items;
        }
        if tab.result.search.is_some() {
            return HintItem::of(text::HINT_GRID_SEARCH);
        }
        let rows = tab.loaded() > 0;
        let sections = tab.section().is_some();
        let next = if sections { "section" } else { "page" };
        self.hints_when(&[
            (Action::Stop, "stop", busy),
            (Action::GridOpenValue, "load", tab.pending),
            (Action::GridFocusFilter, "where", tab.filters),
            (Action::GridFocusOrder, "order by", tab.filters),
            (Action::GridSortColumn, "sort", tab.filters && rows && !busy),
            (
                Action::GridLoadMore,
                "more",
                tab.has_more && !busy && !tab.is_blocked(&self.budget),
            ),
            (
                Action::GridNextResult,
                next,
                !busy && (sections || tab.has_pages()),
            ),
            (Action::GridStructure, "struct", tab.filters),
            (
                Action::GridCountRows,
                "count",
                tab.total.is_none() && self.command_available(Action::GridCountRows),
            ),
            (Action::GridCopy, "copy", rows),
            (Action::GridOpenValue, "value", rows),
            (Action::GridOpenRow, "row", rows),
            (Action::GridFind, "find", rows),
            (
                Action::GridExport,
                "export",
                self.command_available(Action::GridExport),
            ),
            (Action::Help, "keys", true),
        ])
    }

    /// Lists the commands of the row under the cursor. The commands of a connection
    /// stay on its row, because their keys never act on a table or on a schema alone.
    fn navigator_hint(&self) -> Vec<HintItem> {
        let filtered = self.navigator.is_filtered();
        let Some(row) = self.navigator.current(&self.connections) else {
            return self.hints_when(&[
                (Action::NavigatorClearFilter, "clear filter", filtered),
                (Action::NavigatorFilter, "edit filter", filtered),
                (Action::NavigatorAdd, "add connection", !filtered),
                (Action::NavigatorScripts, "scripts", !filtered),
                (Action::Help, "keys", true),
            ]);
        };
        let item = &self.connections[row.connection];
        let open = self.navigator.is_open(&row, &self.connections);
        let branch = Navigator::is_branch(&row);
        let connection = row.kind == RowKind::Connection;
        let relation = matches!(row.kind, RowKind::Table | RowKind::View);
        let connected = self
            .manager
            .as_ref()
            .is_some_and(|manager| manager.is_connected(item.id));
        let enter = match row.kind {
            RowKind::Connection => "console",
            RowKind::View => "open view",
            _ => "open table",
        };
        self.hints_when(&[
            (Action::NavigatorClearFilter, "clear filter", filtered),
            (Action::NavigatorOpen, enter, connection || relation),
            (Action::NavigatorNewConsole, "new console", true),
            (Action::NavigatorScripts, "scripts", connection),
            (Action::NavigatorRefresh, "refresh", true),
            (Action::NavigatorEdit, "edit connection", connection),
            (
                Action::NavigatorDisconnect,
                "disconnect",
                connection && connected,
            ),
            (
                Action::NavigatorDeletePassword,
                "delete password",
                connection && self.saved_identity(item).is_some(),
            ),
            (Action::NavigatorAdd, "add connection", connection),
            (Action::NavigatorDelete, "remove connection", connection),
            (
                Action::NavigatorExpand,
                if relation { "columns" } else { "expand" },
                branch && !open && !filtered,
            ),
            (
                Action::NavigatorCollapse,
                if branch { "collapse" } else { "parent" },
                (open || !branch) && !filtered,
            ),
            (
                Action::NavigatorStructure,
                "struct",
                item.dialect != Dialect::Redis,
            ),
            (
                Action::NavigatorGenerate,
                "generate SQL",
                self.can_generate(),
            ),
            (Action::NavigatorCopyName, "copy name", true),
            (Action::NavigatorFilter, "filter", true),
            (Action::Help, "keys", true),
        ])
    }
}
