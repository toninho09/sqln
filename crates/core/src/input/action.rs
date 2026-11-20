//! The regions and the actions of the key map, with the facts of each action.

use super::*;

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
    RunSubquery,
    Explain,
    ExplainAnalyze,
    FormatSql,
    SqlDiagnostics,
    ToggleComment,
    OpenHistory,
    /// Opens an external `.sql` file as a console: a copy, or a link that saves to it.
    OpenSqlFile,
    OpenSessions,
    /// Opens the sessions that wait for a lock, with the sessions that hold it.
    OpenLocks,
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
    EditorAiCompleteRequest,
    AiExplainError,
    AiExplainPlan,
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
    NavigatorLocks,
    NavigatorGenerate,
    NavigatorImport,
    NavigatorCopyName,
    NavigatorMoveUp,
    NavigatorMoveDown,
    NavigatorMoveToFolder,
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
    GridProfile,
    GridChart,
    GridKeepPrevious,
    GridComparePrevious,
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
    AutoRefresh,
    GridEditCell,
    GridSetNull,
    GridNewRow,
    GridDeleteRow,
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
    #[expect(clippy::unwrap_used, reason = "DEFAULTS has an item for each action")]
    pub(super) fn item(self) -> &'static Item {
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

    /// Returns true when the grid has the action.
    pub fn on_grid(self, grid: Grid) -> bool {
        let grids = self.item().grids;
        match grid {
            Grid::Console => grids.console,
            Grid::Table => grids.table,
            Grid::Compare => grids.compare,
        }
    }

    /// Returns true for an action that the grid of a table has and the results of a
    /// console do not have.
    pub fn is_table_only(self) -> bool {
        self.on_grid(Grid::Table) && !self.on_grid(Grid::Console)
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

pub(super) struct Item {
    pub(super) action: Action,
    pub(super) region: Region,
    /// The name of the action in `config.json`.
    pub(super) name: &'static str,
    pub(super) keys: &'static [&'static str],
    /// The keys of the Vim mode, in addition to `keys`.
    pub(super) vim_keys: &'static [&'static str],
    /// True when `keys` applies only to the conventional mode.
    pub(super) conventional_only: bool,
    /// The grids that have the action. An action outside the grid has all of them.
    pub(super) grids: Grids,
}

/// The grids that have an action: the results of a console, a table tab, and a compare
/// tab.
#[derive(Clone, Copy)]
pub(super) struct Grids {
    pub(super) console: bool,
    pub(super) table: bool,
    pub(super) compare: bool,
}

impl Grids {
    pub(super) const ALL: Self = Self::new(true, true, true);
    pub(super) const CONSOLE: Self = Self::new(true, false, false);
    /// The grids with a list of results and an import: a console and a table.
    pub(super) const RESULTS: Self = Self::new(true, true, false);
    pub(super) const TABLE: Self = Self::new(false, true, false);
    pub(super) const RELOAD: Self = Self::new(false, true, true);
    pub(super) const COMPARE: Self = Self::new(false, false, true);

    pub(super) const fn new(console: bool, table: bool, compare: bool) -> Self {
        Self {
            console,
            table,
            compare,
        }
    }
}

/// A grid of results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grid {
    Console,
    Table,
    Compare,
}

pub(super) const fn both(
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
        grids: Grids::ALL,
    }
}

impl Item {
    /// Limits the action to some grids.
    pub(super) const fn only(self, grids: Grids) -> Self {
        Self { grids, ..self }
    }
}
