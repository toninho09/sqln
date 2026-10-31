//! TSV copy format. It follows DataGrip, and it keeps the data that DataGrip loses:
//! trailing `NULL` columns, trailing spaces, and carriage returns.

use std::borrow::Cow;

use super::Value;

fn quote(text: &str) -> Cow<'_, str> {
    if text.is_empty() {
        return Cow::Borrowed("\"\"");
    }
    if text.contains(['\t', '\n', '\r', '"']) {
        return Cow::Owned(format!("\"{}\"", text.replace('"', "\"\"")));
    }
    Cow::Borrowed(text)
}

pub fn hex(bytes: &[u8]) -> String {
    let digits: String = bytes.iter().map(|byte| format!("{byte:02X}")).collect();
    format!("0x{digits}")
}

fn binary(bytes: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(bytes) {
        Ok(text) if !text.contains(char::is_control) => quote(text),
        _ => Cow::Owned(hex(bytes)),
    }
}

fn cell(value: &Value) -> Cow<'_, str> {
    match value {
        Value::Null => Cow::Borrowed(""),
        Value::Text(text) | Value::Number(text) => quote(text),
        Value::Binary(bytes) => binary(bytes),
    }
}

/// Writes rows of cells, with a line feed after each row and without a header.
pub fn encode<'a>(rows: impl Iterator<Item = &'a [Value]>) -> String {
    let mut out = String::new();
    for row in rows {
        for (index, value) in row.iter().enumerate() {
            if index > 0 {
                out.push('\t');
            }
            out.push_str(&cell(value));
        }
        out.push('\n');
    }
    out
}
