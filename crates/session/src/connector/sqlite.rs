//! The target of a SQLite connection: where the database lives.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::ConnectError;
use crate::catalog::{Columns, Relation, RelationKind};
use crate::sql::{Dialect, quote_identifier};

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

/// The schema of the database of the connection.
pub const MAIN_SCHEMA: &str = "main";

/// The names of the attached databases. `temp` holds only the temporary objects.
pub const DATABASES_SQL: &str = "SELECT name FROM pragma_database_list WHERE name <> 'temp'";

/// The tables and the views of a schema: the name and the type of each one.
pub fn relations_sql(schema: &str) -> String {
    format!(
        "SELECT name, type FROM {}.sqlite_schema \
         WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
         ORDER BY name",
        quote_identifier(schema, Dialect::Sqlite),
    )
}

/// The triggers of a schema: the name of each trigger and of its table.
pub fn triggers_sql(schema: &str) -> String {
    format!(
        "SELECT name, tbl_name FROM {}.sqlite_schema WHERE type = 'trigger'",
        quote_identifier(schema, Dialect::Sqlite),
    )
}

/// The columns of a relation. `?1` is the relation and `?2` is the schema.
pub const COLUMNS_SQL: &str = "SELECT name FROM pragma_table_info(?1, ?2)";

/// The keys that leave a relation: the number, the other table, the column of the
/// relation, and the column of the other table. A key without target columns reads the
/// primary key of its target. `?1` is the relation and `?2` is the schema.
pub const FOREIGN_KEYS_SQL: &str = "\
    SELECT f.id, f.\"table\", f.\"from\", coalesce(f.\"to\", p.name) \
    FROM pragma_foreign_key_list(?1, ?2) f \
    LEFT JOIN pragma_table_info(f.\"table\", ?2) p ON f.\"to\" IS NULL AND p.pk = f.seq + 1 \
    WHERE coalesce(f.\"to\", p.name) IS NOT NULL \
    ORDER BY f.id, f.seq";

/// The keys of other tables that reach a relation, with the shape of `FOREIGN_KEYS_SQL`:
/// the column of the relation comes before the column of the other table.
pub const REFERENCING_KEYS_SQL: &str = "\
    SELECT f.id, t.name, coalesce(f.\"to\", p.name), f.\"from\" \
    FROM pragma_table_list t \
    JOIN pragma_foreign_key_list(t.name, t.schema) f \
    LEFT JOIN pragma_table_info(?1, ?2) p ON f.\"to\" IS NULL AND p.pk = f.seq + 1 \
    WHERE t.schema = ?2 AND t.type = 'table' AND f.\"table\" = ?1 COLLATE NOCASE \
      AND coalesce(f.\"to\", p.name) IS NOT NULL \
    ORDER BY t.name, f.id, f.seq";

/// A relation of a row of `relations_sql`.
pub fn relation(name: String, kind: &str) -> Relation {
    Relation {
        name,
        kind: if kind == "view" {
            RelationKind::View
        } else {
            RelationKind::Table
        },
        columns: Columns::NotLoaded,
        foreign_keys: Vec::new(),
        referencing: Vec::new(),
        notes: Vec::new(),
    }
}
