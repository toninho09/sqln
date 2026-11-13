//! The readable form of a JSON value. Only the white space changes: the keys keep their
//! order, and a number keeps each digit, because the grid never changes a value.

use std::iter::Peekable;
use std::ops::Range;
use std::str::Chars;

use serde::de::IgnoredAny;

/// A longer value keeps its text, so that the key that opens it stays fast.
pub const FORMAT_LIMIT: usize = 1024 * 1024;
/// Each row of a deep value repeats its indent, so a short text can give a very long
/// form. A value above this limit keeps its text.
const FORMATTED_LIMIT: usize = 8 * FORMAT_LIMIT;
const INDENT: &str = "  ";

/// Returns a JSON object or array with one member on each row. Any other text returns
/// `None`. The check reads the text without building a value, so it keeps each number.
pub fn pretty(text: &str) -> Option<String> {
    let body = text.trim();
    if !is_container(body) {
        return None;
    }
    indented(body)
}

/// Returns true for a valid object or array within the input limit.
fn is_container(text: &str) -> bool {
    let body = text.trim();
    (body.starts_with('{') || body.starts_with('['))
        && body.len() <= FORMAT_LIMIT
        && serde_json::from_str::<IgnoredAny>(body).is_ok()
}

#[derive(Clone, Debug)]
pub enum Validation {
    Valid,
    Invalid {
        line: usize,
        column: usize,
        reason: String,
    },
    TooLarge,
}

/// Checks only text that starts as a JSON object or array.
pub fn validation(text: &str) -> Option<Validation> {
    let body = text.trim_start();
    if !body.starts_with(['{', '[']) {
        return None;
    }
    if text.len() > FORMAT_LIMIT {
        return Some(Validation::TooLarge);
    }
    Some(match serde_json::from_str::<IgnoredAny>(text) {
        Ok(_) => Validation::Valid,
        Err(error) => {
            let line = error.line().max(1);
            let source = text.split('\n').nth(line - 1).unwrap_or_default();
            let bytes = error.column().min(source.len());
            let column = source
                .char_indices()
                .take_while(|(offset, _)| *offset < bytes)
                .count()
                .max(1);
            let message = error.to_string();
            let suffix = format!(" at line {} column {}", error.line(), error.column());
            let reason = message.strip_suffix(&suffix).unwrap_or(&message).to_owned();
            Validation::Invalid {
                line,
                column,
                reason,
            }
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Key,
    String,
    Number,
    Literal,
}

/// The token ranges of one JSON text. The source keeps its original bytes.
#[derive(Debug)]
pub struct Highlights {
    tokens: Vec<(Range<usize>, Kind)>,
}

impl Highlights {
    pub fn new(text: &str) -> Option<Self> {
        matches!(validation(text), Some(Validation::Valid)).then(|| Self::validated(text))
    }

    /// Reads text that the cell window already validates before formatting.
    pub(crate) fn validated(text: &str) -> Self {
        let mut tokens = Vec::new();
        if text.len() > FORMATTED_LIMIT {
            return Self { tokens };
        }
        let bytes = text.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            let start = at;
            let kind = match bytes[at] {
                b'"' => {
                    at += 1;
                    while at < bytes.len() {
                        match bytes[at] {
                            b'\\' => at = (at + 2).min(bytes.len()),
                            b'"' => {
                                at += 1;
                                break;
                            }
                            _ => at += 1,
                        }
                    }
                    let key = bytes[at..].iter().find(|b| !b.is_ascii_whitespace()) == Some(&b':');
                    if key { Kind::Key } else { Kind::String }
                }
                b'-' | b'0'..=b'9' => {
                    at += 1;
                    while at < bytes.len()
                        && matches!(bytes[at], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                    {
                        at += 1;
                    }
                    Kind::Number
                }
                b't' | b'f' | b'n' => {
                    while at < bytes.len() && bytes[at].is_ascii_alphabetic() {
                        at += 1;
                    }
                    Kind::Literal
                }
                _ => {
                    at += 1;
                    continue;
                }
            };
            tokens.push((start..at, kind));
        }
        Self { tokens }
    }

    /// Returns clipped token ranges inside a visible part of the source.
    pub fn in_range(&self, range: Range<usize>) -> impl Iterator<Item = (Range<usize>, Kind)> + '_ {
        let first = self
            .tokens
            .partition_point(|(token, _)| token.end <= range.start);
        self.tokens[first..]
            .iter()
            .take_while(move |(token, _)| range.start < range.end && token.start < range.end)
            .map(move |(token, kind)| {
                (
                    token.start.max(range.start)..token.end.min(range.end),
                    *kind,
                )
            })
    }
}

fn new_row(out: &mut String, depth: usize) {
    out.push('\n');
    for _ in 0..depth {
        out.push_str(INDENT);
    }
}

/// Copies a string after its first quote. An escape holds the next character.
fn copy_string(chars: &mut Peekable<Chars<'_>>, out: &mut String) {
    out.push('"');
    while let Some(c) = chars.next() {
        out.push(c);
        match c {
            '\\' => out.extend(chars.next()),
            '"' => return,
            _ => {}
        }
    }
}

/// Writes valid JSON again. A loop does this, because a value can nest deeply.
fn indented(json: &str) -> Option<String> {
    let mut out = String::with_capacity(json.len() * 2);
    let mut chars = json.chars().peekable();
    let mut depth = 0_usize;
    while let Some(c) = chars.next() {
        if out.len() > FORMATTED_LIMIT {
            return None;
        }
        match c {
            '"' => copy_string(&mut chars, &mut out),
            '{' | '[' => {
                out.push(c);
                while chars.next_if(char::is_ascii_whitespace).is_some() {}
                // An empty object or array stays on one row.
                match chars.next_if(|next| matches!(next, '}' | ']')) {
                    Some(end) => out.push(end),
                    None => {
                        depth += 1;
                        new_row(&mut out, depth);
                    }
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                new_row(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(c);
                new_row(&mut out, depth);
            }
            ':' => out.push_str(": "),
            c if c.is_ascii_whitespace() => {}
            c => out.push(c),
        }
    }
    Some(out)
}
