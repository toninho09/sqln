//! Named scalar parameters. Values never become SQL source text.

use std::ops::Range;

use super::Dialect;
use super::lexer::{TokenKind, tokenize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Text,
    Number,
    Boolean,
    Null,
}

impl Kind {
    pub fn next(self) -> Self {
        match self {
            Self::Text => Self::Number,
            Self::Number => Self::Boolean,
            Self::Boolean => Self::Null,
            Self::Null => Self::Text,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Parameter {
    pub name: String,
    pub kind: Kind,
    pub value: String,
}

impl std::fmt::Debug for Parameter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Parameter")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("value", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Occurrence {
    pub name: String,
    pub range: Range<usize>,
}

/// Accepts ASCII names with a letter or underscore first. Quotes and comments are opaque.
pub fn occurrences(sql: &str, dialect: Dialect) -> Vec<Occurrence> {
    if dialect == Dialect::Redis {
        return Vec::new();
    }
    let tokens = tokenize(sql, dialect);
    let bytes = sql.as_bytes();
    tokens.windows(2).filter_map(|pair| {
        let (colon, name) = (pair[0], pair[1]);
        let start = colon.start();
        if colon.kind != TokenKind::Punctuation || bytes[start] != b':'
            || colon.end() != name.start() || name.kind != TokenKind::Word
            || start > 0 && matches!(bytes[start - 1], b':' | b'_' | b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'$')
        {
            return None;
        }
        let word = &sql[name.start()..name.end()];
        if !word.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return None;
        }
        Some(Occurrence { name: word.to_owned(), range: start..name.end() })
    }).collect()
}

pub fn names(sql: &str, dialect: Dialect) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in occurrences(sql, dialect) {
        if seen.insert(item.name.clone()) {
            names.push(item.name);
        }
    }
    names
}

/// Validates number syntax without converting the value to a floating-point number.
pub fn is_number(text: &str) -> bool {
    text.starts_with(|c: char| c == '-' || c.is_ascii_digit())
        && serde_json::from_str::<Box<serde_json::value::RawValue>>(text)
            .is_ok_and(|raw| raw.get() == text)
}

/// MySQL binds strings, not decimal values. A typed cast keeps decimal precision.
fn mysql_number(text: &str) -> Result<(String, usize, usize), String> {
    let (negative, unsigned) = text.strip_prefix('-').map_or((false, text), |t| (true, t));
    let (mantissa, exponent) = unsigned.split_once(['e', 'E']).unwrap_or((unsigned, "0"));
    let exponent: i32 = exponent
        .parse()
        .map_err(|_| "The number exceeds the MySQL decimal range.")?;
    let (integer, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{integer}{fraction}");
    let point = i64::try_from(integer.len()).unwrap_or(i64::MAX) + i64::from(exponent);
    let length = i64::try_from(digits.len()).unwrap_or(i64::MAX);
    if point > 65 || length - point > 30 || point < -30 {
        return Err(
            "The number exceeds the MySQL decimal range (65 digits, 30 decimal places).".to_owned(),
        );
    }
    let mut value = if point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else if point >= length {
        format!("{digits}{}", "0".repeat((point - length) as usize))
    } else {
        let point = point as usize;
        format!("{}.{}", &digits[..point], &digits[point..])
    };
    let (whole, fractional) = value.split_once('.').unwrap_or((&value, ""));
    let scale = fractional.len();
    let precision = (whole.trim_start_matches('0').len() + scale).max(1);
    if precision > 65 {
        return Err("The number exceeds the MySQL decimal range.".to_owned());
    }
    if negative {
        value.insert(0, '-');
    }
    Ok((value, precision, scale))
}

#[derive(Debug)]
pub struct BoundSql {
    pub sql: String,
    pub parameters: Vec<Parameter>,
    replacements: Vec<(Range<usize>, Range<usize>)>,
}

impl BoundSql {
    pub fn source_offset(&self, offset: usize) -> usize {
        let mut delta: isize = 0;
        for (source, sent) in &self.replacements {
            if offset < sent.start {
                break;
            }
            if offset < sent.end {
                return source.start;
            }
            delta += source.len() as isize - sent.len() as isize;
        }
        offset.saturating_add_signed(delta)
    }
}

pub fn bind(sql: &str, dialect: Dialect, values: &[Parameter]) -> Result<BoundSql, String> {
    let mut result = BoundSql {
        sql: String::new(),
        parameters: Vec::new(),
        replacements: Vec::new(),
    };
    let mut end = 0;
    for occurrence in occurrences(sql, dialect) {
        let mut parameter = values
            .iter()
            .find(|value| value.name == occurrence.name)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "Set parameter :{} in the Parameters panel.",
                    occurrence.name
                )
            })?;
        match parameter.kind {
            Kind::Number if !is_number(&parameter.value) => {
                return Err(format!(
                    "Parameter :{} needs a valid number.",
                    parameter.name
                ));
            }
            Kind::Boolean if !matches!(parameter.value.as_str(), "true" | "false") => {
                return Err(format!(
                    "Parameter :{} needs true or false.",
                    parameter.name
                ));
            }
            _ => {}
        }
        result.sql.push_str(&sql[end..occurrence.range.start]);
        let start = result.sql.len();
        let marker = match (dialect, parameter.kind) {
            (Dialect::Postgres, _) => format!("${}", result.parameters.len() + 1),
            (Dialect::Sqlite, Kind::Number) => "CAST(? AS NUMERIC)".to_owned(),
            (Dialect::MySql, Kind::Number) => {
                let (value, precision, scale) = mysql_number(&parameter.value)?;
                parameter.value = value;
                format!("CAST(? AS DECIMAL({precision},{scale}))")
            }
            _ => "?".to_owned(),
        };
        result.sql.push_str(&marker);
        result
            .replacements
            .push((occurrence.range.clone(), start..result.sql.len()));
        result.parameters.push(parameter);
        end = occurrence.range.end;
    }
    result.sql.push_str(&sql[end..]);
    // Native markers cannot share a binding list with named parameters.
    if !result.parameters.is_empty() {
        let masked =
            occurrences(sql, dialect)
                .into_iter()
                .rev()
                .fold(sql.to_owned(), |mut text, item| {
                    text.replace_range(item.range, " ");
                    text
                });
        let tokens = tokenize(&masked, dialect);
        if tokens.iter().any(|token| {
            token.kind == TokenKind::Punctuation
                && match &masked[token.start()..token.end()] {
                    "?" => dialect != Dialect::Postgres,
                    "$" => masked
                        .as_bytes()
                        .get(token.end())
                        .is_some_and(u8::is_ascii_alphanumeric),
                    "@" => dialect == Dialect::Sqlite,
                    _ => false,
                }
        }) {
            return Err("Do not mix :name parameters with native parameter markers. Use :name for each value.".to_owned());
        }
    }
    Ok(result)
}

/// Command history keeps values separate from SQL. Diagnostic logs must not use this text.
pub fn command(sql: &str, parameters: &[Parameter]) -> String {
    use std::fmt::Write;
    let mut text = crate::command_log::LimitedText::new(crate::command_log::HISTORY_BYTES - 1024);
    let _ = write!(text, "{sql}");
    if !parameters.is_empty() && write!(text, "\nParameters:").is_ok() {
        for (index, value) in parameters.iter().enumerate() {
            if write!(
                text,
                "\n{} (:{}): {:?} = {:?}",
                index + 1,
                value.name,
                value.kind,
                value.value
            )
            .is_err()
            {
                break;
            }
        }
    }
    text.finish()
}
