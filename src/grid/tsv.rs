//! TSV copy format. It follows DataGrip, and it keeps the data that DataGrip loses:
//! trailing `NULL` columns, trailing spaces, and carriage returns.

use super::{Row, Value};

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

const LOW_BITS: u64 = 0x0101_0101_0101_0101;
const HIGH_BITS: u64 = 0x8080_8080_8080_8080;

/// Returns a word with the high bit of each zero byte of `word` set.
#[inline]
fn zero_bytes(word: u64) -> u64 {
    word.wrapping_sub(LOW_BITS) & !word & HIGH_BITS
}

/// Reads eight bytes at a time, because four tests of each byte cost a third of the
/// time of a scan of one. A separator is ASCII, so a byte of another character never
/// matches.
fn has_separator(text: &[u8], delimiter: u8) -> bool {
    let repeat = |byte: u8| u64::from(byte) * LOW_BITS;
    let (delimiter_word, line_feed, carriage_return, quote) = (
        repeat(delimiter),
        repeat(b'\n'),
        repeat(b'\r'),
        repeat(b'"'),
    );
    let (chunks, rest) = text.as_chunks::<8>();
    for chunk in chunks {
        let word = u64::from_ne_bytes(*chunk);
        let found = zero_bytes(word ^ delimiter_word)
            | zero_bytes(word ^ line_feed)
            | zero_bytes(word ^ carriage_return)
            | zero_bytes(word ^ quote);
        if found != 0 {
            return true;
        }
    }
    rest.iter()
        .any(|&byte| byte == delimiter || byte == b'\n' || byte == b'\r' || byte == b'"')
}

/// Writes a field. It quotes a field that holds a separator, or that a reader could take
/// for a `NULL`.
fn push_field(out: &mut String, text: &str, delimiter: u8, null: &str) {
    if !text.is_empty() && text != null && !has_separator(text.as_bytes(), delimiter) {
        out.push_str(text);
        return;
    }
    out.push('"');
    let mut start = 0;
    for (at, _) in text.match_indices('"') {
        out.push_str(&text[start..=at]);
        out.push('"');
        start = at + 1;
    }
    out.push_str(&text[start..]);
    out.push('"');
}

pub fn delimited_text(text: &str, delimiter: u8, null: &str) -> String {
    let mut out = String::with_capacity(text.len());
    push_field(&mut out, text, delimiter, null);
    out
}

/// The two digits of each byte in the order of a `u16`, so one store writes both.
const fn hex_pairs() -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut value = 0;
    while value < 256 {
        table[value] = HEX_DIGITS[value >> 4] as u16 | ((HEX_DIGITS[value & 15] as u16) << 8);
        value += 1;
    }
    table
}
static PAIRS: [u16; 256] = hex_pairs();

pub fn hex(bytes: &[u8]) -> String {
    let mut out = vec![0; 2 + bytes.len() * 2];
    out[..2].copy_from_slice(b"0x");
    {
        let (blocks, _) = out[2..].as_chunks_mut::<8>();
        let (source, rest) = bytes.as_chunks::<4>();
        for (block, chunk) in blocks.iter_mut().zip(source) {
            let first = PAIRS[usize::from(chunk[0])].to_le_bytes();
            let second = PAIRS[usize::from(chunk[1])].to_le_bytes();
            let third = PAIRS[usize::from(chunk[2])].to_le_bytes();
            let fourth = PAIRS[usize::from(chunk[3])].to_le_bytes();
            *block = [
                first[0], first[1], second[0], second[1], third[0], third[1], fourth[0], fourth[1],
            ];
        }
        let done = source.len() * 8;
        let (tail, _) = out[2 + done..].as_chunks_mut::<2>();
        for (pair, byte) in tail.iter_mut().zip(rest) {
            *pair = PAIRS[usize::from(*byte)].to_le_bytes();
        }
    }
    // SAFETY: the buffer holds `0x` and two hexadecimal digits of each byte, which are
    // all ASCII.
    unsafe { String::from_utf8_unchecked(out) }
}

/// Writes the text of a cell. `null` is the text of a `NULL` value.
pub fn push_cell(out: &mut String, value: Value<'_>, delimiter: u8, null: &str) {
    match value {
        Value::Null => out.push_str(null),
        Value::Text(text) | Value::Number(text) => push_field(out, text, delimiter, null),
        Value::Binary(bytes) => match bytes.text() {
            Some(text) => push_field(out, text, delimiter, null),
            None => out.push_str(&hex(&bytes)),
        },
    }
}

/// Writes rows of cells, with a line feed after each row and without a header. It reads
/// the cells of a row in order, because an iterator of cells for each row costs a step
/// more for each value.
pub fn encode(rows: &[Row], left: usize, right: usize, size: usize) -> String {
    let mut out = String::with_capacity(size);
    let columns = right - left + 1;
    for row in rows {
        let mut cells = row.cells();
        for _ in 0..left {
            cells.next();
        }
        for index in 0..columns {
            if index > 0 {
                out.push('\t');
            }
            // A broken reply can hold a row without its last values.
            match cells.next() {
                None | Some(Value::Null) => {}
                Some(Value::Text(text) | Value::Number(text)) => {
                    push_field(&mut out, text, b'\t', "")
                }
                Some(Value::Binary(bytes)) => match bytes.text() {
                    Some(text) => push_field(&mut out, text, b'\t', ""),
                    None => out.push_str(&hex(&bytes)),
                },
            }
        }
        out.push('\n');
    }
    out
}
