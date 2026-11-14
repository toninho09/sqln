//! The query history: one JSON line for each statement, in the global folder.
//! Several instances use this file, so a write only adds text at its end. A write
//! opens the file again, because `trim` replaces the file under the other instances.

use std::fs;
use std::io::{self, Write};

use serde::{Deserialize, Serialize};

use super::Root;
use super::file::{self, Expect};

const MEBIBYTE: usize = 1024 * 1024;
/// A window lists at most this number of statements.
pub const LOAD_LIMIT: usize = 1000;
/// A longer statement stays out of the history.
pub const STATEMENT_LIMIT: usize = 64 * 1024;
/// A larger file goes down to `KEPT_BYTES` at the next start.
const FILE_LIMIT: usize = 8 * MEBIBYTE;
const KEPT_BYTES: usize = 4 * MEBIBYTE;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// The seconds since the Unix epoch.
    pub at: u64,
    /// The stable ID of the connection.
    pub connection: String,
    pub ok: bool,
    pub sql: String,
}

/// Adds the line of an entry to `lines`.
pub fn push_line(lines: &mut Vec<u8>, entry: &Entry) {
    // A plain struct of text and numbers always has a JSON form.
    if serde_json::to_writer(&mut *lines, entry).is_ok() {
        lines.push(b'\n');
    }
}

/// Writes whole lines at the end of the file, in one write.
pub fn append(root: &Root, lines: &[u8]) -> io::Result<()> {
    file::open_private_append(&root.history_file())?.write_all(lines)
}

/// Returns the entries of a connection, the newest first. A line that does not read
/// stays out, such as the rest of a write that a crash stopped.
pub fn load(root: &Root, connection: &str, limit: usize) -> Vec<Entry> {
    let Ok(bytes) = fs::read(root.history_file()) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&bytes)
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<Entry>(line).ok())
        .filter(|entry| entry.connection == connection)
        .take(limit)
        .collect()
}

/// Keeps the newest lines of a file above the limit. Another instance can hold the
/// lock, and then this start leaves the file as it is.
pub fn trim(root: &Root) -> io::Result<()> {
    let path = root.history_file();
    let size = fs::metadata(&path).map_or(0, |metadata| metadata.len());
    if size <= FILE_LIMIT as u64 {
        return Ok(());
    }
    let Some(_lock) = file::try_lock(&root.locks_dir().join("history.lock"))? else {
        return Ok(());
    };
    let bytes = fs::read(&path)?;
    let start = bytes.len().saturating_sub(KEPT_BYTES);
    let line = bytes[start..].iter().position(|byte| *byte == b'\n');
    let start = start + line.map_or(0, |at| at + 1);
    file::write_private_atomic(&path, &bytes[start..], Expect::Any)
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(())
}
