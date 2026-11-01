//! The readable form of a JSON value. Only the white space changes: the keys keep their
//! order, and a number keeps each digit, because the grid never changes a value.

use std::iter::Peekable;
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
    let opens = body.starts_with('{') || body.starts_with('[');
    if !opens || body.len() > FORMAT_LIMIT || serde_json::from_str::<IgnoredAny>(body).is_err() {
        return None;
    }
    indented(body)
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
