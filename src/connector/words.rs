//! Words of a command line, with the quote rules of `redis-cli`. Redis and Kafka
//! consoles read each line with these rules.

use super::ExecError;
use crate::ui::text;

fn bad_line(offset: usize) -> ExecError {
    ExecError {
        message: text::REDIS_BAD_QUOTES.to_owned(),
        offset: Some(offset),
    }
}

/// Reads the value of a `\x` escape. It returns `None` for any other text.
fn hex_escape(bytes: &[u8]) -> Option<u8> {
    let digits = std::str::from_utf8(bytes.get(..2)?).ok()?;
    u8::from_str_radix(digits, 16).ok()
}

/// Reads one word from `start`, as `redis-cli` does. It returns the word and the
/// position after it. A quote can start in the middle of a word, and a closing quote
/// ends the word.
fn read_word(bytes: &[u8], start: usize) -> Result<(Vec<u8>, usize), ExecError> {
    let mut word = Vec::new();
    let mut quote = None;
    let mut pos = start;
    loop {
        let Some(&byte) = bytes.get(pos) else {
            return if quote.is_some() {
                Err(bad_line(start))
            } else {
                Ok((word, pos))
            };
        };
        pos += 1;
        match (quote, byte) {
            (None, b' ' | b'\t' | b'\r' | b'\n') => return Ok((word, pos)),
            (None, b'"' | b'\'') => quote = Some(byte),
            (Some(open), _) if byte == open => {
                let follows_space = bytes.get(pos).is_none_or(u8::is_ascii_whitespace);
                return if follows_space {
                    Ok((word, pos))
                } else {
                    Err(bad_line(start))
                };
            }
            (Some(b'\''), b'\\') if bytes.get(pos) == Some(&b'\'') => {
                word.push(b'\'');
                pos += 1;
            }
            (Some(b'"'), b'\\') if bytes.get(pos) == Some(&b'x') => {
                match bytes.get(pos + 1..).and_then(hex_escape) {
                    Some(value) => {
                        word.push(value);
                        pos += 3;
                    }
                    None => {
                        word.push(b'x');
                        pos += 1;
                    }
                }
            }
            (Some(b'"'), b'\\') if pos < bytes.len() => {
                word.push(match bytes[pos] {
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'b' => 8,
                    b'a' => 7,
                    other => other,
                });
                pos += 1;
            }
            _ => word.push(byte),
        }
    }
}

/// Splits a command line in words, as `redis-cli` does. Kafka commands use the same rules.
pub fn split_words(line: &str) -> Result<Vec<Vec<u8>>, ExecError> {
    let bytes = line.as_bytes();
    let mut words = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        if bytes[pos].is_ascii_whitespace() {
            pos += 1;
            continue;
        }
        let (word, next) = read_word(bytes, pos)?;
        words.push(word);
        pos = next;
    }
    Ok(words)
}

/// Writes a word between double quotes, so that `split_words` reads the same text.
pub fn quoted_word(word: &str) -> String {
    let mut quoted = String::with_capacity(word.len() + 2);
    quoted.push('"');
    for c in word.chars() {
        match c {
            '"' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            c if c.is_ascii_control() => quoted.push_str(&format!("\\x{:02x}", u32::from(c))),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// Writes a name as one word of a command line. A name without spaces, quotes, or
/// control characters stays as it is.
pub fn command_word(name: &str) -> String {
    let plain = !name.is_empty()
        && !name
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '\'');
    if plain {
        name.to_owned()
    } else {
        quoted_word(name)
    }
}
