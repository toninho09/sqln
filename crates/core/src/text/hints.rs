//! Hint bars, key labels, and key help.

use super::{HintPart, plural};

pub const HINT_FILTER: &[HintPart] = &[
    ("Enter", "apply"),
    ("Tab", "accept or next field"),
    ("Esc", "grid"),
    ("Ctrl+Space", "suggestions"),
];
pub const HINT_TREE_FILTER: &[HintPart] = &[
    ("Enter", "open table"),
    ("Up/Down", "move"),
    ("Tab", "tree"),
    ("Esc", "cancel"),
];
pub const HINT_GRID_SEARCH: &[HintPart] =
    &[("Enter", "next"), ("Up/Down", "move"), ("Esc", "close")];
/// The effect of `Enter` and of `Tab` when the key does not accept a suggestion.
pub const LIST_EDITOR_KEYS: (HintPart, HintPart) = (("Enter", "new line"), ("Tab", "indent"));
pub const LIST_FILTER_KEYS: (HintPart, HintPart) = (("Enter", "apply"), ("Tab", "next field"));

pub const HINT_SEARCH: &[HintPart] = &[
    ("Tab", "next field"),
    ("Enter", "next or replace"),
    ("Ctrl+A", "replace all"),
    ("Esc", "close"),
];
pub const HINT_PICKER: &[HintPart] = &[
    ("Up/Down", "move"),
    ("Enter", "choose"),
    ("r", "rename"),
    ("Del", "remove a script"),
    ("Esc", "cancel"),
];
pub const HINT_CHOOSE: &[HintPart] = &[("Up/Down", "move"), ("Enter", "choose"), ("Esc", "cancel")];
pub const HINT_NAME: &[HintPart] = &[("Enter", "saves"), ("Esc", "cancels")];
pub const HINT_SETTINGS_TEXT: &[HintPart] = &[
    ("Tab", "next"),
    ("Enter", "next"),
    ("Esc", "cancel"),
    ("", "An empty field removes the value"),
];
pub const HINT_THEME_LIST: &[HintPart] = &[
    ("Up/Down", "move"),
    ("Enter", "edit"),
    ("Tab", "next"),
    ("Esc", "cancel"),
];
pub const HINT_THEME_COLORS: &[HintPart] = &[
    ("Up/Down", "color"),
    ("Tab", "next"),
    ("Esc", "discard the draft"),
];
pub const HINT_SETTINGS_SECTIONS: &[HintPart] =
    &[("Up/Down", "section"), ("Tab", "next"), ("Esc", "cancel")];
pub const HINT_SUGGESTIONS: &[HintPart] = &[("Ctrl+Space", "suggestions")];
pub const HINT_FIND: &[HintPart] = &[("Ctrl+F", "find")];
pub const HINT_EDITOR_STAYS: &[HintPart] = &[("", "The editor stays available")];
pub const HINT_CONFLICTS: &[HintPart] = &[
    ("Up/Down", "move"),
    ("Left/Right", "choose"),
    ("Enter", "next"),
    ("Esc", "decide later"),
];
/// The application adds the key that reads the file again.
pub const HINT_CONFIG_ERROR: &[HintPart] = &[("", "Configuration error. Fix the file, then")];
pub const HINT_FORM: &[HintPart] = &[
    ("Tab", "next"),
    ("Left/Right", "change"),
    ("Enter", "next"),
    ("Esc", "cancel"),
];
pub const HINT_FORM_TEXT: &[HintPart] = &[("Tab", "next"), ("Enter", "next"), ("Esc", "cancel")];
pub const HINT_FORM_LIST: &[HintPart] = &[
    ("", "Write to filter"),
    ("Up/Down", "move"),
    ("Enter", "choose"),
    ("Esc", "close list"),
];
pub const HINT_FORM_BUTTON: &[HintPart] = &[("Tab", "next"), ("Enter", "press"), ("Esc", "cancel")];
pub const HINT_FORM_SCHEMAS: &[HintPart] = &[
    ("Tab", "next"),
    ("Ctrl+O", "list"),
    ("Enter", "next"),
    ("Esc", "cancel"),
];
pub const HINT_MARK_LIST: &[HintPart] = &[
    ("", "Write to filter"),
    ("Up/Down", "move"),
    ("Space", "mark"),
    ("Enter", "done"),
    ("Esc", "cancel"),
];
pub const HINT_FORM_PATH: &[HintPart] = &[
    ("Tab", "next"),
    ("Ctrl+O", "browse"),
    ("Enter", "next"),
    ("Esc", "cancel"),
];
pub const HINT_BROWSER: &[HintPart] = &[
    ("", "Write to filter"),
    ("Up/Down", "move"),
    ("Enter", "open"),
    ("Left", "up"),
    ("~", "home"),
    (".", "hidden"),
    ("Esc", "cancel"),
];
pub const HINT_EXPORT_FORMATS: &[HintPart] = &[
    ("Up/Down", "format"),
    ("Tab", "next"),
    ("Enter", "next"),
    ("Esc", "cancel"),
];
pub const HINT_IMPORT_SECTIONS: &[HintPart] = &[
    ("Up/Down", "section"),
    ("Tab", "next"),
    ("Enter", "next"),
    ("Esc", "cancel"),
];
pub const HINT_IMPORT_COLUMNS: &[HintPart] = &[
    ("Up/Down", "column"),
    ("Left/Right", "file column"),
    ("Tab", "next"),
    ("Esc", "cancel"),
];
pub const HINT_IMPORT_PREVIEW: &[HintPart] = &[
    ("Up/Down", "rows"),
    ("Left/Right", "columns"),
    ("Tab", "next"),
    ("Esc", "cancel"),
];
pub const HINT_DIALOG: &[HintPart] = &[
    ("Tab", "next option"),
    ("Enter", "confirm"),
    ("Esc", "cancel"),
];
pub const HINT_HELP: &[HintPart] = &[
    ("Type", "search"),
    ("Tab", "page"),
    ("Up/Down", "scroll"),
    ("PgUp/PgDn", "page"),
    ("Esc", "close"),
];
pub const HELP_SEARCH: &str = "Type a command or shortcut to filter";
pub const HELP_ALL: &str = "All keys";
pub const HELP_GUIDE_SEARCH: &str = "Type a word to filter the guide";

pub const HINT_OVERLAY_SEARCH: &[HintPart] = &[
    ("Enter", "next"),
    ("Shift+Enter", "previous"),
    ("Ctrl+C", "copy"),
    ("Esc", "close find"),
];
pub const HINT_OVERLAY: &[HintPart] = &[
    ("Ctrl+F", "find"),
    ("Up/Down", "scroll"),
    ("Shift+arrows", "select"),
    ("Ctrl+C", "copy"),
    ("Esc", "close"),
];
pub const HINT_OVERLAY_ROW: &[HintPart] = &[
    ("Ctrl+F", "find"),
    ("Left/Right", "row"),
    ("Up/Down", "scroll"),
    ("Shift+arrows", "select"),
    ("Ctrl+C", "copy"),
    ("Esc", "close"),
];
pub const HINT_OVERLAY_JSON: &[HintPart] = &[
    ("Ctrl+F", "find"),
    ("f", "switch form"),
    ("Up/Down", "scroll"),
    ("Shift+arrows", "select"),
    ("Ctrl+C", "copy"),
    ("Esc", "close"),
];

/// Writes a key of the key map for the hint bar: `ctrl+w e` becomes `Ctrl+W e`.
pub fn key_label(key: &str) -> String {
    let token = |token: &str| {
        let modified = token.len() > 1 && token.contains('+');
        let parts: Vec<String> = token
            .split('+')
            .map(|part| match part {
                "delete" => "Del".to_owned(),
                // A letter holds its case alone, because `n` and `N` are two keys.
                _ if part.chars().count() == 1 && modified => part.to_uppercase(),
                _ if part.chars().count() == 1 => part.to_owned(),
                _ => {
                    let mut chars = part.chars();
                    chars.next().map_or_else(String::new, |first| {
                        first.to_uppercase().chain(chars).collect::<String>()
                    })
                }
            })
            .collect();
        parts.join("+")
    };
    key.split_whitespace()
        .map(token)
        .collect::<Vec<_>>()
        .join(" ")
}

pub const HINT_PREFIX: &[HintPart] = &[("Ctrl+W", "…"), ("Tab", "explorer"), ("Esc", "cancel")];
pub const HINT_SEQUENCE: &[HintPart] = &[("", "Press the next key"), ("Esc", "cancel")];
pub const HINT_RECORD_KEYS: &[HintPart] = &[
    ("Enter", "confirm"),
    ("OK", "confirm"),
    ("Esc", "cancel recording"),
];
pub const HINT_RECORD_MOUSE: &[HintPart] = &[("OK", "confirm"), ("Esc", "cancel recording")];
pub const HINT_EXPLORER_MENU: &[HintPart] = &[("Ctrl+W Tab", "…"), ("Esc", "cancel")];
pub const HINT_COMMANDS: &[HintPart] = &[("Ctrl+W", "commands")];
pub const HINT_PREFIX_MENU: &[HintPart] = &[("Ctrl+W", "menu")];
pub const HINT_PALETTE: &[HintPart] = &[
    ("", "Write to filter"),
    ("Up/Down", "move"),
    ("Enter", "run"),
    ("Esc", "cancel"),
];

pub const HINT_HISTORY: &[HintPart] = &[
    ("", "Write to filter"),
    ("Up/Down", "move"),
    ("Enter", "insert"),
    ("Esc", "cancel"),
];
pub const TITLE_KEYS: &str = "Help";

pub const FILTER_KEYS: &[(&str, &str)] = &[
    ("Enter", "Apply the filters"),
    ("Tab", "Accept the suggestion, or the next field"),
    ("Shift+Tab", "Next field"),
    ("Ctrl+Space", "Suggestions"),
    ("Esc", "Close the list, or go to the grid"),
];

pub const EDITOR_KEYS: &[(&str, &str)] =
    &[("Tab", "Accept the suggestion"), ("Esc", "Close the list")];
pub const HINT_PARAMETERS: &[HintPart] = &[
    ("Tab", "field"),
    ("↑/↓", "parameter"),
    ("Enter", "set"),
    ("Esc", "editor"),
];

pub fn keys_title(mode: &str, region: &str) -> String {
    format!(" {TITLE_KEYS} · {mode} · {region} ")
}

pub fn help_footer(count: usize, noun: &str, first: usize, last: usize, total: usize) -> String {
    format!(" {} · {first}–{last}/{total} ", plural(count, noun))
}
