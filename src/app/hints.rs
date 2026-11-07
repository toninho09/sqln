//! The hint row: the commands that act in the state on screen, the most relevant first.
//!
//! The row keeps the commands that fit, so the order of a list decides what a small
//! screen shows. A command stays out of a list when its key does nothing in that state.

use super::App;
use crate::compare::form as compare_form;
use crate::compare::tab::CompareTab;
use crate::console::Focus;
use crate::diagram::tab::DiagramTab;
use crate::export::form as export_form;
use crate::form::{Field, FieldStyle};
use crate::import::form as import_form;
use crate::input::{Action, Chord, Lookup, Region};
use crate::navigator::{Navigator, RowKind};
use crate::settings;
use crate::table::{TableFocus, TableTab};
use crate::ui::kit::feedback::{HintItem, fit_hints, hint_text};
use crate::ui::text;
use unicode_width::UnicodeWidthStr;

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
        if self.command_page.is_some() {
            return HintItem::of(text::HINT_PREFIX);
        }
        if self.pending_prefix() {
            return HintItem::of(text::HINT_PREFIX);
        }
        if self.pending_key.is_some() {
            return HintItem::of(text::HINT_SEQUENCE);
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
            return self.with_commands(self.navigator_hint());
        }
        // Without the focus in the explorer, the row names the way to it.
        if self.home() {
            let explorer = if self.navigator.collapsed {
                Action::ToggleNavigator
            } else {
                Action::FocusLeft
            };
            return self.hint_parts(&[
                (explorer, "explorer"),
                (Action::OpenPalette, "commands"),
                (Action::Help, "keys"),
                (Action::Quit, "quit"),
            ]);
        }
        let items = if let Some(tab) = self.diagram() {
            self.diagram_hint(tab)
        } else if let Some(tab) = self.compare() {
            self.compare_hint(tab)
        } else if let Some(tab) = self.table() {
            self.table_hint(tab)
        } else {
            self.console_hint()
        };
        let local_input = self.table().is_some_and(|tab| {
            tab.list.is_some() || tab.result.search.is_some() || tab.focus != TableFocus::Grid
        }) || self
            .compare()
            .is_some_and(|tab| tab.result.search.is_some())
            || (self.console_on_screen()
                && match self.console().focus {
                    Focus::Parameters => true,
                    Focus::Results => self.console().search_is_open(),
                    Focus::Editor => {
                        self.console().editor.search().is_some()
                            || self.console().editor.suggestions().is_some()
                    }
                });
        if local_input {
            items
        } else {
            self.with_commands(items)
        }
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
            if form.shortcuts.is_recording() {
                return Some(if form.shortcuts.mouse_confirm {
                    text::HINT_RECORD_MOUSE
                } else {
                    text::HINT_RECORD_KEYS
                });
            }
            if form.shortcuts.is_editing()
                && settings::SECTIONS[form.section]
                    .fields
                    .contains(&settings::Field::ShortcutCommands)
            {
                return Some(text::HINT_SHORTCUT_EDIT);
            }
            return Some(match form.focus {
                settings::Field::ShortcutCommands => text::HINT_SHORTCUT_COMMANDS,
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
        if let Some(form) = &self.compare_form {
            return Some(match form.focus {
                _ if form.list_shown() => text::HINT_FORM_LIST,
                compare_form::Field::Compare | compare_form::Field::Cancel => {
                    text::HINT_FORM_BUTTON
                }
                compare_form::Field::SourceTable | compare_form::Field::TargetTable => {
                    text::HINT_FORM_TEXT
                }
                _ => text::HINT_FORM,
            });
        }
        if let Some(form) = &self.import_form {
            return Some(match form.focus {
                import_form::Field::Sections => text::HINT_IMPORT_SECTIONS,
                field if field.is_path() => text::HINT_FORM_PATH,
                import_form::Field::Columns => text::HINT_IMPORT_COLUMNS,
                import_form::Field::Preview => text::HINT_IMPORT_PREVIEW,
                import_form::Field::Import | import_form::Field::Cancel => text::HINT_FORM_BUTTON,
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
        if self.help.is_some() {
            return Some(text::HINT_HELP);
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
            _ if form.schema_list.is_some() => text::HINT_MARK_LIST,
            _ if form.focus.is_path() => text::HINT_FORM_PATH,
            _ if form.focus == Field::Schemas => text::HINT_FORM_SCHEMAS,
            FieldStyle::Text | FieldStyle::Secret => text::HINT_FORM_TEXT,
            FieldStyle::Button => text::HINT_FORM_BUTTON,
            FieldStyle::Choice | FieldStyle::Flag => text::HINT_FORM,
        })
    }

    fn with_commands(&self, mut items: Vec<HintItem>) -> Vec<HintItem> {
        let extras = [
            (Action::ContextCommands, "panel"),
            (Action::NextTab, "next tab"),
            (Action::PreviousTab, "previous tab"),
            (Action::CloseTab, "close tab"),
            (Action::OpenSettings, "settings"),
            (Action::Help, "keys"),
        ];
        for (action, label) in extras {
            if self.fits_cursor(action) {
                for item in self.hint_parts(&[(action, label)]) {
                    if !items.contains(&item) {
                        items.push(item);
                    }
                }
            }
        }
        if self
            .keymap
            .lookup(Region::Global, Chord::parse("ctrl+w").unwrap())
            == Lookup::Sequence
        {
            items.extend(HintItem::of(text::HINT_PREFIX_MENU));
        }
        items.extend(self.hint_parts(&[(Action::OpenPalette, "commands")]));
        items
    }

    /// Reserves the palette command before fitting the other whole commands.
    pub(super) fn fitted_hints(&self, width: usize) -> Vec<HintItem> {
        let mut items = self.hint_items();
        let palette = self.shown_key(Action::OpenPalette);
        let reserved = items
            .iter()
            .position(|item| item.label == "commands" && Some(&item.key) == palette.as_ref());
        let Some(index) = reserved else {
            return fit_hints(&items, width);
        };
        let item = items.remove(index);
        let cost = hint_text(std::slice::from_ref(&item)).width();
        if cost > width {
            return fit_hints(&items, width);
        }
        let mut kept = fit_hints(&items, width.saturating_sub(cost + 2));
        kept.push(item);
        kept
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
            .filter(|item| item.2 && self.fits_cursor(item.0))
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
        let available = |action| self.command_available(action);
        [
            (Action::Stop, "stop", available(Action::Stop)),
            (Action::Commit, "commit", self.fits_cursor(Action::Commit)),
            (Action::Rollback, "rollback", available(Action::Rollback)),
        ]
    }

    fn console_hint(&self) -> Vec<HintItem> {
        let console = self.console();
        if console.focus == Focus::Parameters {
            let mut items = self.hint_parts(&[
                (Action::Run, "run"),
                (Action::FocusUp, "editor"),
                (Action::OpenPalette, "commands"),
            ]);
            items.extend(HintItem::of(text::HINT_PARAMETERS));
            return items;
        }
        if console.focus == Focus::Results {
            return if console.search_is_open() {
                HintItem::of(text::HINT_GRID_SEARCH)
            } else {
                self.results_hint()
            };
        }
        if console.editor.has_ghosts() && console.editor.review().is_none() {
            let keys = console.editor.ghost_keys();
            return vec![
                HintItem {
                    key: keys.accept.clone(),
                    label: "accept",
                },
                HintItem {
                    key: keys.close.clone(),
                    label: "reject",
                },
            ];
        }
        if let Some(review) = console.editor.review() {
            return self.hint_parts(&[
                (Action::ReviewAccept, "accept"),
                (Action::ReviewReject, "reject"),
                (Action::ReviewNext, "next"),
                (Action::ReviewAcceptAll, "accept all"),
                (Action::ReviewRejectAll, "reject all"),
                (Action::ReviewClose, review.keys.close_label_lower()),
            ]);
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

    fn editor_hint(&self) -> Vec<HintItem> {
        let console = self.console();
        let mut commands = self.ending_commands().to_vec();
        commands.extend([
            (Action::Run, "run", console.can_run()),
            (Action::RunScript, "script", console.can_run()),
            (
                Action::ChangeConnection,
                "connection",
                !console.can_run() && !self.connections.is_empty(),
            ),
            (Action::FocusLeft, "navigator", !console.can_run()),
            (Action::FocusDown, "results", !console.results.is_empty()),
            (
                Action::FocusParameters,
                "parameters",
                !console.parameters.fields.is_empty(),
            ),
            (Action::EditorFind, "find", true),
            (
                Action::EditorSuggestions,
                "suggestions",
                console.editor.is_inserting(),
            ),
            (Action::FormatSql, "format", !console.editor.is_blank()),
            (Action::OpenHistory, "history", true),
            (Action::Explain, "plan", !console.editor.is_blank()),
            (Action::ToggleComment, "comment", !console.editor.is_blank()),
            (Action::EditorSave, "save", true),
            (Action::EditorReplace, "replace", true),
            (
                Action::ExplainAnalyze,
                "analyze",
                !console.editor.is_blank(),
            ),
            (Action::ToggleMode, "mode", true),
            (Action::ChooseSchema, "schema", true),
            (Action::RenameConsole, "rename", true),
        ]);
        self.hints_when(&commands)
    }

    fn results_hint(&self) -> Vec<HintItem> {
        let mut commands = self.ending_commands().to_vec();
        commands.extend([
            (Action::GridClearSelection, "clear selection", true),
            (Action::GridCopy, "copy", true),
            (Action::GridOpenValue, "value", true),
            (Action::GridFind, "find", true),
            (Action::GridOpenRow, "row", true),
            (Action::FocusUp, "editor", true),
            (Action::GridExport, "export", true),
            (Action::GridNextResult, "tab", true),
            (Action::GridPreviousResult, "previous", true),
            (Action::GridSelectAll, "select all", true),
            (Action::GridToggleSelection, "select", true),
            (Action::GridNarrow, "narrow", true),
            (Action::GridWiden, "widen", true),
            (Action::GridAutoWidth, "auto width", true),
            (Action::GridCloseResult, "close result", true),
        ]);
        self.hints_when(&commands)
    }

    fn diagram_hint(&self, tab: &DiagramTab) -> Vec<HintItem> {
        let boxes = !tab.model.entities.is_empty();
        let chosen = tab.selected.is_some();
        let keys = if tab.keys_only {
            "all columns"
        } else {
            "keys only"
        };
        self.hints_when(&[
            (Action::Stop, "stop", tab.is_busy()),
            (Action::DiagramOpen, "data", chosen),
            (Action::DiagramStructure, "struct", chosen),
            (Action::DiagramKeysOnly, keys, boxes),
            (Action::DiagramCopy, "mermaid", boxes),
            (Action::DiagramReload, "reload", !tab.is_busy()),
            (Action::Help, "keys", true),
        ])
    }

    fn compare_hint(&self, tab: &CompareTab) -> Vec<HintItem> {
        if tab.result.search.is_some() {
            return HintItem::of(text::HINT_GRID_SEARCH);
        }
        let rows = tab.done && !tab.result.table.rows.is_empty();
        self.hints_when(&[
            (Action::Stop, "stop", tab.is_busy()),
            (
                Action::CompareScript,
                "script",
                self.command_available(Action::CompareScript),
            ),
            (
                Action::CompareCopy,
                "copy script",
                self.command_available(Action::CompareCopy),
            ),
            (Action::GridClearSelection, "clear selection", true),
            (Action::GridOpenRow, "row", rows),
            (Action::GridFind, "find", rows),
            (Action::GridCopy, "copy", rows),
            (Action::GridOpenValue, "value", rows),
            (Action::GridReload, "reload", !tab.is_busy()),
            (Action::GridExport, "export", true),
            (Action::Help, "keys", true),
        ])
    }

    fn table_hint(&self, tab: &TableTab) -> Vec<HintItem> {
        if tab.list.is_some() {
            return self.list_hint(text::LIST_FILTER_KEYS);
        }
        if tab.focus != TableFocus::Grid {
            let mut items = self.hints_when(&[(Action::Stop, "stop", tab.is_busy())]);
            items.extend(HintItem::of(text::HINT_FILTER));
            return items;
        }
        if tab.result.search.is_some() {
            return HintItem::of(text::HINT_GRID_SEARCH);
        }
        let section = tab.section().is_some();
        self.hints_when(&[
            (Action::Stop, "stop", true),
            (Action::GridClearSelection, "clear selection", true),
            (Action::GridOpenValue, "load", tab.pending),
            (Action::GridSaveEdits, "apply changes", true),
            (Action::GridReviewEdits, "review", true),
            (Action::GridDiscardEdits, "discard", true),
            (Action::GridReload, "reload", tab.shows_sessions()),
            (Action::GridCopy, "copy", true),
            (Action::GridOpenValue, "value", !tab.pending),
            (Action::GridFind, "find", true),
            (Action::GridFocusFilter, "where", true),
            (Action::GridSortColumn, "sort", true),
            (Action::GridOpenRow, "row", true),
            (Action::GridFollowKey, "follow", true),
            (Action::GridLoadMore, "more", true),
            (
                Action::GridNextResult,
                if section { "section" } else { "page" },
                true,
            ),
            (Action::GridPreviousResult, "previous", true),
            (Action::GridExport, "export", true),
            (Action::GridStructure, "struct", true),
            (Action::GridFocusOrder, "order by", true),
            (Action::GridCountRows, "count", tab.total.is_none()),
            (Action::GridImport, "import", true),
            (Action::GridEditCell, "edit", true),
            (Action::GridNewRow, "new row", true),
            (Action::GridDeleteRow, "delete row", true),
            (Action::GridReload, "reload", !tab.shows_sessions()),
            (Action::GridSelectAll, "select all", true),
            (Action::GridToggleSelection, "select", true),
            (Action::GridNarrow, "narrow", true),
            (Action::GridWiden, "widen", true),
            (Action::GridAutoWidth, "auto width", true),
            (Action::GridCancelQuery, "cancel query", true),
            (Action::GridEndSession, "end session", true),
        ])
    }

    /// Lists the useful commands of the current explorer row.
    /// Lists the commands of the moment for the row under the cursor. The explorer has
    /// many commands, so this row keeps the main ones. `Ctrl+W` lists the others, and
    /// `explorer_commands` is the full list.
    fn navigator_hint(&self) -> Vec<HintItem> {
        let row = self.navigator.current(&self.connections);
        let kind = row.as_ref().map(|row| row.kind);
        let console = kind == Some(RowKind::Console);
        let connection = kind == Some(RowKind::Connection);
        let relation = matches!(kind, Some(RowKind::Table | RowKind::View));
        let enter = match kind {
            Some(RowKind::Connection) => "console",
            Some(RowKind::Console) => "open console",
            Some(RowKind::View) => "open view",
            Some(RowKind::Object(_)) => "ddl",
            Some(RowKind::Schema | RowKind::Consoles | RowKind::Group(_)) => "open",
            _ => "open table",
        };
        let mut commands = if self.console_on_screen() {
            self.ending_commands().to_vec()
        } else {
            Vec::new()
        };
        commands.extend([
            (Action::NavigatorClearFilter, "clear filter", true),
            (
                Action::NavigatorOpen,
                enter,
                !matches!(kind, Some(RowKind::Column)),
            ),
            (
                Action::NavigatorNewConsole,
                "new console",
                connection || matches!(kind, Some(RowKind::Consoles | RowKind::Schema)),
            ),
            (Action::NavigatorRefresh, "rename", console),
            (Action::NavigatorStructure, "struct", relation),
            (
                Action::NavigatorExpand,
                if relation { "columns" } else { "expand" },
                true,
            ),
            (
                Action::NavigatorCollapse,
                "collapse",
                row.as_ref().is_some_and(Navigator::is_branch),
            ),
            (
                Action::NavigatorCollapse,
                "parent",
                row.as_ref().is_some_and(|row| !Navigator::is_branch(row)),
            ),
            (Action::NavigatorRefresh, "refresh", !console),
            (Action::NavigatorEdit, "edit", true),
            (
                Action::NavigatorFilter,
                "filter",
                row.is_some() || self.navigator.is_filtered(),
            ),
            (Action::NavigatorStructure, "struct", !relation),
            (Action::NavigatorSessions, "sessions", true),
            (Action::NavigatorCopyName, "copy name", true),
            (Action::NavigatorGenerate, "sql/ddl", true),
            (Action::NavigatorDiagram, "diagram", true),
            (Action::NavigatorCompare, "compare", true),
            (Action::NavigatorImport, "import", true),
            (Action::NavigatorNewConsole, "new console", relation),
            (Action::NavigatorAdd, "add connection", true),
            (Action::NavigatorScripts, "scripts", true),
            (Action::NavigatorDisconnect, "disconnect", true),
            (Action::NavigatorDeletePassword, "delete password", true),
            (
                Action::NavigatorDelete,
                if console {
                    "remove console"
                } else {
                    "remove connection"
                },
                true,
            ),
        ]);
        self.hints_when(&commands)
    }
}
