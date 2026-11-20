//! The settings dialog, themes, and shortcuts.

use super::HintPart;

pub const FIELD_AI_REQUEST: &str = "Request";
pub const TITLE_SETTINGS: &str = "Settings";
pub const SETTINGS_SAVED: &str = "The settings are saved.";
pub const SECTION_GENERAL: &str = "General";
pub const SECTION_EDITOR: &str = "Editor";
pub const SECTION_RESULTS: &str = "Results";
pub const SECTION_APPEARANCE: &str = "Appearance";
pub const SECTION_SHORTCUTS: &str = "Shortcuts";
pub const SECTION_MCP: &str = "MCP";
pub const SECTION_AUTOCOMPLETE: &str = "Autocomplete";
pub const FIELD_AI_ENABLED: &str = "Enabled";
pub const FIELD_AI_PROVIDER: &str = "Provider";
pub const FIELD_AI_MODEL: &str = "Model";
pub const FIELD_AI_ENDPOINT: &str = "Endpoint";
pub const FIELD_AI_SOURCE: &str = "Key source";
pub const FIELD_AI_PATH: &str = "Key file";
pub const FIELD_AI_KEY: &str = "New vault key";
pub const FIELD_AI_CONTEXT: &str = "Context tokens";
pub const FIELD_AI_OUTPUT: &str = "Output tokens";
pub const FIELD_AI_TIMEOUT: &str = "Timeout seconds";
pub const FIELD_AI_REASONING: &str = "Reasoning effort";
pub const FIELD_AI_SIMULTANEOUS: &str = "Max simultaneous";
pub const FIELD_MCP: &str = "MCP";
pub const FIELD_MCP_DEFAULT: &str = "Default access";
pub const FIELD_MCP_MAX: &str = "Max access";
pub const FIELD_MCP_REMEMBER: &str = "Remember clients";
/// The help of the IA section. SQLN never changes the files of a client.
pub const HELP_MCP: &str = "Claude Code: claude mcp add --transport stdio --scope user sqln -- sqln mcp\nOther clients: see docs/MCP.md.";
pub const FIELD_SHORTCUT_MODE: &str = "Key mode";
pub const SHORTCUT_COMMANDS: &str = "Commands";
pub const SHORTCUT_KEYS: &str = "Shortcuts";
pub const SHORTCUT_UNASSIGNED: &str = "Not assigned";
pub const SHORTCUT_BACK: &str = "Back to commands";
pub const HINT_SHORTCUT_COMMANDS: &[HintPart] = &[
    ("Up/Down", "command"),
    ("Enter", "edit"),
    ("Tab", "next"),
    ("Esc", "cancel"),
];
pub const HINT_SHORTCUT_EDIT: &[HintPart] = &[
    ("Tab", "next"),
    ("Left/Right", "change"),
    ("Enter", "choose"),
    ("Esc", "back to commands"),
];
pub const FIELD_SHORTCUT_BINDING: &str = "Shortcut";
pub const FIELD_SHORTCUT_FINISH: &str = "Finish with";
pub const SHORTCUT_ADD: &str = "+ Add shortcut";
pub const SHORTCUT_MOUSE_OK: &str = "Mouse OK";
pub const SHORTCUT_RECORD: &str = "Record";
pub const SHORTCUT_REMOVE: &str = "Remove shortcut";
pub const SHORTCUT_RESTORE: &str = "Restore action";
pub const SHORTCUT_OK: &str = "OK";
pub const SHORTCUT_CANCEL: &str = "Cancel recording";
pub const SHORTCUT_EMPTY: &str = "Press at least one key.";
pub const SHORTCUT_FINISH_FIRST: &str = "Confirm or cancel the recording first.";
pub const SHORTCUT_RECORD_KEYS: &str =
    "Press the keys. Enter or a click on OK confirms. Esc cancels.";
pub const SHORTCUT_RECORD_MOUSE: &str =
    "Press the keys, including Enter. Click OK to confirm. Esc cancels.";
pub const SHORTCUT_RESTORED: &str = "The action uses its inherited keys. Save to apply.";
pub const FIELD_KEYBOARD: &str = "Keyboard";
pub const FIELD_REOPEN_TABS: &str = "Reopen tabs";
pub const FIELD_QUERY_HISTORY: &str = "Query history";
pub const FIELD_FORMAT_JSON: &str = "Format JSON";
pub const FIELD_SHOW_SYSTEM_COMMANDS: &str = "System commands";
pub const FIELD_BELL_AFTER: &str = "Bell seconds";
pub const FIELD_SUGGESTION_KEYS: &str = "Autocomplete";
pub const FIELD_UNDO_LIMIT: &str = "Undo (MiB)";
pub const FIELD_PAGE_SIZE: &str = "Page size";
pub const FIELD_ROW_CAP: &str = "Row cap";
pub const FIELD_CAP_IN_SQL: &str = "Limit in SQL";
pub const FIELD_BUDGET: &str = "Data (MiB)";
pub const FIELD_THEME: &str = "Default theme";
pub const FIELD_EXPLORER_SIDE: &str = "Explorer side";
pub const FIELD_CONNECTION_THEME: &str = "Theme";
pub const FIELD_CONNECTION_ICON: &str = "Icon";
pub const FIELD_THEMES: &str = "Custom themes";
pub const FIELD_THEME_NAME: &str = "Name";
pub const FIELD_THEME_BASE: &str = "Base";
pub const BUTTON_THEME_NEW: &str = "New theme";
pub const BUTTON_THEME_EDIT: &str = "Edit";
pub const BUTTON_THEME_DELETE: &str = "Delete";
pub const BUTTON_THEME_DONE: &str = "Done";
pub const THEME_NONE: &str = "No custom themes.";
pub const THEME_EDITOR_OPEN: &str = "Close the theme editor before saving.";
pub const THEME_IN_USE: &str =
    "The theme is in use. Change the default theme or the theme of a connection first.";
pub const THEME_ID_TAKEN: &str = "A theme with this id already exists.";
pub const THEME_INVALID_ID: &str =
    "The name gives an id of lowercase letters, numbers, and dashes.";
pub const HELP_THEMES: &str =
    "Each theme is one file of the themes folder. New theme copies the base palette.";

/// The warning of a color without the form `#RRGGBB`.
pub fn theme_hex_invalid(role: &str) -> String {
    format!("The color of {role} needs the form #RRGGBB.")
}

/// The warning of a palette with low contrast.
pub fn theme_contrast(count: usize) -> String {
    format!("Low contrast in {count} places. The text can be hard to read on some terminals.")
}

pub fn settings_number(name: &str) -> String {
    format!("{name} needs a number of 1 or more.")
}
pub fn settings_seconds(name: &str) -> String {
    format!("{name} needs a number of seconds. The value 0 stops the bell.")
}

pub fn from_global(value: &str) -> String {
    format!("from global: {value}")
}
