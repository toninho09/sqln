//! Writes the `INSERT` of a batch. A file is untrusted text, so each literal must keep
//! its meaning under any setting of the session.

use crate::clipboard::base64_bytes;
use crate::sql::literal::{binary_literal, bytes_literal, text_literal};
use crate::sql::{Dialect, quote_identifier};
use crate::ui::text;

/// The size of a statement, without its first row. MySQL refuses a statement above
/// `max_allowed_packet`, and its smallest default is 4 MiB.
pub const MAX_STATEMENT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetColumn {
    pub name: String,
    pub binary: bool,
    /// The column of the file that gives the values.
    pub source: usize,
}

/// The table of an import and the columns that receive values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// The name of the table, as the statement writes it.
    pub table: String,
    pub columns: Vec<TargetColumn>,
    pub dialect: Dialect,
    /// True when a file writes binary as Base64, as a JSON export does.
    pub base64: bool,
}

/// Returns true for the type of a column that holds bytes.
pub fn is_binary_type(name: &str) -> bool {
    let name = name.to_lowercase();
    name == "bytea" || name.contains("blob") || name.contains("binary")
}

/// Returns the digits of the text that a CSV export writes for binary.
fn hex_digits(value: &str) -> Option<&str> {
    let digits = value.strip_prefix("0x")?;
    let valid = digits.len().is_multiple_of(2) && digits.bytes().all(|b| b.is_ascii_hexdigit());
    valid.then_some(digits)
}

impl Target {
    /// Writes the text before the rows.
    pub fn head(&self) -> String {
        let names: Vec<String> = self
            .columns
            .iter()
            .map(|column| quote_identifier(&column.name, self.dialect))
            .collect();
        format!("INSERT INTO {} ({}) VALUES ", self.table, names.join(", "))
    }

    fn literal(&self, column: &TargetColumn, value: &str) -> Result<String, String> {
        if value.contains('\0') {
            return Err(text::import_nul(&column.name));
        }
        if !column.binary {
            return Ok(text_literal(value, self.dialect));
        }
        if self.base64 {
            return match base64_bytes(value) {
                Some(bytes) => Ok(bytes_literal(&bytes, self.dialect)),
                None => Err(text::import_base64(&column.name)),
            };
        }
        // The export writes a binary value that is valid text as that text. A bare
        // literal would keep the type of text, and `bytea` would read each backslash.
        Ok(match hex_digits(value) {
            Some(digits) => binary_literal(digits, self.dialect),
            None => bytes_literal(value.as_bytes(), self.dialect),
        })
    }

    /// Writes the values of one row, or the reason that the row cannot go.
    pub fn values(&self, cells: &[Option<String>]) -> Result<String, String> {
        let mut values = String::from("(");
        for (index, column) in self.columns.iter().enumerate() {
            if index > 0 {
                values.push_str(", ");
            }
            match cells.get(column.source).and_then(Option::as_deref) {
                Some(value) => values.push_str(&self.literal(column, value)?),
                None => values.push_str("NULL"),
            }
        }
        values.push(')');
        Ok(values)
    }
}
