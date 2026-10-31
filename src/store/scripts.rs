//! The scripts of the consoles. A console has a `<id>.sql` file with its text and a
//! `<id>.json` file with its identity, its name, and its connection.
//!
//! The scope of a connection decides where its consoles live.

use std::borrow::Cow;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::file::{self, Expect, FileLock, WriteError};
use super::{Root, Scope};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptMeta {
    pub version: u32,
    pub id: String,
    pub name: String,
    pub connection_id: String,
    pub connection_scope: Scope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
}

impl ScriptMeta {
    pub fn new(id: String, name: String, connection_id: String, scope: Scope) -> Self {
        Self {
            version: VERSION,
            id,
            name,
            connection_id,
            connection_scope: scope,
            schema: None,
        }
    }
}

/// A script on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptEntry {
    pub meta: ScriptMeta,
    pub scope: Scope,
    /// The last change of the text.
    pub modified: Option<SystemTime>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    Crlf,
}

/// Returns the text with `\n` line ends, and the line end that the file used.
pub fn decode(text: &str) -> (String, LineEnding) {
    if text.contains("\r\n") {
        (text.replace("\r\n", "\n"), LineEnding::Crlf)
    } else {
        (text.to_owned(), LineEnding::Lf)
    }
}

pub fn encode(text: &str, ending: LineEnding) -> Cow<'_, str> {
    match ending {
        LineEnding::Lf => Cow::Borrowed(text),
        LineEnding::Crlf => Cow::Owned(text.replace('\n', "\r\n")),
    }
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn sql_file(root: &Root, id: &str) -> PathBuf {
    root.consoles_dir().join(format!("{id}.sql"))
}

fn meta_file(root: &Root, id: &str) -> PathBuf {
    root.consoles_dir().join(format!("{id}.json"))
}

/// Returns true when the text can be the name of a file of a script.
fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Lists the scripts of a folder, the newest first. A file that cannot be read stays out.
pub fn list(root: &Root, scope: Scope) -> Vec<ScriptEntry> {
    let Ok(entries) = fs::read_dir(root.consoles_dir()) else {
        return Vec::new();
    };
    let mut scripts: Vec<ScriptEntry> = entries
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter_map(|entry| {
            let meta: ScriptMeta = serde_json::from_slice(&fs::read(entry.path()).ok()?).ok()?;
            let stem = entry.path().file_stem()?.to_str()?.to_owned();
            if meta.id != stem || !is_safe_id(&stem) {
                return None;
            }
            let modified = [sql_file(root, &meta.id), entry.path()]
                .iter()
                .find_map(|path| fs::metadata(path).and_then(|meta| meta.modified()).ok());
            Some(ScriptEntry {
                meta,
                scope,
                modified,
            })
        })
        .collect();
    scripts.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.meta.name.cmp(&b.meta.name))
    });
    scripts
}

/// Writes the identity file of a script.
pub fn write_meta(root: &Root, meta: &ScriptMeta, expect: Expect) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(meta).map_err(|error| error.to_string())?;
    text.push('\n');
    file::write_atomic(&meta_file(root, &meta.id), text.as_bytes(), expect)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Creates the two files of a new script. It returns the hash of the text file.
pub fn create(root: &Root, meta: &ScriptMeta, text: &str) -> Result<u64, String> {
    write_meta(root, meta, Expect::Missing)?;
    file::write_atomic(&sql_file(root, &meta.id), text.as_bytes(), Expect::Missing)
        .map_err(|error| error.to_string())
}

/// Writes the text file of a script. It returns the hash of the new content.
pub fn write_text(root: &Root, id: &str, text: &str, expect: Expect) -> Result<u64, WriteError> {
    file::write_atomic(&sql_file(root, id), text.as_bytes(), expect)
}

/// Reads the text file of a script, with the hash of its bytes.
pub fn read_text(root: &Root, id: &str) -> io::Result<(String, u64)> {
    file::read(&sql_file(root, id))
}

/// Returns the size and the time of the last change of the text file.
pub fn stat(root: &Root, id: &str) -> Option<(u64, SystemTime)> {
    let meta = fs::metadata(sql_file(root, id)).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// Removes the two files of a script. A file that is already absent is not an error.
pub fn remove(root: &Root, id: &str) -> Result<(), String> {
    for path in [sql_file(root, id), meta_file(root, id)] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

/// Takes the lock of a script. It returns `None` when another process holds it.
pub fn lock(root: &Root, id: &str) -> io::Result<Option<FileLock>> {
    file::try_lock(&root.locks_dir().join(format!("{id}.lock")))
}

/// Copies a script to another folder, checks the copy, and removes the original.
/// `meta` describes the script in its new place.
pub fn move_to(from: &Root, to: &Root, meta: &ScriptMeta) -> Result<u64, String> {
    let (text, _) = read_text(from, &meta.id).unwrap_or_default();
    let hash = create(to, meta, &text)?;
    let copied = read_text(to, &meta.id).map_err(|error| error.to_string())?;
    if copied.1 != hash || copied.0 != text {
        return Err("The copy of the script does not match the original.".to_owned());
    }
    remove(from, &meta.id)?;
    Ok(hash)
}
