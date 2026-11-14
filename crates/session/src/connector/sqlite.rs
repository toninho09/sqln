//! The target of a SQLite connection: where the database lives.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::ConnectError;

/// Where a SQLite database lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SqliteSource {
    File(PathBuf),
    /// One database in memory. It lives while the pool holds its only session, and a
    /// disconnect or the end of the application loses it.
    Memory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqliteTarget {
    pub source: SqliteSource,
    pub allow_create: bool,
    /// `None` keeps the default of SQLite.
    pub foreign_keys: Option<bool>,
    pub busy_timeout: Duration,
}

impl SqliteTarget {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            source: SqliteSource::File(path.into()),
            allow_create: false,
            foreign_keys: None,
            busy_timeout: Duration::from_secs(5),
        }
    }

    pub fn in_memory() -> Self {
        Self {
            source: SqliteSource::Memory,
            allow_create: false,
            foreign_keys: None,
            busy_timeout: Duration::from_secs(5),
        }
    }

    pub fn is_memory(&self) -> bool {
        matches!(self.source, SqliteSource::Memory)
    }

    /// The path of the file of the source. A database in memory has the `:memory:`
    /// text, which never reaches the driver.
    pub fn path(&self) -> &Path {
        match &self.source {
            SqliteSource::File(path) => path,
            SqliteSource::Memory => Path::new(":memory:"),
        }
    }

    /// True when the file of the source exists. A database in memory has no file.
    pub fn source_exists(&self) -> bool {
        matches!(&self.source, SqliteSource::File(path) if path.exists())
    }

    /// The text of the place of the source.
    pub fn file_display(&self) -> String {
        match &self.source {
            SqliteSource::File(path) => path.display().to_string(),
            SqliteSource::Memory => ":memory:".to_owned(),
        }
    }
}

/// Reads the text of a SQLite file field. `:memory:` opens one database in memory.
pub fn file_source(text: &str) -> SqliteSource {
    if text.trim().eq_ignore_ascii_case(":memory:") {
        SqliteSource::Memory
    } else {
        SqliteSource::File(PathBuf::from(text))
    }
}

/// Refuses the text forms that open a database in memory.
pub fn validate_path(path: &Path) -> Result<(), ConnectError> {
    let text = path.to_string_lossy();
    let lower = text.trim().to_lowercase();
    let in_memory = lower.is_empty()
        || lower == ":memory:"
        || lower.starts_with("file:")
        || lower.contains("mode=memory");
    if in_memory {
        return Err(ConnectError::new(format!(
            "SQLite needs a file. This path is not valid: {text}"
        )));
    }
    Ok(())
}
