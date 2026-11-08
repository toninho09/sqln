//! `themes/<id>.json`: the custom color themes of one scope. A file holds one
//! palette. A damaged file gives a note, and the other files load.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::Root;
use super::file::{self, Expect};

pub const VERSION: u32 = 1;

/// The ids that the built-in themes use, so no file can take them.
pub const RESERVED: [&str; 8] = [
    "auto",
    "astrodark",
    "ansi",
    "tokyo-night",
    "catppuccin-mocha",
    "catppuccin-latte",
    "gruvbox",
    "nord",
];

/// A color of a theme file, written as `#RRGGBB`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hex(pub u32);

impl Serialize for Hex {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.text())
    }
}

impl<'de> Deserialize<'de> for Hex {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        parse_hex(&text)
            .map(Hex)
            .ok_or_else(|| serde::de::Error::custom("A color needs the form #RRGGBB."))
    }
}

impl Hex {
    pub fn text(self) -> String {
        format!("#{:06x}", self.0)
    }
}

/// Reads `#RRGGBB`, with or without the mark.
pub fn parse_hex(text: &str) -> Option<u32> {
    let text = text.trim().strip_prefix('#').unwrap_or(text.trim());
    if text.len() != 6 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(text, 16).ok()
}

/// The palette of a theme file. Each field mirrors one field of `Scheme`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Colors {
    pub red: Hex,
    pub orange: Hex,
    pub green: Hex,
    pub cyan: Hex,
    pub blue: Hex,
    pub purple: Hex,
    pub text: Hex,
    pub comment: Hex,
    pub ui_red: Hex,
    pub ui_orange: Hex,
    pub ui_yellow: Hex,
    pub ui_green: Hex,
    pub ui_cyan: Hex,
    pub ui_purple: Hex,
    pub accent: Hex,
    pub base: Hex,
    pub panel: Hex,
    pub float: Hex,
    pub deep: Hex,
    pub label_text: Hex,
    pub border: Hex,
    pub current_line: Hex,
    pub selection: Hex,
    pub highlight: Hex,
    pub prompt: Hex,
    pub ui_text: Hex,
    pub inactive: Hex,
    pub winbar: Hex,
    pub strong: Hex,
    pub search: Hex,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ThemeFile {
    pub version: u32,
    pub name: String,
    pub colors: Colors,
}

/// One theme of the folder, with the state of its file.
#[derive(Clone, Debug)]
pub struct StoredTheme {
    pub id: String,
    pub name: String,
    pub colors: Colors,
    /// The hash of the file at the load. A write needs it, so no external
    /// change goes away without a warning.
    pub hash: u64,
}

/// Reads the themes of a scope. A file that cannot load gives a note.
pub fn load(root: &Root) -> (Vec<StoredTheme>, Vec<String>) {
    let mut themes = Vec::new();
    let mut notes = Vec::new();
    let Ok(entries) = fs::read_dir(root.themes_dir()) else {
        return (themes, notes);
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        let Some(id) = path.file_stem().and_then(|id| id.to_str()) else {
            continue;
        };
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let label = path
            .file_name()
            .map_or(id.to_owned(), |name| name.to_string_lossy().into_owned());
        // File names are external text; remove terminal controls from diagnostics.
        let label: String = label.chars().filter(|ch| !ch.is_control()).collect();
        let mut note = |message: String| notes.push(format!("themes/{label}: {message}"));
        if !valid_id(id) {
            note("The name is not a valid theme id.".to_owned());
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                note("The file cannot be read.".to_owned());
                continue;
            }
        };
        match serde_json::from_slice::<ThemeFile>(&bytes) {
            Err(_) => note("The file is not a valid theme.".to_owned()),
            Ok(file) if file.version != VERSION => note(format!("The version must be {VERSION}.")),
            Ok(file) if file.name.trim().is_empty() => note("The theme needs a name.".to_owned()),
            Ok(file) => themes.push(StoredTheme {
                id: id.to_owned(),
                name: file.name,
                colors: file.colors,
                hash: file::hash(&bytes),
            }),
        }
    }
    (themes, notes)
}

const INVALID_ID: &str = "The name gives an id of lowercase letters, numbers, and dashes.";

/// Writes one theme. `known` is the hash of the file at its load, and `None`
/// expects a new file. It returns the hash of the new file.
pub fn save(
    root: &Root,
    id: &str,
    name: &str,
    colors: &Colors,
    known: Option<u64>,
) -> Result<u64, String> {
    if !valid_id(id) {
        return Err(INVALID_ID.to_owned());
    }
    if name.trim().is_empty() {
        return Err("The theme needs a name.".to_owned());
    }
    let _lock =
        file::lock(&root.locks_dir().join("themes.lock")).map_err(|error| error.to_string())?;
    let file = ThemeFile {
        version: VERSION,
        name: name.trim().to_owned(),
        colors: *colors,
    };
    let mut text = serde_json::to_string_pretty(&file).map_err(|error| error.to_string())?;
    text.push('\n');
    let path = root.themes_dir().join(format!("{id}.json"));
    let expect = known.map_or(Expect::Missing, Expect::Hash);
    file::write_private_atomic(&path, text.as_bytes(), expect).map_err(|error| error.to_string())
}

/// Removes the file of a theme that still holds the hash of its load. A file
/// that another program removed is already gone, so the removal works.
pub fn remove(root: &Root, id: &str, known: u64) -> Result<(), String> {
    if !valid_id(id) {
        return Err(INVALID_ID.to_owned());
    }
    let _lock =
        file::lock(&root.locks_dir().join("themes.lock")).map_err(|error| error.to_string())?;
    let path = root.themes_dir().join(format!("{id}.json"));
    match fs::read(&path) {
        Err(read) if read.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("The theme file cannot be read.".to_owned()),
        Ok(bytes) if file::hash(&bytes) == known => {
            fs::remove_file(&path).map_err(|error| error.to_string())
        }
        Ok(_) => Err("Another program changed the file.".to_owned()),
    }
}

/// A hash of the names and the contents of the folder, for change detection.
pub fn manifest(root: &Root) -> Option<u64> {
    let entries = fs::read_dir(root.themes_dir()).ok()?;
    let mut parts: Vec<(String, u64)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let hash = fs::read(&path)
            .ok()
            .map(|bytes| file::hash(&bytes))
            .unwrap_or(0);
        parts.push((name, hash));
    }
    parts.sort();
    let mut hasher = DefaultHasher::new();
    for (name, hash) in parts {
        name.hash(&mut hasher);
        hash.hash(&mut hasher);
    }
    Some(hasher.finish())
}

/// True for an id that a file can hold: `a-z`, `0-9`, and `-`, up to 64
/// characters, without a reserved name.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !RESERVED.contains(&id)
}

/// Builds the id of a display name: lowercase, and the other characters
/// become dashes.
pub fn slug(name: &str) -> String {
    let mut id = String::new();
    let mut dash = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            id.push(character.to_ascii_lowercase());
            dash = true;
        } else if dash && !id.ends_with('-') {
            id.push('-');
            dash = false;
        }
    }
    id.trim_matches('-').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_gives_its_id() {
        assert_eq!(slug("My Theme!"), "my-theme");
        assert_eq!(slug("  blue 2  "), "blue-2");
        assert_eq!(slug("---"), "");
    }

    #[test]
    fn an_id_needs_plain_characters() {
        assert!(valid_id("my-theme"));
        assert!(valid_id("2"));
        assert!(!valid_id(""));
        assert!(!valid_id("My Theme"));
        assert!(!valid_id("gruvbox.json"));
        assert!(!valid_id("gruvbox"));
        assert!(!valid_id(&"a".repeat(65)));
    }

    #[test]
    fn a_color_reads_its_text() {
        assert_eq!(parse_hex("#FF838B"), Some(0xFF838B));
        assert_eq!(parse_hex("ff838b"), Some(0xFF838B));
        assert_eq!(parse_hex("#838"), None);
        assert_eq!(parse_hex("zzzzzz"), None);
    }
}
