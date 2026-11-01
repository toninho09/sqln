//! The SQL text that the navigator writes for a table or a view. A template that needs
//! values holds `<column>` and an empty `WHERE`, so the database refuses an incomplete
//! text. A `?` does not do this, because SQLite binds a missing value as `NULL`.

use crate::complete::quote_name;
use crate::sql::Dialect;

/// The rows of a generated `SELECT`.
const ROWS: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Template {
    SelectAll,
    SelectColumns,
    Insert,
    Update,
    Delete,
}

impl Template {
    pub const ALL: [Self; 5] = [
        Self::SelectAll,
        Self::SelectColumns,
        Self::Insert,
        Self::Update,
        Self::Delete,
    ];

    /// The templates of a view, which rarely accepts a write.
    pub const READS: [Self; 2] = [Self::SelectAll, Self::SelectColumns];

    pub fn needs_columns(self) -> bool {
        matches!(self, Self::SelectColumns | Self::Insert | Self::Update)
    }
}

/// Writes the name of a relation. A relation of the current schema needs no schema.
pub fn qualified_name(schema: Option<&str>, relation: &str, dialect: Dialect) -> String {
    let name = quote_name(relation, dialect, None);
    match schema {
        Some(schema) => format!("{}.{name}", quote_name(schema, dialect, None)),
        None => name,
    }
}

/// Writes one statement. `relation` is the text of `qualified_name`.
pub fn generate(
    template: Template,
    dialect: Dialect,
    relation: &str,
    columns: &[String],
) -> String {
    let names: Vec<String> = columns
        .iter()
        .map(|column| quote_name(column, dialect, None))
        .collect();
    let values: Vec<String> = columns.iter().map(|column| format!("<{column}>")).collect();
    match template {
        Template::SelectAll => format!("SELECT *\nFROM {relation}\nLIMIT {ROWS};"),
        Template::SelectColumns => {
            format!(
                "SELECT {}\nFROM {relation}\nLIMIT {ROWS};",
                names.join(", ")
            )
        }
        Template::Insert => format!(
            "INSERT INTO {relation} ({})\nVALUES ({});",
            names.join(", "),
            values.join(", ")
        ),
        Template::Update => {
            let pairs: Vec<String> = names
                .iter()
                .zip(&values)
                .map(|(name, value)| format!("{name} = {value}"))
                .collect();
            format!("UPDATE {relation}\nSET {}\nWHERE ;", pairs.join(", "))
        }
        Template::Delete => format!("DELETE FROM {relation}\nWHERE ;"),
    }
}
