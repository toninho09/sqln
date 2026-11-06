//! The key map: the action of each key in each region.
//!
//! A binding is one key or a sequence of keys, such as `ctrl+w e`. The editor keys
//! stay in `editor`, because the EdTUI handler owns the Vim mode.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};

use crate::editor::KeyboardMode;

/// The keys that accept the item of a suggestion list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SuggestionKeys {
    #[default]
    Tab,
    Enter,
    Both,
}

impl SuggestionKeys {
    /// Returns true when the key accepts the item of the list.
    pub fn accepts(self, key: KeyEvent) -> bool {
        let chosen = match key.code {
            KeyCode::Tab => self != Self::Enter,
            KeyCode::Enter => self != Self::Tab,
            _ => false,
        };
        chosen && key.modifiers.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl Chord {
    pub fn from_event(key: KeyEvent) -> Self {
        let mut modifiers =
            key.modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
        // A character holds its case, so Shift adds nothing.
        if matches!(key.code, KeyCode::Char(_)) {
            modifiers -= KeyModifiers::SHIFT;
        }
        let code = if key.code == KeyCode::BackTab {
            modifiers |= KeyModifiers::SHIFT;
            KeyCode::Tab
        } else {
            key.code
        };
        Self { code, modifiers }
    }

    /// Returns the configuration text of a key.
    pub fn text(self) -> Result<String, String> {
        let name = match self.code {
            KeyCode::Char(' ') => "space".to_owned(),
            KeyCode::Char(c) if !c.is_control() && !c.is_whitespace() => c.to_string(),
            KeyCode::F(n) if n > 0 => format!("f{n}"),
            KeyCode::Enter => "enter".to_owned(),
            KeyCode::Esc => "esc".to_owned(),
            KeyCode::Tab => "tab".to_owned(),
            KeyCode::Backspace => "backspace".to_owned(),
            KeyCode::Delete => "delete".to_owned(),
            KeyCode::Insert => "insert".to_owned(),
            KeyCode::Home => "home".to_owned(),
            KeyCode::End => "end".to_owned(),
            KeyCode::PageUp => "pageup".to_owned(),
            KeyCode::PageDown => "pagedown".to_owned(),
            KeyCode::Up => "up".to_owned(),
            KeyCode::Down => "down".to_owned(),
            KeyCode::Left => "left".to_owned(),
            KeyCode::Right => "right".to_owned(),
            _ => return Err("This key cannot be recorded.".to_owned()),
        };
        let mut parts = Vec::new();
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            parts.push("ctrl");
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            parts.push("alt");
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            parts.push("shift");
        }
        parts.push(&name);
        Ok(parts.join("+"))
    }

    /// Returns true for `Ctrl+W`, the key before the commands of the application.
    pub fn is_prefix(self) -> bool {
        self.code == KeyCode::Char('w') && self.modifiers == KeyModifiers::CONTROL
    }

    /// Returns true for a character without Ctrl or Alt: during input, it is text.
    pub fn is_plain_char(self) -> bool {
        matches!(self.code, KeyCode::Char(_)) && (self.modifiers - KeyModifiers::SHIFT).is_empty()
    }

    /// Returns the same key without Shift, when the key has Shift.
    pub fn without_shift(self) -> Option<Self> {
        self.modifiers.contains(KeyModifiers::SHIFT).then(|| Self {
            modifiers: self.modifiers - KeyModifiers::SHIFT,
            ..self
        })
    }

    /// Reads a key such as `ctrl+enter`, `shift+f5`, `alt+up`, or `<`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (modifier_text, name) = match text.rsplit_once('+') {
            Some((start, "")) => (start.strip_suffix('+').unwrap_or(start), "+"),
            Some(parts) => parts,
            None => ("", text),
        };
        let mut modifiers = KeyModifiers::NONE;
        for modifier in modifier_text
            .split('+')
            .filter(|modifier| !modifier.is_empty())
        {
            modifiers |= match modifier.to_ascii_lowercase().as_str() {
                "ctrl" => KeyModifiers::CONTROL,
                "alt" => KeyModifiers::ALT,
                "shift" => KeyModifiers::SHIFT,
                _ => return Err(format!("Unknown key: {text}")),
            };
        }
        let code = key_code(name).ok_or_else(|| format!("Unknown key: {text}"))?;
        Ok(Self::from_event(KeyEvent::new(code, modifiers)))
    }
}

fn key_code(name: &str) -> Option<KeyCode> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Some(KeyCode::Char(c));
    }
    let code = match name.to_ascii_lowercase().as_str() {
        "enter" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "space" => KeyCode::Char(' '),
        other => KeyCode::F(other.strip_prefix('f')?.parse().ok()?),
    };
    Some(code)
}

/// The place where a key has its meaning. The panels come before `Global`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Region {
    Global,
    Editor,
    Navigator,
    Grid,
    Diagram,
    /// An inline review of proposed changes in the editor.
    Review,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    AgentSessions,
    AgentReview,
    AgentDirect,
    AgentStop,
    AgentSqlToConsole,
    AgentSqlClear,
    AgentSession,
    AgentTasks,
    Run,
    RunScript,
    Explain,
    ExplainAnalyze,
    FormatSql,
    ToggleComment,
    OpenHistory,
    OpenSessions,
    OpenDiagram,
    OpenCompare,
    /// Follows a key of the row of the cursor of a table tab.
    FollowKey,
    ExplorerCommands,
    ContextCommands,
    EditorSave,
    EditorFind,
    EditorReplace,
    EditorSuggestions,
    EditorAiComplete,
    EditorAiAccept,
    EditorAiReject,
    EditorAiCancel,
    EditorUndo,
    EditorRedo,
    EditorSelectAll,
    EditorCopy,
    EditorCut,
    EditorPaste,
    Stop,
    ToggleMode,
    Commit,
    Rollback,
    Help,
    OpenPalette,
    CloseTab,
    Quit,
    NextTab,
    PreviousTab,
    ToggleNavigator,
    ChooseSchema,
    ReloadConfig,
    RenameConsole,
    ChangeConnection,
    OpenSettings,
    FocusLeft,
    FocusRight,
    FocusParameters,
    ToggleParameters,
    FocusUp,
    FocusDown,
    NavigatorUp,
    NavigatorDown,
    NavigatorCollapse,
    NavigatorExpand,
    NavigatorOpen,
    NavigatorNewConsole,
    NavigatorRefresh,
    NavigatorDisconnect,
    NavigatorEdit,
    NavigatorDelete,
    NavigatorAdd,
    NavigatorScripts,
    NavigatorDeletePassword,
    NavigatorStructure,
    NavigatorDiagram,
    NavigatorCompare,
    NavigatorSessions,
    NavigatorGenerate,
    NavigatorImport,
    NavigatorCopyName,
    NavigatorFilter,
    NavigatorClearFilter,
    GridUp,
    GridDown,
    GridLeft,
    GridRight,
    GridPageUp,
    GridPageDown,
    GridFirstRow,
    GridLastRow,
    GridFirstColumn,
    GridLastColumn,
    GridSelectAll,
    GridToggleSelection,
    GridClearSelection,
    GridCopy,
    GridOpenValue,
    GridOpenRow,
    GridFind,
    GridNarrow,
    GridWiden,
    GridAutoWidth,
    GridCloseResult,
    GridPreviousResult,
    GridNextResult,
    GridApply,
    GridClearFilters,
    GridFocusFilter,
    GridFocusOrder,
    GridSortColumn,
    GridLoadMore,
    GridCountRows,
    GridExport,
    GridImport,
    GridEditCell,
    GridSetNull,
    GridReviewEdits,
    GridSaveEdits,
    GridDiscardEdits,
    GridStructure,
    GridReload,
    GridCancelQuery,
    GridEndSession,
    GridFollowKey,
    CompareScript,
    CompareCopy,
    DiagramUp,
    DiagramDown,
    DiagramLeft,
    DiagramRight,
    DiagramPanUp,
    DiagramPanDown,
    DiagramPanLeft,
    DiagramPanRight,
    DiagramPageUp,
    DiagramPageDown,
    DiagramOpen,
    DiagramStructure,
    DiagramCopy,
    DiagramReload,
    DiagramKeysOnly,
    ReviewAccept,
    ReviewReject,
    ReviewAcceptAll,
    ReviewRejectAll,
    ReviewNext,
    ReviewPrevious,
    ReviewClose,
}

impl Action {
    fn item(self) -> &'static Item {
        DEFAULTS.iter().find(|item| item.action == self).unwrap()
    }

    /// Returns the name that `config.json` uses.
    pub fn name(self) -> &'static str {
        self.item().name
    }

    pub fn region(self) -> Region {
        self.item().region
    }

    /// Returns the keys of the action without the overrides of the configuration.
    pub fn default_keys(self, mode: KeyboardMode) -> Vec<String> {
        default_keys(self.item(), mode)
    }

    /// Lists the actions of a region in the order of the help screen.
    pub fn in_region(region: Region) -> impl Iterator<Item = Self> {
        DEFAULTS
            .iter()
            .filter(move |item| item.region == region)
            .map(|item| item.action)
    }

    /// Returns true for a command, without basic cursor or view movement.
    pub fn is_command(self) -> bool {
        !self.extends_selection()
            && !matches!(
                self,
                Self::NavigatorUp
                    | Self::NavigatorDown
                    | Self::DiagramUp
                    | Self::DiagramDown
                    | Self::DiagramLeft
                    | Self::DiagramRight
                    | Self::DiagramPanUp
                    | Self::DiagramPanDown
                    | Self::DiagramPanLeft
                    | Self::DiagramPanRight
                    | Self::DiagramPageUp
                    | Self::DiagramPageDown
            )
    }

    /// Returns true for an action that only the grid of a table has.
    pub fn is_table_only(self) -> bool {
        matches!(
            self,
            Action::GridEditCell
                | Action::GridSetNull
                | Action::GridReviewEdits
                | Action::GridSaveEdits
                | Action::GridDiscardEdits
                | Action::GridApply
                | Action::GridClearFilters
                | Action::GridFocusFilter
                | Action::GridFocusOrder
                | Action::GridSortColumn
                | Action::GridLoadMore
                | Action::GridCountRows
                | Action::GridStructure
                | Action::GridReload
                | Action::GridCancelQuery
                | Action::GridEndSession
                | Action::GridFollowKey
        )
    }

    /// Returns true for an action that only the grid of a compare has.
    pub fn is_compare_only(self) -> bool {
        matches!(self, Action::CompareScript | Action::CompareCopy)
    }

    /// Returns true for an action that only the results of a console have.
    pub fn is_console_only(self) -> bool {
        matches!(self, Action::GridCloseResult)
    }

    /// Returns true for a move that Shift changes into a selection.
    pub fn extends_selection(self) -> bool {
        matches!(
            self,
            Action::GridUp
                | Action::GridDown
                | Action::GridLeft
                | Action::GridRight
                | Action::GridPageUp
                | Action::GridPageDown
                | Action::GridFirstRow
                | Action::GridLastRow
                | Action::GridFirstColumn
                | Action::GridLastColumn
        )
    }
}

struct Item {
    action: Action,
    region: Region,
    /// The name of the action in `config.json`.
    name: &'static str,
    keys: &'static [&'static str],
    /// The keys of the Vim mode, in addition to `keys`.
    vim_keys: &'static [&'static str],
    /// True when `keys` applies only to the conventional mode.
    conventional_only: bool,
}

const fn both(
    action: Action,
    region: Region,
    name: &'static str,
    keys: &'static [&'static str],
    vim_keys: &'static [&'static str],
) -> Item {
    Item {
        action,
        region,
        name,
        keys,
        vim_keys,
        conventional_only: false,
    }
}

const DEFAULTS: &[Item] = &[
    both(
        Action::Run,
        Region::Global,
        "run",
        &["ctrl+enter", "f5", "ctrl+w e"],
        &[],
    ),
    both(
        Action::RunScript,
        Region::Global,
        "runScript",
        &["shift+enter", "shift+f5", "ctrl+w s"],
        &[],
    ),
    both(
        Action::FormatSql,
        Region::Global,
        "formatSql",
        &["ctrl+w f"],
        &[],
    ),
    // A terminal without enhanced keys sends `Ctrl+/` as the byte of `Ctrl+7`.
    both(
        Action::ToggleComment,
        Region::Global,
        "toggleComment",
        &["ctrl+w /", "ctrl+/", "ctrl+7"],
        &[],
    ),
    both(
        Action::OpenHistory,
        Region::Global,
        "openHistory",
        &["ctrl+w H"],
        &[],
    ),
    both(
        Action::OpenSessions,
        Region::Global,
        "openSessions",
        &["ctrl+w S"],
        &[],
    ),
    both(
        Action::OpenDiagram,
        Region::Global,
        "openDiagram",
        &["ctrl+w D"],
        &[],
    ),
    both(
        Action::OpenCompare,
        Region::Global,
        "openCompare",
        &["ctrl+w v"],
        &[],
    ),
    both(
        Action::FollowKey,
        Region::Global,
        "followKey",
        &["ctrl+w F"],
        &[],
    ),
    both(
        Action::ExplorerCommands,
        Region::Global,
        "explorerCommands",
        &["ctrl+w tab"],
        &[],
    ),
    both(
        Action::ContextCommands,
        Region::Global,
        "contextCommands",
        &["ctrl+w space"],
        &[],
    ),
    both(
        Action::EditorSave,
        Region::Editor,
        "editorSave",
        &["ctrl+s"],
        &[],
    ),
    both(
        Action::EditorFind,
        Region::Editor,
        "editorFind",
        &["ctrl+f"],
        &["/"],
    ),
    both(
        Action::EditorReplace,
        Region::Editor,
        "editorReplace",
        &[],
        &[],
    ),
    both(
        Action::EditorAiComplete,
        Region::Editor,
        "editorAiComplete",
        &[],
        &[],
    ),
    both(
        Action::EditorAiAccept,
        Region::Editor,
        "editorAiAccept",
        &[],
        &[],
    ),
    both(
        Action::EditorAiReject,
        Region::Editor,
        "editorAiReject",
        &[],
        &[],
    ),
    both(
        Action::EditorAiCancel,
        Region::Editor,
        "editorAiCancel",
        &[],
        &[],
    ),
    both(
        Action::EditorSuggestions,
        Region::Editor,
        "editorSuggestions",
        &["ctrl+space"],
        &[],
    ),
    Item {
        action: Action::EditorUndo,
        region: Region::Editor,
        name: "editorUndo",
        keys: &["ctrl+z"],
        vim_keys: &["u"],
        conventional_only: true,
    },
    Item {
        action: Action::EditorRedo,
        region: Region::Editor,
        name: "editorRedo",
        keys: &["ctrl+y"],
        vim_keys: &["ctrl+r"],
        conventional_only: true,
    },
    Item {
        action: Action::EditorSelectAll,
        region: Region::Editor,
        name: "editorSelectAll",
        keys: &["ctrl+a"],
        vim_keys: &["g g V G"],
        conventional_only: true,
    },
    Item {
        action: Action::EditorCopy,
        region: Region::Editor,
        name: "editorCopy",
        keys: &["ctrl+c"],
        vim_keys: &["y"],
        conventional_only: true,
    },
    Item {
        action: Action::EditorCut,
        region: Region::Editor,
        name: "editorCut",
        keys: &["ctrl+x"],
        vim_keys: &["d"],
        conventional_only: true,
    },
    Item {
        action: Action::EditorPaste,
        region: Region::Editor,
        name: "editorPaste",
        keys: &["ctrl+v"],
        vim_keys: &["p"],
        conventional_only: true,
    },
    both(
        Action::FocusParameters,
        Region::Global,
        "focusParameters",
        &["ctrl+w P"],
        &[],
    ),
    both(
        Action::ToggleParameters,
        Region::Global,
        "toggleParameters",
        &[],
        &[],
    ),
    both(Action::Stop, Region::Global, "stop", &["ctrl+w x"], &[]),
    both(
        Action::Explain,
        Region::Global,
        "explain",
        &["ctrl+w E"],
        &[],
    ),
    both(
        Action::ExplainAnalyze,
        Region::Global,
        "explainAnalyze",
        &["ctrl+w A"],
        &[],
    ),
    both(
        Action::ToggleMode,
        Region::Global,
        "toggleMode",
        &["ctrl+w m"],
        &[],
    ),
    both(Action::Commit, Region::Global, "commit", &["ctrl+w C"], &[]),
    both(
        Action::Rollback,
        Region::Global,
        "rollback",
        &["ctrl+w R"],
        &[],
    ),
    both(
        Action::Help,
        Region::Global,
        "help",
        &["f1", "ctrl+w ?"],
        &[],
    ),
    both(
        Action::OpenPalette,
        Region::Global,
        "openPalette",
        &["ctrl+p", "ctrl+w :"],
        &[],
    ),
    both(
        Action::CloseTab,
        Region::Global,
        "closeTab",
        &["ctrl+w c"],
        &[],
    ),
    both(Action::Quit, Region::Global, "quit", &["ctrl+w q"], &[]),
    both(
        Action::NextTab,
        Region::Global,
        "nextTab",
        &["ctrl+w n"],
        &[],
    ),
    both(
        Action::PreviousTab,
        Region::Global,
        "previousTab",
        &["ctrl+w p"],
        &[],
    ),
    both(
        Action::ToggleNavigator,
        Region::Global,
        "toggleNavigator",
        &["ctrl+w b"],
        &[],
    ),
    both(
        Action::ChooseSchema,
        Region::Global,
        "chooseSchema",
        &["ctrl+w d"],
        &[],
    ),
    both(
        Action::ReloadConfig,
        Region::Global,
        "reloadConfig",
        &["ctrl+w r"],
        &[],
    ),
    both(
        Action::RenameConsole,
        Region::Global,
        "renameConsole",
        &["ctrl+w N"],
        &[],
    ),
    both(
        Action::ChangeConnection,
        Region::Global,
        "changeConnection",
        &["ctrl+w a"],
        &[],
    ),
    both(
        Action::OpenSettings,
        Region::Global,
        "openSettings",
        &["ctrl+w ,"],
        &[],
    ),
    both(
        Action::FocusLeft,
        Region::Global,
        "focusLeft",
        &["ctrl+w h", "ctrl+w left"],
        &[],
    ),
    both(
        Action::FocusRight,
        Region::Global,
        "focusRight",
        &["ctrl+w l", "ctrl+w right"],
        &[],
    ),
    both(
        Action::FocusUp,
        Region::Global,
        "focusUp",
        &["ctrl+w k", "ctrl+w up"],
        &[],
    ),
    both(
        Action::FocusDown,
        Region::Global,
        "focusDown",
        &["ctrl+w j", "ctrl+w down"],
        &[],
    ),
    both(
        Action::NavigatorUp,
        Region::Navigator,
        "navigatorUp",
        &["up"],
        &["k"],
    ),
    both(
        Action::NavigatorDown,
        Region::Navigator,
        "navigatorDown",
        &["down"],
        &["j"],
    ),
    both(
        Action::NavigatorCollapse,
        Region::Navigator,
        "navigatorCollapse",
        &["left"],
        &["h"],
    ),
    both(
        Action::NavigatorExpand,
        Region::Navigator,
        "navigatorExpand",
        &["right"],
        &["l"],
    ),
    both(
        Action::NavigatorOpen,
        Region::Navigator,
        "navigatorOpen",
        &["enter"],
        &[],
    ),
    both(
        Action::NavigatorNewConsole,
        Region::Navigator,
        "newConsole",
        &["n"],
        &[],
    ),
    both(
        Action::NavigatorRefresh,
        Region::Navigator,
        "refreshCatalog",
        &["r"],
        &[],
    ),
    both(
        Action::NavigatorDisconnect,
        Region::Navigator,
        "disconnect",
        &["D"],
        &[],
    ),
    both(
        Action::NavigatorEdit,
        Region::Navigator,
        "editConnection",
        &["e"],
        &[],
    ),
    Item {
        action: Action::NavigatorDelete,
        region: Region::Navigator,
        name: "deleteConnection",
        keys: &["delete"],
        vim_keys: &["x"],
        conventional_only: true,
    },
    both(
        Action::NavigatorAdd,
        Region::Navigator,
        "addConnection",
        &["a"],
        &[],
    ),
    both(
        Action::NavigatorScripts,
        Region::Navigator,
        "scriptsWithoutConnection",
        &["s"],
        &[],
    ),
    both(
        Action::NavigatorDeletePassword,
        Region::Navigator,
        "deletePassword",
        &["p"],
        &[],
    ),
    both(
        Action::NavigatorStructure,
        Region::Navigator,
        "navigatorStructure",
        &["i"],
        &[],
    ),
    both(
        Action::NavigatorDiagram,
        Region::Navigator,
        "navigatorDiagram",
        &["d"],
        &[],
    ),
    both(
        Action::NavigatorCompare,
        Region::Navigator,
        "navigatorCompare",
        &["c"],
        &[],
    ),
    both(
        Action::NavigatorSessions,
        Region::Navigator,
        "navigatorSessions",
        &["S"],
        &[],
    ),
    both(
        Action::NavigatorGenerate,
        Region::Navigator,
        "navigatorGenerate",
        &["g"],
        &[],
    ),
    both(
        Action::NavigatorImport,
        Region::Navigator,
        "navigatorImport",
        &["I"],
        &[],
    ),
    both(
        Action::NavigatorCopyName,
        Region::Navigator,
        "navigatorCopyName",
        &["y", "ctrl+c"],
        &[],
    ),
    both(
        Action::NavigatorFilter,
        Region::Navigator,
        "navigatorFilter",
        &["/", "ctrl+f"],
        &[],
    ),
    both(
        Action::NavigatorClearFilter,
        Region::Navigator,
        "navigatorClearFilter",
        &["esc"],
        &[],
    ),
    both(Action::GridUp, Region::Grid, "gridUp", &["up"], &["k"]),
    both(
        Action::GridDown,
        Region::Grid,
        "gridDown",
        &["down"],
        &["j"],
    ),
    both(
        Action::GridLeft,
        Region::Grid,
        "gridLeft",
        &["left"],
        &["h"],
    ),
    both(
        Action::GridRight,
        Region::Grid,
        "gridRight",
        &["right"],
        &["l"],
    ),
    both(
        Action::GridPageUp,
        Region::Grid,
        "gridPageUp",
        &["pageup"],
        &[],
    ),
    both(
        Action::GridPageDown,
        Region::Grid,
        "gridPageDown",
        &["pagedown"],
        &[],
    ),
    both(
        Action::GridFirstRow,
        Region::Grid,
        "gridFirstRow",
        &["alt+up"],
        &["g g"],
    ),
    both(
        Action::GridLastRow,
        Region::Grid,
        "gridLastRow",
        &["alt+down"],
        &["G"],
    ),
    both(
        Action::GridFirstColumn,
        Region::Grid,
        "gridFirstColumn",
        &["alt+left"],
        &["0"],
    ),
    both(
        Action::GridLastColumn,
        Region::Grid,
        "gridLastColumn",
        &["alt+right"],
        &["$"],
    ),
    both(
        Action::GridSelectAll,
        Region::Grid,
        "gridSelectAll",
        &["ctrl+a"],
        &[],
    ),
    both(
        Action::GridToggleSelection,
        Region::Grid,
        "gridToggleSelection",
        &[],
        &["v"],
    ),
    both(
        Action::GridClearSelection,
        Region::Grid,
        "gridClearSelection",
        &["esc"],
        &[],
    ),
    both(
        Action::GridCopy,
        Region::Grid,
        "gridCopy",
        &["ctrl+c"],
        &["y"],
    ),
    both(
        Action::GridOpenValue,
        Region::Grid,
        "gridOpenValue",
        &["enter"],
        &[],
    ),
    both(
        Action::GridOpenRow,
        Region::Grid,
        "gridOpenRow",
        &["r"],
        &[],
    ),
    both(
        Action::GridFind,
        Region::Grid,
        "gridFind",
        &["ctrl+f"],
        &["/"],
    ),
    both(Action::GridNarrow, Region::Grid, "gridNarrow", &["<"], &[]),
    both(Action::GridWiden, Region::Grid, "gridWiden", &[">"], &[]),
    both(
        Action::GridAutoWidth,
        Region::Grid,
        "gridAutoWidth",
        &["="],
        &[],
    ),
    both(
        Action::GridCloseResult,
        Region::Grid,
        "gridCloseResult",
        &["x", "delete"],
        &[],
    ),
    both(
        Action::GridPreviousResult,
        Region::Grid,
        "gridPreviousResult",
        &["["],
        &[],
    ),
    both(
        Action::GridNextResult,
        Region::Grid,
        "gridNextResult",
        &["]"],
        &[],
    ),
    both(Action::GridApply, Region::Grid, "gridApply", &[], &[]),
    both(
        Action::GridClearFilters,
        Region::Grid,
        "gridClearFilters",
        &[],
        &[],
    ),
    both(
        Action::GridFocusFilter,
        Region::Grid,
        "focusFilter",
        &["f"],
        &[],
    ),
    both(
        Action::GridFocusOrder,
        Region::Grid,
        "focusOrder",
        &["o"],
        &[],
    ),
    both(
        Action::GridSortColumn,
        Region::Grid,
        "sortColumn",
        &["s"],
        &[],
    ),
    both(Action::GridLoadMore, Region::Grid, "loadMore", &["m"], &[]),
    both(
        Action::GridCountRows,
        Region::Grid,
        "gridCountRows",
        &["c"],
        &[],
    ),
    both(Action::GridExport, Region::Grid, "gridExport", &["e"], &[]),
    both(Action::GridImport, Region::Grid, "gridImport", &["I"], &[]),
    both(
        Action::GridEditCell,
        Region::Grid,
        "gridEditCell",
        &["f2"],
        &[],
    ),
    both(
        Action::GridSetNull,
        Region::Grid,
        "gridSetNull",
        &["ctrl+n"],
        &[],
    ),
    both(
        Action::GridReviewEdits,
        Region::Grid,
        "gridReviewEdits",
        &["ctrl+r"],
        &[],
    ),
    both(
        Action::GridSaveEdits,
        Region::Grid,
        "gridSaveEdits",
        &["ctrl+s"],
        &[],
    ),
    both(
        Action::GridDiscardEdits,
        Region::Grid,
        "gridDiscardEdits",
        &["ctrl+d"],
        &[],
    ),
    both(
        Action::GridStructure,
        Region::Grid,
        "gridStructure",
        &["i"],
        &[],
    ),
    both(Action::GridReload, Region::Grid, "gridReload", &["R"], &[]),
    both(
        Action::GridFollowKey,
        Region::Grid,
        "gridFollowKey",
        &["F"],
        &[],
    ),
    both(
        Action::GridCancelQuery,
        Region::Grid,
        "gridCancelQuery",
        &["q"],
        &[],
    ),
    both(
        Action::GridEndSession,
        Region::Grid,
        "gridEndSession",
        &["K"],
        &[],
    ),
    both(
        Action::CompareScript,
        Region::Grid,
        "compareScript",
        &["S"],
        &[],
    ),
    both(
        Action::CompareCopy,
        Region::Grid,
        "compareCopy",
        &["Y"],
        &[],
    ),
    both(
        Action::DiagramUp,
        Region::Diagram,
        "diagramUp",
        &["up"],
        &["k"],
    ),
    both(
        Action::DiagramDown,
        Region::Diagram,
        "diagramDown",
        &["down"],
        &["j"],
    ),
    both(
        Action::DiagramLeft,
        Region::Diagram,
        "diagramLeft",
        &["left"],
        &["h"],
    ),
    both(
        Action::DiagramRight,
        Region::Diagram,
        "diagramRight",
        &["right"],
        &["l"],
    ),
    both(
        Action::DiagramPanUp,
        Region::Diagram,
        "diagramPanUp",
        &["shift+up"],
        &[],
    ),
    both(
        Action::DiagramPanDown,
        Region::Diagram,
        "diagramPanDown",
        &["shift+down"],
        &[],
    ),
    both(
        Action::DiagramPanLeft,
        Region::Diagram,
        "diagramPanLeft",
        &["shift+left"],
        &[],
    ),
    both(
        Action::DiagramPanRight,
        Region::Diagram,
        "diagramPanRight",
        &["shift+right"],
        &[],
    ),
    both(
        Action::DiagramPageUp,
        Region::Diagram,
        "diagramPageUp",
        &["pageup"],
        &[],
    ),
    both(
        Action::DiagramPageDown,
        Region::Diagram,
        "diagramPageDown",
        &["pagedown"],
        &[],
    ),
    both(
        Action::DiagramOpen,
        Region::Diagram,
        "diagramOpen",
        &["enter"],
        &[],
    ),
    both(
        Action::DiagramStructure,
        Region::Diagram,
        "diagramStructure",
        &["i"],
        &[],
    ),
    both(
        Action::DiagramCopy,
        Region::Diagram,
        "diagramCopy",
        &["y"],
        &[],
    ),
    both(
        Action::DiagramReload,
        Region::Diagram,
        "diagramReload",
        &["R"],
        &[],
    ),
    both(
        Action::DiagramKeysOnly,
        Region::Diagram,
        "diagramKeysOnly",
        &["K"],
        &[],
    ),
    both(
        Action::ReviewAccept,
        Region::Review,
        "reviewAccept",
        &["a", "tab"],
        &[],
    ),
    both(
        Action::ReviewReject,
        Region::Review,
        "reviewReject",
        &["r"],
        &[],
    ),
    both(
        Action::ReviewAcceptAll,
        Region::Review,
        "reviewAcceptAll",
        &["A"],
        &[],
    ),
    both(
        Action::ReviewRejectAll,
        Region::Review,
        "reviewRejectAll",
        &["R"],
        &[],
    ),
    both(
        Action::ReviewNext,
        Region::Review,
        "reviewNext",
        &["down", "j", "]"],
        &[],
    ),
    both(
        Action::ReviewPrevious,
        Region::Review,
        "reviewPrevious",
        &["up", "k", "["],
        &[],
    ),
    both(
        Action::ReviewClose,
        Region::Review,
        "reviewClose",
        &["esc"],
        &[],
    ),
    both(
        Action::AgentSessions,
        Region::Global,
        "agentSessions",
        &[],
        &[],
    ),
    both(Action::AgentReview, Region::Global, "agentReview", &[], &[]),
    both(Action::AgentDirect, Region::Global, "agentDirect", &[], &[]),
    both(Action::AgentStop, Region::Global, "agentStop", &[], &[]),
    both(
        Action::AgentSqlToConsole,
        Region::Global,
        "agentSqlToConsole",
        &[],
        &[],
    ),
    both(
        Action::AgentSqlClear,
        Region::Global,
        "agentSqlClear",
        &[],
        &[],
    ),
    both(
        Action::AgentSession,
        Region::Global,
        "agentSession",
        &[],
        &[],
    ),
    both(Action::AgentTasks, Region::Global, "agentTasks", &[], &[]),
];

/// The answer of the map to one key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    Action(Action),
    /// The keys start a sequence that needs more keys.
    Sequence,
    None,
}

#[derive(Debug)]
pub struct Keymap {
    regions: HashMap<Region, Bindings>,
    keys: HashMap<Action, Vec<String>>,
}

#[derive(Debug, Default)]
struct Bindings {
    complete: HashMap<Vec<Chord>, Action>,
    prefixes: HashMap<Vec<Chord>, Action>,
}

impl Keymap {
    pub fn new(mode: KeyboardMode) -> Self {
        Self::with_overrides(mode, &HashMap::new()).expect("the default keys have no conflict")
    }

    /// Applies the `keymap` part of the configuration. A list replaces the default
    /// keys of its action, and an empty list removes them.
    pub fn with_overrides(
        mode: KeyboardMode,
        overrides: &HashMap<String, Vec<String>>,
    ) -> Result<Self, String> {
        if let Some(name) = overrides
            .keys()
            .find(|name| !DEFAULTS.iter().any(|item| item.name == *name))
        {
            return Err(format!("Unknown action: {name}"));
        }
        let mut map = Self {
            regions: HashMap::new(),
            keys: HashMap::new(),
        };
        for item in DEFAULTS {
            let keys: Vec<String> = match overrides.get(item.name) {
                Some(keys) => keys.clone(),
                None => default_keys(item, mode),
            };
            for text in &keys {
                map.bind(item.region, item.action, item.name, text)?;
            }
            map.keys.insert(item.action, keys);
        }
        Ok(map)
    }

    fn bind(
        &mut self,
        region: Region,
        action: Action,
        name: &str,
        text: &str,
    ) -> Result<(), String> {
        let conflict = |other: Action| format!("{} conflicts with {name}: {text}.", other.name());
        let chords = text
            .split_whitespace()
            .map(Chord::parse)
            .collect::<Result<Vec<_>, _>>()?;
        if chords.is_empty() {
            return Err(format!("Unknown key: {text}"));
        }
        let bindings = self.regions.entry(region).or_default();
        for end in 1..=chords.len() {
            if let Some(other) = bindings.complete.get(&chords[..end]) {
                return Err(conflict(*other));
            }
        }
        if let Some(other) = bindings.prefixes.get(chords.as_slice()) {
            return Err(conflict(*other));
        }
        for end in 1..chords.len() {
            bindings.prefixes.insert(chords[..end].to_vec(), action);
        }
        bindings.complete.insert(chords, action);
        Ok(())
    }

    pub fn lookup(&self, region: Region, key: Chord) -> Lookup {
        self.sequence(region, &[key])
    }

    /// Returns the action or prefix of a sequence of any length.
    pub fn sequence(&self, region: Region, keys: &[Chord]) -> Lookup {
        let Some(bindings) = self.regions.get(&region) else {
            return Lookup::None;
        };
        if let Some(action) = bindings.complete.get(keys) {
            return Lookup::Action(*action);
        }
        if bindings.prefixes.contains_key(keys) {
            return Lookup::Sequence;
        }
        Lookup::None
    }

    /// Returns the action of a sequence with two keys.
    pub fn after(&self, region: Region, first: Chord, second: Chord) -> Option<Action> {
        match self.sequence(region, &[first, second]) {
            Lookup::Action(action) => Some(action),
            _ => None,
        }
    }

    /// Returns the keys of an action, as the user writes them.
    pub fn keys(&self, action: Action) -> &[String] {
        self.keys.get(&action).map_or(&[], Vec::as_slice)
    }

    /// Lists the actions of a region in the order of the help screen.
    pub fn actions(&self, region: Region) -> impl Iterator<Item = Action> + '_ {
        Action::in_region(region)
    }
}

fn default_keys(item: &Item, mode: KeyboardMode) -> Vec<String> {
    let vim = mode == KeyboardMode::Vim;
    let base = if vim && item.conventional_only {
        &[][..]
    } else {
        item.keys
    };
    let extra = if vim { item.vim_keys } else { &[] };
    base.iter()
        .chain(extra)
        .map(|key| (*key).to_owned())
        .collect()
}
