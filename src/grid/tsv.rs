//! TSV copy format. It follows DataGrip, and it keeps the data that DataGrip loses:
//! trailing `NULL` columns, trailing spaces, and carriage returns.

use std::borrow::Cow;

use super::{Binary, Value};

/// Quotes a field that holds a separator, or that a reader could take for a `NULL`.
pub fn delimited_text<'a>(text: &'a str, delimiter: char, null: &str) -> Cow<'a, str> {
    let plain = !text.is_empty() && text != null && !text.contains([delimiter, '\n', '\r', '"']);
    if plain {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(format!("\"{}\"", text.replace('"', "\"\"")))
    }
}

pub fn hex(bytes: &[u8]) -> String {
    let digits: String = bytes.iter().map(|byte| format!("{byte:02X}")).collect();
    format!("0x{digits}")
}

fn binary<'a>(bytes: &'a Binary, delimiter: char, null: &str) -> Cow<'a, str> {
    match bytes.text() {
        Some(text) => delimited_text(text, delimiter, null),
        None => Cow::Owned(hex(bytes)),
    }
}

/// Returns the text of a cell. `null` is the text of a `NULL` value.
pub fn delimited_cell<'a>(value: &'a Value, delimiter: char, null: &'a str) -> Cow<'a, str> {
    match value {
        Value::Null => Cow::Borrowed(null),
        Value::Text(text) | Value::Number(text) => delimited_text(text, delimiter, null),
        Value::Binary(bytes) => binary(bytes, delimiter, null),
    }
}

/// Writes rows of cells, with a line feed after each row and without a header.
pub fn encode<'a>(rows: impl Iterator<Item = impl Iterator<Item = &'a Value>>) -> String {
    let mut out = String::new();
    for row in rows {
        for (index, value) in row.enumerate() {
            if index > 0 {
                out.push('\t');
            }
            out.push_str(&delimited_cell(value, '\t', ""));
        }
        out.push('\n');
    }
    out
}
