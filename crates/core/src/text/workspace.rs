//! History, confirmations, configuration, workspace, scripts, and SQL files.

use super::BUTTON_RELOAD;

pub const TITLE_HISTORY: &str = "Query history";
pub const NO_HISTORY: &str = "This connection has no history.";
pub const NO_HISTORY_MATCH: &str = "No statement has this text.";

pub fn history_failed(message: &str) -> String {
    format!("The query history did not save: {message}")
}

pub const FORMAT_UNCHANGED: &str = "The SQL has this format already.";
pub const FORMAT_REFUSED: &str = "SQLN cannot format this text. Close its string or comment.";

pub const TITLE_CONFIRM_RUN: &str = "Confirm the run";
pub const OPTION_RUN: &str = "Run";

/// The risks of one run, for the question before it starts.
pub struct RunRisk<'a> {
    /// The name of a production connection.
    pub production: Option<&'a str>,
    pub changes: usize,
    pub no_where: usize,
    pub total: usize,
    /// The start of the first statement with a risk.
    pub first: &'a str,
}

pub fn run_risk(risk: &RunRisk) -> String {
    let RunRisk {
        production,
        changes,
        no_where,
        total,
        first,
    } = risk;
    let mut lines = Vec::new();
    if let Some(name) = production {
        lines.push(format!("{name} is a production connection."));
        lines.push(format!(
            "Statements that can change data: {changes} of {total}."
        ));
    }
    if *no_where > 0 {
        lines.push(format!(
            "Statements without WHERE: {no_where} of {total}. They change all rows."
        ));
    }
    lines.push(String::new());
    lines.push((*first).to_owned());
    lines.join("\n")
}

/// The question of the command line, before the interface starts.
pub fn password_prompt(user: &str) -> String {
    format!("Password for {user} (Enter for none): ")
}
/// The question for the token of a source whose password field holds a token.
pub fn token_prompt(source: &str) -> String {
    format!("{source} token: ")
}
/// The question of the command line for a Schema Registry user without a password.
pub fn registry_password_prompt(user: &str) -> String {
    format!("Schema Registry password for {user} (Enter for none): ")
}
pub const READ_ONLY_NO_FILE: &str =
    "The database file does not exist. A read-only connection cannot create it.";
/// The refusal of connection options without a target.
pub const OPTIONS_NEED_TARGET: &str =
    "Connection options need a target. Change the options of a saved connection in its form.";

// Files: the configuration, the project, and the saved consoles.
pub const TITLE_CREATE_PROJECT: &str = "Create project";
pub const OPTION_CREATE_PROJECT: &str = "Create project";
pub const OPTION_GLOBAL_ONLY: &str = "Global settings only";
pub const TITLE_CONFIG_ERROR: &str = "Invalid configuration";
pub const TITLE_CONFIG_CHANGED: &str = "Configuration changed";
pub const BODY_CONFIG_CHANGED: &str = "Another program changed a configuration file. Open sessions stay as they are. Load the configuration again?";
pub const OPTION_CLOSE: &str = "Close";
pub const OPTION_RELOAD: &str = "Reload";
pub const OPTION_IGNORE: &str = "Ignore";
pub const CONFIG_BLOCKED: &str =
    "The configuration has an error. Fix the file, then press Ctrl+W r.";
pub const CONFIG_RELOADED: &str = "Configuration loaded again.";
pub const NO_STORE: &str = "This session has no configuration files.";
pub const NO_PROJECT: &str = "This session has no project. Create a .sqln folder first.";
pub const PROJECT_CREATED: &str = "Project created.";
pub const SAVED: &str = "Saved.";
pub const NOTHING_TO_SAVE: &str = "This console has no text to save.";
pub const NO_PLACE: &str = "The console has no connection, so it has no place to save.";
pub const TITLE_UNSAVED: &str = "Text not saved";
pub const OPTION_CLOSE_UNSAVED: &str = "Close without saving";
pub const OPTION_CLOSE_DISCARD: &str = "Close and discard the text";
pub const WORKSPACE_UNREADABLE: &str =
    "SQLN cannot read workspace.json. The layout and the cursors start again.";
pub const AGE_NOW: &str = "just now";
pub const MARK_OPEN: &str = " (open)";
/// The node above the consoles of a connection in the navigator.
pub const NODE_CONSOLES: &str = "Consoles";
pub const NEW_CONSOLE_ROW: &str = "+ New console";
pub const NO_ORPHANS: &str = "No scripts without a connection.";
pub const TITLE_ORPHANS: &str = "Scripts without a connection";
pub const TITLE_ASSOCIATE: &str = "Choose a connection";
pub const TITLE_ASSOCIATE_CONFIRM: &str = "Associate script";
pub const OPTION_ASSOCIATE: &str = "Associate";
pub const ASSOCIATE_SAVE_FIRST: &str = "The script has a save problem. Resolve it, then try again.";
pub const TITLE_REMOVE_CONSOLE: &str = "Remove console";
pub const SCRIPT_OPEN: &str = "Close the console before you remove its script.";
pub const SCRIPT_BUSY: &str = "Another SQLN instance uses this script.";
pub const SCRIPT_REMOVED: &str = "Script removed.";
pub const TITLE_CONFLICTS: &str = "Scripts changed outside SQLN";
pub const TITLE_SQL_FILE: &str = "Open a SQL file";
pub const OPTION_SQL_FILE_COPY: &str = "Copy";
pub const OPTION_SQL_FILE_LINKED: &str = "Linked to the file";
pub const TITLE_SQL_FILE_CHANGED: &str = "File changed outside SQLN";
pub const TITLE_SQL_FILE_REMOVED: &str = "File removed";
pub const OPTION_SQL_FILE_RELOAD: &str = "Reload the file";
pub const OPTION_SQL_FILE_OVERWRITE: &str = "Overwrite the file";
pub const OPTION_SQL_FILE_RECREATE: &str = "Recreate the file";
pub const OPTION_SQL_FILE_WRITE_CLOSE: &str = "Write and close";
pub const OPTION_SQL_FILE_CLOSE_WITHOUT: &str = "Close without writing";
pub const SQL_FILE_RELOADED: &str = "The file is open again.";
pub const STATE_CHANGED: &str = "changed";
pub const STATE_REMOVED: &str = "removed";
pub const CHOICE_NONE: &str = "choose";
pub const CHOICE_RELOAD: &str = "Reload";
pub const CHOICE_OVERWRITE: &str = "Overwrite";
pub const CHOICE_RECREATE: &str = "Recreate";
pub const CHOICE_CLOSE: &str = "Close";
pub const BUTTON_LATER: &str = "Decide later";
pub const CONFLICT_CHOOSE: &str = "Choose an action for each script.";

pub fn create_project(folder: &str) -> String {
    format!("SQLN did not find a .sqln folder. Create a project in {folder}?")
}

pub fn project_failed(message: &str) -> String {
    format!("Cannot create the project: {message}")
}

pub fn config_error(message: &str, file: &str) -> String {
    format!(
        "{message}\n\nFile: {file}\n\nSQLN does not connect to a database until the file is correct. Fix the file, then use Reload."
    )
}

pub fn save_failed(name: &str, message: &str) -> String {
    format!("Cannot save {name}: {message} The text stays in the editor.")
}

pub fn unsaved_failed(name: &str, message: &str) -> String {
    format!(
        "SQLN cannot save {name}: {message}\n\nClose the console and lose the text that no file holds?"
    )
}

pub fn unsaved_changed(name: &str) -> String {
    format!(
        "Another program changed the script of {name}. SQLN did not save the text.\n\nClose the console and lose the text that no file holds?"
    )
}

pub fn unsaved_removed(name: &str) -> String {
    format!(
        "Another program removed the script of {name}. SQLN did not save the text.\n\nClose the console and lose the text that no file holds?"
    )
}

pub fn sql_file_question(path: &str) -> String {
    format!(
        "{path}\n\nOpen the file as a copy of its text, or link the console to the file? A linked console writes the file on each save."
    )
}

pub fn sql_file_failed(path: &str, message: &str) -> String {
    format!("Cannot open {path}: {message}")
}

pub fn sql_file_changed(path: &str) -> String {
    format!(
        "Another program changed {path} after this console read it.\n\nReload the file and lose the text of the console, or write the text over the file?"
    )
}

pub fn sql_file_removed(path: &str) -> String {
    format!("Another program removed {path}.\n\nRecreate the file with the text of the console?")
}

pub fn sql_file_unsaved(path: &str) -> String {
    format!(
        "The console holds changes for {path} that the file does not have.\n\nWrite the file before the console closes?"
    )
}

pub fn age_minutes(minutes: u64) -> String {
    format!("{minutes} min ago")
}

pub fn age_hours(hours: u64) -> String {
    format!("{hours} h ago")
}

pub fn age_days(days: u64) -> String {
    format!("{days} d ago")
}

pub fn consoles_title(connection: &str, scope: &str) -> String {
    format!("Consoles of {connection} · {scope}")
}

/// Tells how to load a table tab of the last session. A key that the user removed
/// leaves the button.
pub fn table_pending(key: Option<&str>) -> String {
    format!("{} loads the rows.", key.unwrap_or(BUTTON_RELOAD))
}

pub fn script_in_use(name: &str) -> String {
    format!("{name} is open in another SQLN instance.")
}

pub fn script_failed(name: &str, message: &str) -> String {
    format!("Cannot use the script {name}: {message}")
}

pub fn remove_script(name: &str, folder: &str) -> String {
    format!(
        "Remove the script {name}? SQLN deletes its .sql file and its .json file in {folder}. This action cannot be undone."
    )
}

pub fn associate_same(name: &str, connection: &str, folder: &str) -> String {
    format!(
        "Associate {name} with {connection}?\n\nThe files stay in {folder}.\nSQLN does not run any SQL."
    )
}

pub fn associate_move(name: &str, connection: &str, scope: &str, folder: &str) -> String {
    format!(
        "Associate {name} with {connection}?\n\nThe connection has the {scope} scope. SQLN moves the files to {folder}.\nSQLN does not run any SQL."
    )
}

pub fn associated(connection: &str) -> String {
    format!("The script now uses {connection}. Run it again to execute.")
}
