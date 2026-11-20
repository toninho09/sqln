//! Menus, the palette, the status line, the welcome page, and the home screen.

use crate::catalog::Group;

pub const TITLE_EXPLORER: &str = "EXPLORER";
pub const TITLE_PALETTE: &str = "Commands";
pub const TITLE_PREFIX: &str = "Ctrl+W";
pub const TITLE_EXPLORER_MENU: &str = "Ctrl+W · Explorer";
pub const NO_COMMANDS: &str = "No command matches";
pub const NO_ITEMS: &str = "No step or example matches";

pub const STATUS_EDITOR: &str = "EDITOR";
pub const STATUS_NORMAL: &str = "NORMAL";
pub const STATUS_INSERT: &str = "INSERT";
pub const STATUS_VISUAL: &str = "VISUAL";
pub const STATUS_RESULTS: &str = "RESULTS";
pub const STATUS_EXPLORER: &str = "EXPLORER";
pub const STATUS_FILTER: &str = "FILTER";
pub const STATUS_TABLE: &str = "TABLE";
pub const STATUS_DIAGRAM: &str = "DIAGRAM";
pub const STATUS_HOME: &str = "HOME";

pub const GROUP_PANELS: &str = "Panels";
pub const GROUP_TABS: &str = "Tabs";
pub const GROUP_RUN: &str = "Run";
pub const GROUP_TRANSACTION: &str = "Transaction";
pub const GROUP_CONNECTION: &str = "Connection";
pub const GROUP_APPLICATION: &str = "Application";
pub const GROUP_CONSOLE: &str = "Console";
pub const GROUP_ROW: &str = "Row";
pub const GROUP_REVIEW: &str = "Changes";
pub const MENU_FOCUS: &str = "Move the focus";
pub const MENU_NAVIGATOR: &str = "Toggle the explorer";
pub const MENU_NEXT_TAB: &str = "Next tab";
pub const MENU_PREVIOUS_TAB: &str = "Previous tab";
pub const MENU_RENAME: &str = "Rename the tab";
pub const MENU_CLOSE_TAB: &str = "Close the tab";
pub const MENU_RUN: &str = "Run statement";
pub const MENU_SCRIPT: &str = "Run script";
pub const MENU_SUBQUERY: &str = "Run subquery";
pub const RUN_SELECTION: &str = "Run selection";

pub fn run_label(selection: bool) -> &'static str {
    if selection { RUN_SELECTION } else { MENU_RUN }
}

pub fn run_hint(selection: bool) -> &'static str {
    if selection {
        "run selection"
    } else {
        "run statement"
    }
}
pub const MENU_EXPLORER: &str = "Explorer commands";
pub const MENU_CONTEXT: &str = "Panel commands";
pub const MENU_OPEN: &str = "Open";
pub const MENU_EXPAND: &str = "Expand";
pub const MENU_COLLAPSE: &str = "Collapse/parent";
pub const MENU_CLEAR_FILTER: &str = "Clear filter";
pub const MENU_LOAD_GRID: &str = "Load grid";
pub const COMMAND_LOAD_GRID: &str = "Load the grid";
pub const COMMAND_OPEN_CONSOLE: &str = "Open the console";
pub const COMMAND_OPEN_DATA: &str = "Open the data";
pub const COMMAND_OPEN_DDL: &str = "Open the DDL in a new console";
pub const COMMAND_TOGGLE_ROW: &str = "Open or close the row";
pub const GROUP_SELECTION_VIEW: &str = "Selection and view";
pub const GROUP_TABLE_RESULTS: &str = "Table and results";
pub const MENU_SESSIONS: &str = "Sessions";
pub const MENU_MOVE_UP: &str = "Move up";
pub const MENU_MOVE_DOWN: &str = "Move down";
pub const MENU_MOVE_TO_FOLDER: &str = "Move to folder";
pub const MENU_RENAME_FOLDER: &str = "Rename folder";
pub const MENU_REMOVE_FOLDER: &str = "Remove folder";
pub const MENU_NEW_CONSOLE: &str = "New console";
pub const MENU_SCRIPTS: &str = "Scripts";
pub const MENU_REFRESH: &str = "Read the catalog";
pub const MENU_RENAME_CONSOLE: &str = "Rename the console";
pub const MENU_REMOVE_CONSOLE: &str = "Remove the console";
pub const MENU_EDIT_CONNECTION: &str = "Edit connection";
pub const MENU_ADD_CONNECTION: &str = "Add a connection";
pub const MENU_REMOVE_CONNECTION: &str = "Remove connection";
pub const MENU_DISCONNECT: &str = "Disconnect";
pub const MENU_DELETE_PASSWORD: &str = "Delete the password";
pub const MENU_STRUCTURE: &str = "Show the structure";
pub const MENU_DIAGRAM: &str = "ER diagram";
pub const MENU_COMPARE: &str = "Compare";
pub const MENU_GENERATE: &str = "Generate SQL";
pub const MENU_IMPORT: &str = "Import a file";
pub const MENU_COPY_NAME: &str = "Copy the name";
pub const MENU_FILTER: &str = "Filter the tables";
pub const MENU_EXPLAIN: &str = "Show the plan";
pub const MENU_EXPLAIN_ANALYZE: &str = "Plan with a run";
pub const MENU_AI_EXPLAIN_ERROR: &str = "Explain error (AI)";
pub const MENU_AI_EXPLAIN_PLAN: &str = "Explain plan (AI)";
pub const MENU_STOP: &str = "Stop";
pub const MENU_MODE: &str = "Auto or manual";
pub const MENU_COMMIT: &str = "Commit";
pub const MENU_ROLLBACK: &str = "Rollback";
pub const MENU_FORMAT: &str = "Format the SQL";
pub const MENU_COMMENT: &str = "Comment the rows";
pub const MENU_HISTORY: &str = "Query history";
pub const MENU_SCHEMA: &str = "Schema or database";
pub const MENU_CHANGE: &str = "Change connection";
pub const MENU_SETTINGS: &str = "Settings";
pub const MENU_RELOAD: &str = "Reload the config";
pub const MENU_COMMANDS: &str = "Commands";
pub const MENU_KEYS: &str = "All keys";
pub const MENU_QUIT: &str = "Quit";

pub const WELCOME_NO_CONNECTION: &str = "No connection yet";
pub const WELCOME_WRITE: &str = "Write SQL here";
pub const WELCOME_ADD: &str = "Add a connection";
pub const WELCOME_RUN: &str = MENU_RUN;
pub const WELCOME_SCRIPT: &str = MENU_SCRIPT;
pub const WELCOME_SUGGESTIONS: &str = "Show suggestions";
pub const WELCOME_SUGGESTIONS_KEY: &str = "Ctrl+Space";
pub const WELCOME_COMMANDS: &str = "Show all commands";
pub const WELCOME_KEYS: &str = "Show help";

pub const HOME_NO_CONSOLE: &str = "No console is open";
pub const HOME_EXPLORER: &str = "Show the explorer";
pub const HOME_CONSOLE: &str = "Open a console";
pub const HOME_CONNECT: &str = "Connect to a database";
pub const HOME_OPEN_SQL: &str = "Open a SQL file";
pub const HOME_SQL_NEEDS_CONNECTION: &str = "Add a connection before opening SQL files";
pub const HOME_SAVED: &str = "Open saved work";
pub const HOME_SAVED_CONSOLES: &str = "Saved consoles of selected connection";
pub const HOME_SCRIPTS: &str = "Scripts without a connection";
pub const HOME_SETTINGS: &str = "Open the settings";
pub const HOME_QUIT: &str = "Quit SQLN";
/// The version under the logo of the home screen.
pub const HOME_VERSION: &str = env!("CARGO_PKG_VERSION");
/// The name of the application in large letters, for the home screen. Only ASCII, so
/// each terminal shows it.
pub const HOME_LOGO: [&str; 5] = [
    r"  ____   ___  _     _   _ ",
    r" / ___| / _ \| |   | \ | |",
    r" \___ \| | | | |   |  \| |",
    r"  ___) | |_| | |___| |\  |",
    r" |____/ \__\_\_____|_| \_|",
];

/// Writes the name and the version for the first screen.
pub fn welcome_title() -> String {
    format!("SQLN {}", env!("CARGO_PKG_VERSION"))
}
/// Returns the name of a group of the explorer.
pub fn group_name(group: Group) -> &'static str {
    match group {
        Group::Routines => "Routines",
        Group::Sequences => "Sequences",
        Group::Triggers => "Triggers",
        Group::Types => "Types",
    }
}
pub const MENU_FOLLOW: &str = "Follow a key";
pub const CONTROL_RUN: &str = MENU_RUN;
pub const CONTROL_SCRIPT: &str = MENU_SCRIPT;
pub const CONTROL_STOP: &str = "Stop";
/// The button at the right end of the main tabs. It quits as `Ctrl+W q`.
pub const CONTROL_QUIT: &str = "Quit";
pub const CONTROL_COMMIT: &str = "Commit";
pub const CONTROL_ROLLBACK: &str = "Rollback";
pub const REGION_EDITOR: &str = "Editor";
pub const REGION_NAVIGATOR: &str = "Navigator";
pub const REGION_GRID: &str = "Results";
pub const MODE_CONVENTIONAL: &str = "Conventional";
pub const MODE_VIM: &str = "Vim";
pub const GROUP_ANALYSIS: &str = "Analysis";
pub const REGION_TABLE: &str = "Table";
pub const REGION_DIAGRAM: &str = "Diagram";
pub const REGION_REVIEW: &str = "Review";
pub const REGION_COMPARE: &str = "Compare";
pub const REGION_FILTER: &str = "Filter";
pub const MODE_AUTO: &str = "Auto";
pub const MODE_MANUAL: &str = "Manual";

pub fn command_result_target(console: &str, result: usize) -> String {
    format!("{console} · result {result}")
}

/// Writes the title of the explorer page, with the row that its commands act on.
pub fn explorer_menu_title(row: Option<&str>) -> String {
    match row {
        Some(row) => format!("{TITLE_EXPLORER_MENU} · {row}"),
        None => TITLE_EXPLORER_MENU.to_owned(),
    }
}
