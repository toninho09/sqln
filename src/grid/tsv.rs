//! TSV copy format. It follows DataGrip, and it keeps the data that DataGrip loses:
//! trailing `NULL` columns, trailing spaces, and carriage returns.

use super::Value;

const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

/// Returns the digit at `place` of the text of `bytes` after `0x`.
pub(super) fn hex_digit(bytes: &[u8], place: usize) -> Option<char> {
    let byte = *bytes.get(place / 2)?;
    let nibble = if place.is_multiple_of(2) {
        byte >> 4
    } else {
        byte & 15
    };
    Some(char::from(HEX_DIGITS[usize::from(nibble)]))
}

/// Reads all the bytes without an early stop, because the compiler then tests many bytes
/// in one step. A separator is ASCII, so a byte of another character never matches.
fn has_separator(text: &str, delimiter: u8) -> bool {
    text.bytes().fold(false, |found, byte| {
        found | (byte == delimiter) | (byte == b'\n') | (byte == b'\r') | (byte == b'"')
    })
}

/// Writes a field. It quotes a field that holds a separator, or that a reader could take
/// for a `NULL`.
fn push_field(out: &mut String, text: &str, delimiter: u8, null: &str) {
    if !text.is_empty() && text != null && !has_separator(text, delimiter) {
        out.push_str(text);
        return;
    }
    out.push('"');
    let mut start = 0;
    for (at, byte) in text.bytes().enumerate() {
        if byte == b'"' {
            out.push_str(&text[start..=at]);
            out.push('"');
            start = at + 1;
        }
    }
    out.push_str(&text[start..]);
    out.push('"');
}

pub fn delimited_text(text: &str, delimiter: u8, null: &str) -> String {
    let mut out = String::with_capacity(text.len());
    push_field(&mut out, text, delimiter, null);
    out
}

pub fn hex(bytes: &[u8]) -> String {
    let mut out = vec![0; 2 + bytes.len() * 2];
    out[..2].copy_from_slice(b"0x");
    let (pairs, _) = out[2..].as_chunks_mut();
    for (digits, byte) in pairs.iter_mut().zip(bytes) {
        *digits = [
            HEX_DIGITS[usize::from(byte >> 4)],
            HEX_DIGITS[usize::from(byte & 15)],
        ];
    }
    String::from_utf8(out).unwrap_or_default()
}

/// Writes the text of a cell. `null` is the text of a `NULL` value.
pub fn push_cell(out: &mut String, value: &Value, delimiter: u8, null: &str) {
    match value {
        Value::Null => out.push_str(null),
        Value::Text(text) | Value::Number(text) => push_field(out, text, delimiter, null),
        Value::Binary(bytes) => match bytes.text() {
            Some(text) => push_field(out, text, delimiter, null),
            None => out.push_str(&hex(bytes)),
        },
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
            push_cell(&mut out, value, b'\t', "");
        }
        out.push('\n');
    }
    out
}
