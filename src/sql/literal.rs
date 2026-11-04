//! The literals of the SQL that SQLN writes: text, bytes, and numbers.

use super::{Dialect, escape_literal, quote_literal};
use crate::grid::{Value, hex};

/// Writes text as a literal that does not change with the session. PostgreSQL uses the
/// `E` form, and MySQL uses the hexadecimal form for text with a backslash.
pub fn text_literal(value: &str, dialect: Dialect) -> String {
    match dialect {
        Dialect::Postgres => escape_literal(value),
        // The hexadecimal form does not change with `NO_BACKSLASH_ESCAPES`.
        Dialect::MySql if value.contains('\\') => {
            format!("_utf8mb4 {}", hex(value.as_bytes()))
        }
        _ => quote_literal(value, dialect),
    }
}

/// Writes hexadecimal digits as a literal of bytes.
pub fn binary_literal(digits: &str, dialect: Dialect) -> String {
    if dialect == Dialect::Postgres {
        escape_literal(&format!("\\x{digits}"))
    } else {
        format!("X'{digits}'")
    }
}

pub fn bytes_literal(bytes: &[u8], dialect: Dialect) -> String {
    let digits = hex(bytes);
    binary_literal(digits.trim_start_matches("0x"), dialect)
}

/// Writes a value of the grid as a literal of a dialect. A number keeps its text.
pub fn value_literal(value: Value<'_>, dialect: Dialect) -> String {
    match value {
        Value::Null => "NULL".to_owned(),
        Value::Number(text) if is_number(text) => text.to_owned(),
        Value::Text(text) | Value::Number(text) => text_literal(text, dialect),
        Value::Binary(bytes) => bytes_literal(bytes.bytes(), dialect),
    }
}

/// Returns true for text that SQL and JSON read as a number.
pub fn is_number(text: &str) -> bool {
    serde_json::from_str::<serde_json::Number>(text).is_ok()
}
