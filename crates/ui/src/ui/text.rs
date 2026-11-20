//! Interface text that needs the types of the interface layers.
//! The other text is in `crate::text`.

use crate::export::ExportFormat;
use crate::generate::Template;
use crate::import::ImportFormat;
use crate::import_form::Section;
use crate::input::{Action, SuggestionKeys};
use crate::structure::SessionSignal;

pub use crate::text::*;

/// Lists the commands of an open suggestion list.
pub fn hint_list(keys: SuggestionKeys, (enter, tab): (HintPart, HintPart)) -> Vec<HintPart> {
    let (accept, other) = match keys {
        SuggestionKeys::Tab => ("Tab", Some(enter)),
        SuggestionKeys::Enter => ("Enter", Some(tab)),
        SuggestionKeys::Both => ("Tab/Enter", None),
    };
    let mut parts = vec![(accept, "accept"), ("Up/Down", "move")];
    parts.extend(other);
    parts.push(("Esc", "close list"));
    parts
}

pub fn suggestion_keys_name(keys: SuggestionKeys) -> &'static str {
    match keys {
        SuggestionKeys::Tab => "Tab",
        SuggestionKeys::Enter => "Enter",
        SuggestionKeys::Both => "Tab or Enter",
    }
}

pub fn explorer_side_name(side: crate::store::config::ExplorerSide) -> &'static str {
    use crate::store::config::ExplorerSide;
    match side {
        ExplorerSide::Left => "Left",
        ExplorerSide::Right => "Right",
    }
}

pub fn theme_name(name: &crate::store::config::ThemeName) -> &'static str {
    use crate::store::config::ThemeName;
    match name {
        ThemeName::Auto => "Auto",
        ThemeName::Astrodark => "Astrodark",
        ThemeName::Ansi => "Terminal colors",
        ThemeName::TokyoNight => "Tokyo Night",
        ThemeName::CatppuccinMocha => "Catppuccin Mocha",
        ThemeName::CatppuccinLatte => "Catppuccin Latte",
        ThemeName::Gruvbox => "Gruvbox",
        ThemeName::Nord => "Nord",
        ThemeName::Dracula => "Dracula",
        ThemeName::OneDark => "One Dark",
        ThemeName::Monokai => "Monokai",
        ThemeName::SolarizedDark => "Solarized Dark",
        ThemeName::SolarizedLight => "Solarized Light",
        ThemeName::GithubDark => "GitHub Dark",
        ThemeName::RosePine => "Rose Pine",
        ThemeName::HighContrast => "High Contrast",
        ThemeName::HighContrastLight => "High Contrast Light",
        ThemeName::Everforest => "Everforest",
        ThemeName::Kanagawa => "Kanagawa",
        ThemeName::NightOwl => "Night Owl",
        ThemeName::RosePineDawn => "Rose Pine Dawn",
        ThemeName::EverforestLight => "Everforest Light",
        ThemeName::Ember => "Ember",
        ThemeName::Custom(_) => "Custom",
    }
}

/// The name of a theme in lists. A custom theme shows its own name.
pub fn theme_label(
    name: &crate::store::config::ThemeName,
    custom: &crate::ui::theme::ThemeCatalog,
) -> String {
    match name {
        crate::store::config::ThemeName::Custom(id) => {
            crate::grid::printable(custom.name_of(id).unwrap_or(id))
        }
        name => theme_name(name).to_owned(),
    }
}

pub fn connection_icon_name(icon: crate::store::config::ConnectionIcon) -> &'static str {
    use crate::store::config::ConnectionIcon;
    match icon {
        ConnectionIcon::Warning => "Warning",
        ConnectionIcon::Skull => "Skull",
        ConnectionIcon::Radiation => "Radiation",
        ConnectionIcon::Flag => "Flag",
        ConnectionIcon::Crown => "Crown",
        ConnectionIcon::Flask => "Flask",
        ConnectionIcon::Star => "Star",
        ConnectionIcon::Diamond => "Diamond",
        ConnectionIcon::Tools => "Tools",
        ConnectionIcon::Gear => "Gear",
        ConnectionIcon::Home => "Home",
        ConnectionIcon::Heart => "Heart",
        ConnectionIcon::Snowflake => "Snowflake",
        ConnectionIcon::Check => "Check",
    }
}
/// One topic of the getting-started guide. The help shows the effective key of each step.
/// An example is a text and a statement.
pub struct GuideText {
    /// The short name in the sidebar.
    pub name: &'static str,
    pub title: &'static str,
    pub intro: &'static str,
    pub steps: &'static [(Action, &'static str)],
    pub examples: &'static [(&'static str, &'static str)],
}

pub const GUIDE: &[GuideText] = &[
    GuideText {
        name: "Connect",
        title: "1. Connect to a database",
        intro: "A connection holds the address of a database: a URI, such as postgres://user@host/db, or the path of a SQLite file. A console is a SQL editor on one connection.",
        steps: &[
            (Action::NavigatorAdd, "Add a connection"),
            (
                Action::NavigatorNewConsole,
                "Open a console on the selected connection",
            ),
            (Action::ChooseSchema, "Choose the schema of the console"),
            (
                Action::ChangeConnection,
                "Change the connection of the console",
            ),
        ],
        examples: &[],
    },
    GuideText {
        name: "Run SQL",
        title: "2. Run SQL and read the results",
        intro: "Write SQL in the console. The results show below the editor. The interface stays active while a query runs, and you can stop the query.",
        steps: &[
            (
                Action::Run,
                "Run the statement at the cursor, or the selection",
            ),
            (Action::RunScript, "Run all the statements of the console"),
            (Action::Stop, "Stop the query"),
            (Action::FocusDown, "Go to the results"),
            (Action::FocusUp, "Go to the editor"),
            (
                Action::GridOpenValue,
                "In the results, show the full value of the cell",
            ),
            (Action::GridCopy, "In the results, copy the selected cells"),
            (Action::GridExport, "Export the result to a file"),
            (Action::ToggleMode, "Use autocommit or a manual transaction"),
            (Action::Commit, "Commit the transaction"),
            (Action::Rollback, "Roll back the transaction"),
        ],
        examples: &[],
    },
    GuideText {
        name: "Tables",
        title: "3. Explore the tables",
        intro: "The explorer shows the connections, schemas, tables, and columns. A table opens in a tab. In the tab, you can filter, sort, and edit the rows.",
        steps: &[
            (Action::ToggleNavigator, "Show or hide the explorer"),
            (Action::NavigatorOpen, "Open the selected console or table"),
            (
                Action::NavigatorStructure,
                "Show the structure of the selected table",
            ),
            (
                Action::NavigatorDiagram,
                "Show the ER diagram of the selected schema",
            ),
            (
                Action::GridFocusFilter,
                "In a table tab, write a WHERE filter",
            ),
            (Action::GridEditCell, "In a table tab, edit the cell"),
            (Action::GridSaveEdits, "Review and save the changes"),
        ],
        examples: &[],
    },
    GuideText {
        name: "Kafka",
        title: "4. Read Kafka with SELECT",
        intro: "A Kafka connection uses kafka://host:9092, or kafkas:// with TLS. SQLN reads the messages in the client with a SELECT subset: joins, aggregates, and ORDER BY are not supported. A plain SELECT reads the retained messages and stops at the last one. docs/KAFKA.md has the full syntax.",
        steps: &[
            (
                Action::NavigatorOpen,
                "Open the last 500 messages of the selected topic",
            ),
            (Action::Run, "Run the statement at the cursor"),
            (Action::Stop, "Stop the read"),
        ],
        examples: &[
            ("List the topics", "TOPICS"),
            (
                "List the partitions of a topic, with their offsets",
                "PARTITIONS orders",
            ),
            (
                "Read the retained messages from the first offset",
                "SELECT * FROM orders;",
            ),
            (
                "Read up to 100 messages that match",
                "SELECT * FROM orders LIMIT 100;",
            ),
            (
                "Examine at most 50,000 messages, with or without a match",
                "SELECT * FROM orders WHERE value.status = 'failed' LIMIT 100 SCAN 50000;",
            ),
            ("Read one partition", "SELECT * FROM orders PARTITION 2;"),
            (
                "Start at an offset",
                "SELECT * FROM orders START AT OFFSET 1500;",
            ),
            (
                "Start at a time",
                "SELECT * FROM orders START AT TIMESTAMP '2026-11-10T12:00:00Z';",
            ),
            (
                "Filter on a field of the value",
                "SELECT * FROM orders WHERE value.total > 1000;",
            ),
            (
                "Filter on the key",
                "SELECT * FROM orders WHERE key = 'customer-42';",
            ),
            (
                "Filter on a header",
                "SELECT * FROM orders WHERE headers.source = 'web';",
            ),
            (
                "Filter on the metadata",
                "SELECT * FROM orders WHERE partition = 2 AND offset > 1500;",
            ),
            (
                "Find the messages without a field",
                "SELECT * FROM orders WHERE value.notes IS MISSING;",
            ),
            (
                "Watch the new messages until you stop the read",
                "SELECT * FROM orders EMIT CHANGES;",
            ),
        ],
    },
    GuideText {
        name: "Scripts",
        title: "5. Scripts, commands, and settings",
        intro: "SQLN saves each console automatically as a script. The settings change the keyboard mode (conventional or Vim), the shortcuts, the theme, and the AI completion. The docs folder explains Kafka, AI, and MCP.",
        steps: &[
            (Action::EditorSave, "Save the console now"),
            (
                Action::NavigatorScripts,
                "Open the scripts without a connection",
            ),
            (Action::OpenSqlFile, "Open a SQL file as a console"),
            (Action::OpenPalette, "Find and run a command"),
            (
                Action::ContextCommands,
                "Show the commands of the current panel",
            ),
            (
                Action::ExplorerCommands,
                "Show the commands of the explorer",
            ),
            (Action::OpenSettings, "Open the settings"),
        ],
        examples: &[],
    },
];

pub fn run_preview(
    preview: Result<crate::editor::RunPreview, crate::editor::RunRefusal>,
    selection: bool,
    script: bool,
) -> String {
    let target = match preview {
        Ok(preview) => {
            let kind = if selection { "Selection" } else { "Statement" };
            if preview.first_line == preview.last_line {
                format!("{kind}: Line {}", preview.first_line)
            } else {
                format!("{kind}: Lines {}–{}", preview.first_line, preview.last_line)
            }
        }
        Err(crate::editor::RunRefusal::CursorOutside) => "No statement at cursor".to_owned(),
        Err(crate::editor::RunRefusal::NoStatement) => "No statement to run".to_owned(),
        Err(crate::editor::RunRefusal::NoSubquery) => NO_SUBQUERY.to_owned(),
    };
    if script {
        format!("{target} · Script: whole script")
    } else {
        target
    }
}

pub fn shortcut_region(region: crate::input::Region) -> &'static str {
    match region {
        crate::input::Region::Global => VALUE_SCOPE_GLOBAL,
        crate::input::Region::Editor => REGION_EDITOR,
        crate::input::Region::Navigator => REGION_NAVIGATOR,
        crate::input::Region::Grid => REGION_GRID,
        crate::input::Region::Diagram => REGION_DIAGRAM,
        crate::input::Region::Review => REGION_REVIEW,
    }
}
/// Returns the row of a SQL template in its list.
pub fn template_name(template: Template) -> &'static str {
    match template {
        Template::SelectAll => "SELECT *",
        Template::SelectColumns => "SELECT with the columns",
        Template::Insert => "INSERT",
        Template::Update => "UPDATE",
        Template::Delete => "DELETE",
    }
}

/// The warning of a theme that the files no longer hold.
pub fn theme_missing(id: &str) -> String {
    format!(
        "The theme {} is missing. The default theme applies.",
        crate::grid::printable(id)
    )
}

pub fn export_format_name(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Csv => "CSV",
        ExportFormat::Tsv => "TSV",
        ExportFormat::Json => "JSON",
        ExportFormat::JsonLines => "JSON Lines",
        ExportFormat::SqlInsert => "SQL INSERT",
    }
}

pub fn import_format_name(format: ImportFormat) -> &'static str {
    match format {
        ImportFormat::Csv => "CSV",
        ImportFormat::Tsv => "TSV",
        ImportFormat::Json => "JSON",
        ImportFormat::JsonLines => "JSON Lines",
    }
}

pub fn import_section_name(section: Section) -> &'static str {
    match section {
        Section::File => "File",
        Section::Format => "Format",
        Section::Columns => "Columns",
        Section::Options => "Options",
        Section::Preview => "Preview",
    }
}

pub fn keyboard_name(name: crate::store::config::KeyboardName) -> String {
    match name {
        crate::store::config::KeyboardName::Conventional => MODE_CONVENTIONAL.to_owned(),
        crate::store::config::KeyboardName::Vim => MODE_VIM.to_owned(),
    }
}

/// The note of an empty suggestion list that waits for the columns of a relation. The
/// columns of a Kafka topic are the fields of its schemas.
pub fn reading_columns(relation: &str, dialect: crate::sql::Dialect) -> String {
    if dialect == crate::sql::Dialect::Kafka {
        format!("Reading the fields of {relation}.")
    } else {
        format!("Reading the columns of {relation}.")
    }
}

pub fn syntax_summary(report: &crate::sql::diagnostics::Report, row: usize) -> Option<String> {
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|item| item.location.is_some_and(|(line, _)| line == row + 1))
        .or_else(|| report.diagnostics.first());
    diagnostic
        .map(|item| {
            let place = item
                .location
                .map_or_else(String::new, |(line, column)| format!(" L{line}:C{column}"));
            let partial = if report.note.is_some() {
                ", partial"
            } else {
                ""
            };
            format!(
                "Syntax warning ({}{partial}){place}: {}",
                report.diagnostics.len(),
                item.message
            )
        })
        .or_else(|| report.note.clone())
}

pub fn syntax_details(report: &crate::sql::diagnostics::Report) -> String {
    let mut text = format!("{SYNTAX_LIMITATION}\n\n");
    for (index, item) in report.diagnostics.iter().enumerate() {
        let place = item.location.map_or_else(
            || "Location unavailable".into(),
            |(line, column)| format!("Line {line}, column {column}"),
        );
        text.push_str(&format!("{}. {place}: {}\n\n", index + 1, item.message));
    }
    if let Some(note) = &report.note {
        text.push_str(note);
    }
    text
}

pub fn action_text(action: Action) -> &'static str {
    match action {
        Action::AgentSessions => "Agent sessions: allow or revoke access",
        Action::AgentReview => "Review agent changes",
        Action::AgentDirect => "Toggle agent direct editing for this console",
        Action::AgentStop => "Stop all agent tasks",
        Action::AgentSqlToConsole => "Copy the agent SQL to a new console",
        Action::AgentSqlClear => "Clear the agent SQL",
        Action::AgentSession => "Agent session of this tab: change or revoke access",
        Action::AgentTasks => "Agent tasks of this session: stop a task",
        Action::Run => "Run the selection or the statement",
        Action::RunScript => "Run the script",
        Action::RunSubquery => MENU_SUBQUERY,
        Action::Explain => "Show the plan of the statement",
        Action::ExplainAnalyze => "Run the statement and show its plan",
        Action::Stop => "Stop the script",
        Action::ToggleMode => "Autocommit or manual",
        Action::Commit => "Commit",
        Action::Rollback => "Rollback",
        Action::FormatSql => "Format the SQL",
        Action::SqlDiagnostics => "Show SQL syntax warnings",
        Action::FocusParameters => "Focus query parameters",
        Action::ToggleParameters => "Collapse or expand query parameters",
        Action::ToggleComment => "Comment or uncomment the rows",
        Action::OpenHistory => "Open the query history",
        Action::OpenSessions => "Show the sessions of the server",
        Action::OpenLocks => "Show the lock waits of the server",
        Action::ExplorerCommands => "Show the commands of the explorer",
        Action::ContextCommands => "Show the commands of the current panel",
        Action::EditorSave => "Save the console now",
        Action::EditorFind => "Find in the editor",
        Action::EditorReplace => "Find and replace in the editor",
        Action::EditorSuggestions => "Open the suggestions",
        Action::EditorAiComplete => "Complete with AI",
        Action::EditorAiCompleteRequest => "Complete with AI with a request",
        Action::AiExplainError => "Explain the last error with AI",
        Action::AiExplainPlan => "Explain the plan with AI",
        Action::EditorAiAccept => "Accept the AI proposal",
        Action::EditorAiReject => "Reject the AI proposal",
        Action::EditorAiCancel => "Cancel AI completion",
        Action::EditorUndo => "Undo the last edit",
        Action::EditorRedo => "Redo the last edit",
        Action::EditorSelectAll => "Select all the text",
        Action::EditorCopy => "Copy the selected text",
        Action::EditorCut => "Cut the selected text",
        Action::EditorPaste => "Paste into the editor",
        Action::Help => "Open this screen",
        Action::OpenPalette => "Open the commands",
        Action::CloseTab => "Close the tab",
        Action::Quit => "Quit",
        Action::NextTab => "Next tab",
        Action::PreviousTab => "Previous tab",
        Action::ToggleNavigator => "Hide or show the navigator",
        Action::ChooseSchema => "Choose the schema or database",
        Action::FocusLeft => "Go to the left panel",
        Action::FocusRight => "Go to the right panel",
        Action::FocusUp => "Go to the editor",
        Action::FocusDown => "Go to the results",
        Action::NavigatorUp => "Previous row",
        Action::NavigatorDown => "Next row",
        Action::NavigatorCollapse => "Close, or go to the parent",
        Action::NavigatorExpand => "Open",
        Action::NavigatorOpen => "Open a console or a table, or open the row",
        Action::NavigatorNewConsole => "New console",
        Action::NavigatorRefresh => "Read the catalog again",
        Action::NavigatorDisconnect => "Disconnect",
        Action::NavigatorEdit => "Edit the connection",
        Action::NavigatorDelete => "Remove the connection",
        Action::NavigatorAdd => "Add a connection",
        Action::RenameConsole => "Rename the console",
        Action::ChangeConnection => "Change the connection of the console",
        Action::OpenSettings => "Open the settings",
        Action::OpenSqlFile => "Open a SQL file as a console",
        Action::GridExport => "Export the result to a file",
        Action::GridImport => "Import a file to this table",
        Action::AutoRefresh => "Auto-refresh of the current tab",
        Action::GridEditCell => "Edit the selected cell",
        Action::GridSetNull => "Set the selected cell to NULL",
        Action::GridNewRow => "Add a row",
        Action::GridDeleteRow => "Mark for delete",
        Action::GridReviewEdits => "Review pending table changes",
        Action::GridSaveEdits => "Review and apply pending table changes",
        Action::GridDiscardEdits => "Discard pending table changes",
        Action::NavigatorImport => "Import a file to the table of the row",
        Action::NavigatorScripts => "Scripts without a connection",
        Action::NavigatorDeletePassword => "Delete the saved password",
        Action::NavigatorStructure => "Show the structure of the row",
        Action::NavigatorSessions => "Show the sessions of the server",
        Action::NavigatorLocks => "Show the lock waits of the server",
        Action::NavigatorFilter => "Filter the tables of the explorer",
        Action::NavigatorClearFilter => "Clear the filter of the explorer",
        Action::NavigatorGenerate => "Generate SQL or the DDL of the row",
        Action::NavigatorCopyName => "Copy the name of the row",
        Action::NavigatorMoveUp => "Move the connection or folder up",
        Action::NavigatorMoveDown => "Move the connection or folder down",
        Action::NavigatorMoveToFolder => "Move the connection to a folder",
        Action::GridStructure => "Show the structure of the table",
        Action::GridReload => "Read the grid again",
        Action::GridCancelQuery => "Cancel the query of the session",
        Action::GridEndSession => "End the session",
        Action::GridFollowKey | Action::FollowKey => "Follow a key of the row",
        Action::ReloadConfig => "Load the configuration again",
        Action::GridUp => "Up",
        Action::GridDown => "Down",
        Action::GridLeft => "Left",
        Action::GridRight => "Right",
        Action::GridPageUp => "Previous page",
        Action::GridPageDown => "Next page",
        Action::GridFirstRow => "First row",
        Action::GridLastRow => "Last row",
        Action::GridFirstColumn => "First column",
        Action::GridLastColumn => "Last column",
        Action::GridSelectAll => "Select all",
        Action::GridToggleSelection => "Start or end the selection",
        Action::GridClearSelection => "Clear the selection",
        Action::GridCopy => "Copy as TSV",
        Action::GridOpenValue => "Open the value",
        Action::GridOpenRow => "Open the row",
        Action::GridProfile => "Show the column profile",
        Action::GridChart => "Show a text chart",
        Action::GridComparePrevious => "Compare with another result tab",
        Action::GridCountRows => "Count the rows",
        Action::GridFind => "Find in the loaded rows",
        Action::GridNarrow => "Make the column narrow",
        Action::GridWiden => "Make the column wide",
        Action::GridAutoWidth => "Use the automatic column width",
        Action::GridCloseResult => "Close the result",
        Action::GridPreviousResult => "Previous result",
        Action::GridNextResult => "Next result",
        Action::GridApply => "Apply the WHERE and ORDER BY fields",
        Action::GridClearFilters => "Clear the filters and reload the table",
        Action::GridFocusFilter => "Edit the WHERE filter",
        Action::GridFocusOrder => "Edit the ORDER BY text",
        Action::GridSortColumn => "Sort by the column",
        Action::GridLoadMore => "Load more rows",
        Action::NavigatorDiagram => "Show the ER diagram of the row",
        Action::OpenDiagram => "Show the ER diagram of the tab or the row",
        Action::OpenCompare => "Compare the tab or the row with another source",
        Action::DiagramUp => "Select the table above",
        Action::DiagramDown => "Select the table below",
        Action::DiagramLeft => "Select the table on the left",
        Action::DiagramRight => "Select the table on the right",
        Action::DiagramPanUp => "Move the view up",
        Action::DiagramPanDown => "Move the view down",
        Action::DiagramPanLeft => "Move the view to the left",
        Action::DiagramPanRight => "Move the view to the right",
        Action::DiagramPageUp => "Move the view up one page",
        Action::DiagramPageDown => "Move the view down one page",
        Action::DiagramOpen => "Open the data of the table",
        Action::DiagramStructure => "Show the structure of the table",
        Action::DiagramCopy => "Copy the diagram as Mermaid text",
        Action::DiagramReload => "Read the diagram again",
        Action::DiagramKeysOnly => "Show only the key columns, or all",
        Action::ReviewAccept => "Accept the current change",
        Action::ReviewReject => "Reject the current change",
        Action::ReviewAcceptAll => "Accept all changes",
        Action::ReviewRejectAll => "Reject all changes",
        Action::ReviewNext => "Go to the next change",
        Action::ReviewPrevious => "Go to the previous change",
        Action::ReviewClose => "Close the review",
        Action::NavigatorCompare => {
            "Compare the structure or the data of the row with another source"
        }
        Action::CompareScript => "Open the sync script in a new console",
        Action::CompareCopy => "Copy the sync script",
    }
}

/// Writes the name of an action in the command list.
pub fn command_text(action: Action) -> &'static str {
    match action {
        Action::Help => "Show the keys",
        action => action_text(action),
    }
}

/// Keeps panel command names short enough for the minimum terminal.
pub fn panel_command_text(action: Action) -> &'static str {
    use Action::*;
    match action {
        EditorSave => "Save now",
        EditorFind => "Find",
        EditorReplace => "Replace",
        EditorSuggestions => "Suggestions",
        EditorUndo => "Undo",
        EditorRedo => "Redo",
        EditorSelectAll | GridSelectAll => "Select all",
        EditorCopy | GridCopy => "Copy",
        EditorCut => "Cut",
        EditorPaste => "Paste",
        GridToggleSelection => "Start/end selection",
        GridClearSelection => "Clear selection",
        GridOpenValue => "Open value",
        GridOpenRow => "Open row",
        GridProfile => "Column profile",
        GridChart => "Text chart",
        GridComparePrevious => "Compare results",
        GridFind => "Find",
        GridNarrow => "Narrow column",
        GridWiden => "Widen column",
        GridAutoWidth => "Automatic width",
        GridCloseResult => "Close result",
        GridPreviousResult => "Previous",
        GridNextResult => "Next",
        GridApply => "Apply filters",
        GridClearFilters => "Clear filters",
        GridFocusFilter => "WHERE filter",
        GridFocusOrder => "ORDER BY",
        GridSortColumn => "Sort column",
        GridLoadMore => "Load more",
        GridCountRows => "Count rows",
        GridExport => "Export",
        GridImport => "Import",
        GridEditCell => "Edit cell",
        GridSetNull => "Set NULL",
        GridReviewEdits => "Review changes",
        GridSaveEdits => "Apply changes",
        GridDiscardEdits => "Discard changes",
        GridStructure => "Structure",
        GridReload | DiagramReload => "Reload",
        GridFollowKey => "Follow key",
        GridCancelQuery => "Cancel query",
        GridEndSession => "End session",
        CompareScript => "Open sync script",
        CompareCopy => "Copy sync script",
        DiagramOpen => "Open data",
        DiagramStructure => "Structure",
        DiagramCopy => "Copy Mermaid",
        DiagramKeysOnly => "Key/all columns",
        ReviewAccept => "Accept change",
        ReviewReject => "Reject change",
        ReviewAcceptAll => "Accept all",
        ReviewRejectAll => "Reject all",
        ReviewNext => "Next change",
        ReviewPrevious => "Previous change",
        ReviewClose => "Close review",
        _ => action_text(action),
    }
}

/// Adds technical search terms without changing the visible command name.
pub fn command_aliases(action: Action) -> &'static str {
    match action {
        Action::Explain | Action::ExplainAnalyze => "explain query plan analyze",
        Action::EditorFind | Action::EditorReplace | Action::GridFind => "search find replace",
        Action::EditorSuggestions => "autocomplete completion suggestions",
        Action::GridCopy => "copy cell selection clipboard tsv",
        Action::GridProfile => "profile column null distinct min max histogram statistics",
        Action::GridChart => "chart graph bar line numeric",
        Action::GridComparePrevious => "diff compare result tab execution snapshot",
        Action::NavigatorRefresh => "refresh reload catalog rename console",
        Action::GridFocusFilter => "where filter",
        Action::GridFocusOrder | Action::GridSortColumn => "order by sort",
        Action::NavigatorGenerate => "sql ddl create select insert update delete",
        Action::DiagramCopy => "mermaid diagram export",
        Action::GridReload | Action::DiagramReload => "refresh reload",
        _ => "",
    }
}

/// Writes one row of the help screen.
pub fn keys_row(keys: &str, action: Action) -> String {
    format!("{keys:<24}{}", action_text(action))
}

/// Writes a row of the list of keys of a table. The result keys move between the
/// pages of the table.
pub fn table_keys_row(keys: &str, action: Action) -> String {
    format!("{keys:<24}{}", grid_action_text(action, true, false))
}

/// Writes a row of the list of keys of a structure with sections. The result keys
/// move between the sections.
pub fn section_keys_row(keys: &str, action: Action) -> String {
    format!("{keys:<24}{}", grid_action_text(action, true, true))
}

/// Names the page keys for the grid that receives them.
pub fn grid_action_text(action: Action, table: bool, sections: bool) -> &'static str {
    match (table, sections, action) {
        (true, true, Action::GridPreviousResult) => "Previous section",
        (true, true, Action::GridNextResult) => "Next section",
        (true, false, Action::GridPreviousResult) => "Previous page of rows",
        (true, false, Action::GridNextResult) => "Next page of rows",
        _ => action_text(action),
    }
}

/// Writes the question before a signal. An open transaction of that session rolls back.
/// `blocking` names the session that holds a lock, in the tab of the lock waits, so the
/// user knows which side of the wait receives the signal.
pub fn session_signal_body(
    signal: SessionSignal,
    session: u64,
    user: &str,
    query: &str,
    blocking: bool,
) -> String {
    let effect = match signal {
        SessionSignal::CancelQuery => "The query stops, and the session stays open.",
        SessionSignal::EndSession => {
            "The session closes, and the database undoes its open transaction."
        }
    };
    let role = if blocking {
        "Blocking session"
    } else {
        "Session"
    };
    format!("{role} {session} of {user}.\n{effect}\n\n{query}")
}

/// Returns the name of an access level, as the options of the access request show it.
pub fn agent_access_name(access: crate::store::config::AgentAccess) -> &'static str {
    use crate::store::config::AgentAccess;
    match access {
        AgentAccess::Propose => AGENT_ALLOW,
        AgentAccess::Direct => AGENT_ALLOW_ALL,
        AgentAccess::Full => AGENT_FULL,
    }
}
/// Returns the name of a default access in the settings.
pub fn agent_default_name(value: crate::store::config::AgentDefault) -> &'static str {
    value.access().map_or(AGENT_ASK, agent_access_name)
}
pub fn agent_endpoint(error: &std::io::Error) -> String {
    format!(
        "Agent endpoint unavailable: {}. Check the private runtime directory.",
        crate::grid::printable(&error.to_string())
    )
}
