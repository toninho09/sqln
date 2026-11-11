//! Local syntax warnings. Parser acceptance does not prove database acceptance.

use std::ops::Range;

use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Token, Tokenizer};

use super::{Dialect, Lexer, TokenKind, parameters, split, tokenize};

pub const MAX_BYTES: usize = 256 * 1024;
const MAX_STATEMENTS: usize = 128;
// Parser recursion limits do not bound AST depth or recursive destruction.
const MAX_TOKENS: usize = 1024;
const MAX_WARNINGS: usize = 32;
const MAX_MESSAGE_CHARS: usize = 512;

#[derive(Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    /// Original UTF-8 byte offsets. Unknown parser locations have no underline.
    pub range: Option<Range<usize>>,
    /// One-based character coordinates, not terminal display columns.
    pub location: Option<(usize, usize)>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub diagnostics: Vec<Diagnostic>,
    pub note: Option<String>,
}

impl Report {
    pub fn skipped(reason: &str) -> Self {
        Self {
            note: Some(format!(
                "Syntax check skipped: {reason}. Run remains available."
            )),
            ..Self::default()
        }
    }
}

/// Checks each instruction independently. No connection or parameter values are needed.
pub fn check(sql: &str, dialect: Dialect) -> Report {
    let Some(parser_dialect) = super::parser_dialect(dialect) else {
        return Report::default();
    };
    if sql.len() > MAX_BYTES {
        return Report::skipped("text exceeds 256 KiB");
    }
    let mut report = Report::default();
    if let Some(token) = Lexer::new(sql, dialect).find(|token| {
        token.kind == TokenKind::Comment && !token.closed && sql[token.start()..].starts_with("/*")
    }) {
        report.diagnostics.push(Diagnostic {
            message: "Unclosed block comment. Add */.".into(),
            range: Some(token.start()..token.end()),
            location: Some(location_at(sql, token.start())),
        });
    }
    for (index, statement) in split(sql, dialect).into_iter().enumerate() {
        if index >= MAX_STATEMENTS || report.diagnostics.len() >= MAX_WARNINGS {
            report.note = Some("Syntax check partial: instruction or warning limit reached. Split the script to check the rest.".into());
            break;
        }
        let original = statement.text(sql);
        // ASCII names become a constant and spaces. Byte offsets and columns stay unchanged.
        let mut normalized = original.as_bytes().to_vec();
        for item in parameters::occurrences(original, dialect) {
            normalized[item.range.clone()].fill(b' ');
            normalized[item.range.start] = b'0';
        }
        let normalized = String::from_utf8(normalized).expect("ASCII replacements preserve UTF-8");
        let result = match Tokenizer::new(parser_dialect, &normalized).tokenize_with_location() {
            Ok(tokens) => {
                if tokens
                    .iter()
                    .filter(|token| !matches!(token.token, Token::Whitespace(_)))
                    .nth(MAX_TOKENS)
                    .is_some()
                {
                    report.note = Some(format!(
                        "Syntax check partial: an instruction exceeds {MAX_TOKENS} tokens. Simplify it to check its syntax. Run remains available."
                    ));
                    continue;
                }
                Parser::new(parser_dialect)
                    .with_recursion_limit(64)
                    .with_tokens_with_locations(tokens)
                    .parse_statements()
            }
            Err(error) => Err(error.into()),
        };
        let Err(error) = result else { continue };
        let message = error.to_string();
        let (message, local) = error_location(&message);
        let offset = local.and_then(|(line, column)| offset_at(original, line, column));
        let location = offset.map(|offset| location_at(sql, statement.start + offset));
        let range = offset.and_then(|offset| {
            let tokens = tokenize(original, dialect);
            let token = tokens
                .iter()
                .find(|token| token.start() <= offset && offset < token.end())
                .or_else(|| {
                    tokens
                        .iter()
                        .rev()
                        .find(|token| token.kind != TokenKind::Comment && token.end() <= offset)
                });
            token.map(|token| statement.start + token.start()..statement.start + token.end())
        });
        report.diagnostics.push(Diagnostic {
            message: bounded_message(message),
            range,
            location,
        });
    }
    report
}

fn bounded_message(message: &str) -> String {
    let mut chars = message.chars();
    let mut bounded: String = chars.by_ref().take(MAX_MESSAGE_CHARS).collect();
    if chars.next().is_some() {
        bounded.pop();
        bounded.push('…');
    }
    bounded
}

fn location_at(sql: &str, offset: usize) -> (usize, usize) {
    let before = &sql[..offset];
    let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .chars()
        .count()
        + 1;
    (line, column)
}

// sqlparser 0.63 stores parser locations in the message, not in a typed error field.
fn error_location(message: &str) -> (&str, Option<(usize, usize)>) {
    let Some((body, suffix)) = message.rsplit_once(" at Line: ") else {
        return (message, None);
    };
    let Some((line, column)) = suffix.split_once(", Column: ") else {
        return (message, None);
    };
    match (line.parse(), column.parse()) {
        (Ok(line), Ok(column)) if line > 0 && column > 0 => (body, Some((line, column))),
        _ => (message, None),
    }
}

fn offset_at(sql: &str, line: usize, column: usize) -> Option<usize> {
    let mut start = 0;
    for _ in 1..line {
        start += sql.get(start..)?.find('\n')? + 1;
    }
    let text = sql.get(start..)?.split('\n').next()?;
    if column - 1 == text.chars().count() {
        return Some(start + text.len());
    }
    text.char_indices()
        .nth(column - 1)
        .map(|(offset, _)| start + offset)
}
