//! The `TabSeparatedWithNamesAndTypes` format of ClickHouse. A tab separates two fields,
//! and a line feed ends a row. A field writes a tab, a line feed, and a backslash with
//! a backslash escape. `\N` alone is `NULL`.

use std::borrow::Cow;

use crate::grid::{RowWriter, Value};

/// The fields of one line, without their escapes. `None` is `NULL`.
pub fn fields(line: &[u8]) -> impl Iterator<Item = Option<Cow<'_, [u8]>>> {
    line.split(|byte| *byte == b'\t').map(field)
}

fn field(raw: &[u8]) -> Option<Cow<'_, [u8]>> {
    if raw == b"\\N" {
        return None;
    }
    if !raw.contains(&b'\\') {
        return Some(Cow::Borrowed(raw));
    }
    let mut bytes = Vec::with_capacity(raw.len());
    let mut escape = false;
    for &byte in raw {
        if escape {
            bytes.push(match byte {
                b'b' => 0x08,
                b'f' => 0x0c,
                b'r' => b'\r',
                b'n' => b'\n',
                b't' => b'\t',
                b'0' => 0,
                other => other,
            });
            escape = false;
        } else if byte == b'\\' {
            escape = true;
        } else {
            bytes.push(byte);
        }
    }
    Some(Cow::Owned(bytes))
}

/// The fields of a line as text, for the names, the types, and the catalog reads.
pub fn text_fields(line: &[u8]) -> Vec<Option<String>> {
    fields(line)
        .map(|field| field.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
        .collect()
}

/// Returns true for a ClickHouse type whose values are numbers, such as `UInt64` or
/// `Nullable(Decimal(10, 2))`.
pub fn is_number(kind: &str) -> bool {
    let mut kind = kind;
    for wrapper in ["Nullable(", "LowCardinality("] {
        if let Some(inner) = kind.strip_prefix(wrapper) {
            kind = inner;
        }
    }
    ["Int", "UInt", "Float", "Decimal"]
        .iter()
        .any(|prefix| kind.starts_with(prefix))
}

/// Writes the fields of a line into a row. A text that is not valid UTF-8 keeps its
/// bytes.
pub fn write_row(row: &mut RowWriter, line: &[u8], numbers: &[bool]) {
    for (index, field) in fields(line).enumerate() {
        match field {
            None => row.push(Value::Null),
            Some(bytes) => match std::str::from_utf8(&bytes) {
                Ok(text) if numbers.get(index).copied().unwrap_or(false) => {
                    row.push(Value::Number(text));
                }
                Ok(text) => row.push(Value::Text(text)),
                Err(_) => row.push(Value::binary(&bytes)),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_loses_its_escapes_and_null_stays_null() {
        let line = b"a\\tb\t\\N\tline\\nfeed\t\\\\N\t";
        let fields: Vec<Option<Vec<u8>>> = fields(line)
            .map(|field| field.map(Cow::into_owned))
            .collect();
        assert_eq!(
            fields,
            vec![
                Some(b"a\tb".to_vec()),
                None,
                Some(b"line\nfeed".to_vec()),
                Some(b"\\N".to_vec()),
                Some(Vec::new()),
            ]
        );
    }

    #[test]
    fn number_types_include_their_wrappers() {
        for kind in [
            "UInt8",
            "Int64",
            "Float64",
            "Decimal(10, 2)",
            "Nullable(Int32)",
        ] {
            assert!(is_number(kind), "{kind}");
        }
        for kind in ["String", "Date", "Nullable(String)", "Array(Int32)", "Bool"] {
            assert!(!is_number(kind), "{kind}");
        }
    }
}
