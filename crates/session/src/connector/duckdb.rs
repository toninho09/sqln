//! The target of a DuckDB connection: where the database lives.

use std::path::{Path, PathBuf};

/// Where a DuckDB database lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DuckdbSource {
    File(PathBuf),
    /// One database in memory. It lives while the pool holds its only session, and a
    /// disconnect or the end of the application loses it.
    Memory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuckdbTarget {
    pub source: DuckdbSource,
    pub allow_create: bool,
    /// The only way to make a DuckDB connection read-only. The engine applies it when the
    /// database opens, and a session cannot change it later.
    pub read_only: bool,
}

impl DuckdbTarget {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            source: DuckdbSource::File(path.into()),
            allow_create: false,
            read_only: false,
        }
    }

    pub fn in_memory() -> Self {
        Self {
            source: DuckdbSource::Memory,
            allow_create: false,
            read_only: false,
        }
    }

    pub fn is_memory(&self) -> bool {
        matches!(self.source, DuckdbSource::Memory)
    }

    /// The path of the file of the source. A database in memory has the `:memory:` text,
    /// which never reaches the driver.
    pub fn path(&self) -> &Path {
        match &self.source {
            DuckdbSource::File(path) => path,
            DuckdbSource::Memory => Path::new(":memory:"),
        }
    }

    /// True when the file of the source exists. A database in memory has no file.
    pub fn source_exists(&self) -> bool {
        matches!(&self.source, DuckdbSource::File(path) if path.exists())
    }

    /// The text of the place of the source.
    pub fn file_display(&self) -> String {
        match &self.source {
            DuckdbSource::File(path) => path.display().to_string(),
            DuckdbSource::Memory => ":memory:".to_owned(),
        }
    }
}

/// Reads the text of a DuckDB file field. An empty text and `:memory:` open one database
/// in memory.
pub fn file_source(text: &str) -> DuckdbSource {
    let text = text.trim();
    if text.is_empty() || text.eq_ignore_ascii_case(":memory:") {
        DuckdbSource::Memory
    } else {
        DuckdbSource::File(PathBuf::from(text))
    }
}
