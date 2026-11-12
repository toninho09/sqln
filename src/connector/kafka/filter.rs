//! The filter of a Kafka read. Kafka does not filter on the server, so SQLN reads each
//! message and keeps the messages that match, after the decoding.

use std::cmp::Ordering;

use regex::{Regex, RegexBuilder};
use serde_json::Value as Json;

use super::command::{Cursor, TokenKind, error_at};
use crate::connector::ExecError;
use crate::ui::text;

/// The largest compiled regular expression. A larger one stops the read before it starts.
const REGEX_LIMIT: usize = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Key,
    Value,
    Headers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operator {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    Text(String),
    Number(f64),
    Bool(bool),
    Null,
}

#[derive(Debug)]
pub enum Condition {
    /// The key is this text.
    KeyIs(String),
    /// The text of a part holds this text, without case. The text is in lowercase.
    Contains(Part, String),
    Matches(Part, Regex),
    /// A field of the decoded value. An empty path compares the whole value.
    Compare {
        path: Vec<String>,
        operator: Operator,
        literal: Literal,
    },
}

#[derive(Debug)]
pub enum Filter {
    Condition(Condition),
    Not(Box<Filter>),
    And(Box<Filter>, Box<Filter>),
    Or(Box<Filter>, Box<Filter>),
}

/// The parts of one message that a filter reads.
pub struct Message<'a> {
    pub key: Option<&'a str>,
    pub value: Option<&'a str>,
    /// The decoded value. `None` for a value without JSON.
    pub json: Option<&'a Json>,
    pub headers: &'a str,
}

/// Reads a filter. It stops before a word that does not continue the filter, such as
/// `LIMIT`, so the next clause can follow.
pub fn parse(cursor: &mut Cursor<'_>) -> Result<Filter, ExecError> {
    parse_or(cursor)
}

fn parse_or(cursor: &mut Cursor<'_>) -> Result<Filter, ExecError> {
    let mut left = parse_and(cursor)?;
    while cursor.peek().is_some_and(|token| token.is_word("OR")) {
        cursor.advance();
        let right = parse_and(cursor)?;
        left = Filter::Or(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_and(cursor: &mut Cursor<'_>) -> Result<Filter, ExecError> {
    let mut left = parse_not(cursor)?;
    while cursor.peek().is_some_and(|token| token.is_word("AND")) {
        cursor.advance();
        let right = parse_not(cursor)?;
        left = Filter::And(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_not(cursor: &mut Cursor<'_>) -> Result<Filter, ExecError> {
    if cursor.peek().is_some_and(|token| token.is_word("NOT")) {
        cursor.advance();
        return Ok(Filter::Not(Box::new(parse_not(cursor)?)));
    }
    if cursor.peek().is_some_and(|token| token.is_operator("(")) {
        cursor.advance();
        let inner = parse_or(cursor)?;
        let offset = cursor.offset();
        if !cursor.advance().is_some_and(|token| token.is_operator(")")) {
            return Err(error_at(text::KAFKA_NO_CLOSE, offset));
        }
        return Ok(inner);
    }
    parse_condition(cursor).map(Filter::Condition)
}

fn part(word: &str) -> Option<Part> {
    [
        ("key", Part::Key),
        ("value", Part::Value),
        ("headers", Part::Headers),
    ]
    .into_iter()
    .find(|(name, _)| name.eq_ignore_ascii_case(word))
    .map(|(_, part)| part)
}

fn operator(text: &str) -> Option<Operator> {
    Some(match text {
        "=" => Operator::Equal,
        "!=" => Operator::NotEqual,
        "<" => Operator::Less,
        "<=" => Operator::LessOrEqual,
        ">" => Operator::Greater,
        ">=" => Operator::GreaterOrEqual,
        _ => return None,
    })
}

/// Reads a text between quotes after `CONTAINS`, `MATCHES`, or `key =`.
fn text_operand(cursor: &mut Cursor<'_>) -> Result<(String, usize), ExecError> {
    let offset = cursor.offset();
    match cursor.advance() {
        Some(token) if token.kind == TokenKind::Text => Ok((token.text.clone(), offset)),
        _ => Err(error_at(text::KAFKA_NEEDS_TEXT, offset)),
    }
}

fn literal(cursor: &mut Cursor<'_>) -> Result<Literal, ExecError> {
    let offset = cursor.offset();
    let token = cursor
        .advance()
        .ok_or_else(|| error_at(text::KAFKA_NEEDS_VALUE, offset))?;
    match token.kind {
        TokenKind::Text => Ok(Literal::Text(token.text.clone())),
        TokenKind::Operator => Err(error_at(text::KAFKA_NEEDS_VALUE, offset)),
        TokenKind::Word if token.is_word("true") => Ok(Literal::Bool(true)),
        TokenKind::Word if token.is_word("false") => Ok(Literal::Bool(false)),
        TokenKind::Word if token.is_word("null") => Ok(Literal::Null),
        TokenKind::Word => token
            .text
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .map(Literal::Number)
            .ok_or_else(|| error_at(text::KAFKA_NEEDS_VALUE, offset)),
    }
}

fn parse_condition(cursor: &mut Cursor<'_>) -> Result<Condition, ExecError> {
    let offset = cursor.offset();
    let token = cursor
        .advance()
        .filter(|token| token.kind == TokenKind::Word)
        .ok_or_else(|| error_at(text::KAFKA_NEEDS_FIELD, offset))?;
    let mut names = token.text.split('.');
    let first = names.next().unwrap_or("");
    let path: Vec<String> = names.map(str::to_owned).collect();
    let part = part(first).ok_or_else(|| error_at(text::KAFKA_NEEDS_FIELD, offset))?;
    if path.iter().any(String::is_empty) || (!path.is_empty() && part != Part::Value) {
        return Err(error_at(text::KAFKA_NEEDS_FIELD, offset));
    }
    let next_offset = cursor.offset();
    let next = cursor
        .advance()
        .ok_or_else(|| error_at(text::KAFKA_NEEDS_OPERATOR, next_offset))?;
    if path.is_empty() && next.is_word("CONTAINS") {
        let (needle, _) = text_operand(cursor)?;
        return Ok(Condition::Contains(part, needle.to_lowercase()));
    }
    if path.is_empty() && next.is_word("MATCHES") && part != Part::Headers {
        let (pattern, at) = text_operand(cursor)?;
        let regex = RegexBuilder::new(&pattern)
            .size_limit(REGEX_LIMIT)
            .dfa_size_limit(REGEX_LIMIT)
            .build()
            .map_err(|_| error_at(text::KAFKA_BAD_REGEX, at))?;
        return Ok(Condition::Matches(part, regex));
    }
    let operator = (next.kind == TokenKind::Operator)
        .then(|| operator(&next.text))
        .flatten()
        .ok_or_else(|| error_at(text::KAFKA_NEEDS_OPERATOR, next_offset))?;
    match part {
        Part::Key if operator == Operator::Equal => {
            let (key, _) = text_operand(cursor)?;
            Ok(Condition::KeyIs(key))
        }
        Part::Value => Ok(Condition::Compare {
            path,
            operator,
            literal: literal(cursor)?,
        }),
        _ => Err(error_at(text::KAFKA_NEEDS_OPERATOR, next_offset)),
    }
}

impl Filter {
    /// Returns true when the filter compares a field, so the reader needs JSON.
    pub fn reads_fields(&self) -> bool {
        match self {
            Filter::Condition(Condition::Compare { .. }) => true,
            Filter::Condition(_) => false,
            Filter::Not(inner) => inner.reads_fields(),
            Filter::And(left, right) | Filter::Or(left, right) => {
                left.reads_fields() || right.reads_fields()
            }
        }
    }

    pub fn matches(&self, message: &Message<'_>) -> bool {
        match self {
            Filter::Condition(condition) => condition.matches(message),
            Filter::Not(inner) => !inner.matches(message),
            Filter::And(left, right) => left.matches(message) && right.matches(message),
            Filter::Or(left, right) => left.matches(message) || right.matches(message),
        }
    }
}

fn part_text<'a>(part: Part, message: &Message<'a>) -> Option<&'a str> {
    match part {
        Part::Key => message.key,
        Part::Value => message.value,
        Part::Headers => Some(message.headers),
    }
}

/// Returns the field at the path. A number in the path also reads an item of a list.
pub(super) fn field<'a>(json: &'a Json, path: &[String]) -> Option<&'a Json> {
    path.iter().try_fold(json, |current, name| match current {
        Json::Object(map) => map.get(name),
        Json::Array(items) => items.get(name.parse::<usize>().ok()?),
        _ => None,
    })
}

/// Compares a field with a literal of the same type. Other types have no order.
fn order(value: &Json, literal: &Literal) -> Option<Ordering> {
    match (value, literal) {
        (Json::Number(number), Literal::Number(literal)) => number.as_f64()?.partial_cmp(literal),
        (Json::String(text), Literal::Text(literal)) => Some(text.as_str().cmp(literal)),
        (Json::Bool(value), Literal::Bool(literal)) => Some(value.cmp(literal)),
        (Json::Null, Literal::Null) => Some(Ordering::Equal),
        _ => None,
    }
}

impl Condition {
    fn matches(&self, message: &Message<'_>) -> bool {
        match self {
            Condition::KeyIs(key) => message.key == Some(key.as_str()),
            Condition::Contains(part, needle) => part_text(*part, message)
                .is_some_and(|text| text.to_lowercase().contains(needle.as_str())),
            Condition::Matches(part, regex) => {
                part_text(*part, message).is_some_and(|text| regex.is_match(text))
            }
            Condition::Compare {
                path,
                operator,
                literal,
            } => {
                let Some(value) = message.json.and_then(|json| field(json, path)) else {
                    return false;
                };
                if *literal == Literal::Null || value.is_null() {
                    let same = value.is_null() == (*literal == Literal::Null);
                    return match operator {
                        Operator::Equal => same,
                        Operator::NotEqual => !same,
                        _ => false,
                    };
                }
                let Some(order) = order(value, literal) else {
                    return false;
                };
                match operator {
                    Operator::Equal => order == Ordering::Equal,
                    Operator::NotEqual => order != Ordering::Equal,
                    Operator::Less => order == Ordering::Less,
                    Operator::LessOrEqual => order != Ordering::Greater,
                    Operator::Greater => order == Ordering::Greater,
                    Operator::GreaterOrEqual => order != Ordering::Less,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::command::tokenize;
    use super::*;

    fn filter(text: &str) -> Result<Filter, ExecError> {
        let tokens = tokenize(text).unwrap();
        let mut cursor = Cursor::new(&tokens, text.len());
        let filter = parse(&mut cursor)?;
        assert!(cursor.peek().is_none(), "the filter must read all words");
        Ok(filter)
    }

    fn check(text: &str, json: &str) -> bool {
        let json: Json = serde_json::from_str(json).unwrap();
        let value = json.to_string();
        let message = Message {
            key: Some("Order-17"),
            value: Some(&value),
            json: Some(&json),
            headers: r#"{"source":"web"}"#,
        };
        filter(text).unwrap().matches(&message)
    }

    #[test]
    fn each_operator_compares_a_field_of_the_same_type() {
        let json = r#"{"total": 12.5, "name": "Ana", "paid": true, "items": [{"sku": "a"}]}"#;
        assert!(check("value.total = 12.5", json));
        assert!(check("value.total != 3", json));
        assert!(check("value.total < 13", json));
        assert!(check("value.total <= 12.5", json));
        assert!(check("value.total > 12", json));
        assert!(check("value.total >= 12.5", json));
        assert!(check("value.name = 'Ana'", json));
        assert!(check("value.name > 'A'", json));
        assert!(check("value.paid = true", json));
        assert!(check("value.items.0.sku = 'a'", json));
        assert!(
            !check("value.total = '12.5'", json),
            "a text is not a number"
        );
        assert!(check("value.paid > false", json), "true comes after false");
    }

    #[test]
    fn a_missing_field_is_false_and_null_has_its_own_rule() {
        let json = r#"{"note": null, "name": "Ana"}"#;
        assert!(!check("value.missing = 1", json));
        assert!(!check("value.missing != 1", json));
        assert!(check("NOT value.missing = 1", json));
        assert!(check("value.note = null", json));
        assert!(!check("value.note != null", json));
        assert!(check("value.name != null", json));
        assert!(!check("value.note > 1", json));
    }

    #[test]
    fn text_conditions_read_key_value_and_headers() {
        let json = r#"{"name": "Ana Silva"}"#;
        assert!(check("key = 'Order-17'", json));
        assert!(
            !check("key = 'order-17'", json),
            "the key compares with case"
        );
        assert!(check("key CONTAINS 'order'", json), "contains ignores case");
        assert!(check("value contains 'SILVA'", json));
        assert!(check("headers CONTAINS 'web'", json));
        assert!(check("key MATCHES '^Order-[0-9]+$'", json));
        assert!(!check("key MATCHES '^order'", json));
        assert!(check("key MATCHES '(?i)^order'", json));
    }

    #[test]
    fn and_comes_before_or_and_parentheses_change_the_order() {
        let json = r#"{"a": 1, "b": 2}"#;
        assert!(check("value.a = 9 AND value.b = 9 OR value.a = 1", json));
        assert!(!check("value.a = 9 AND (value.b = 9 OR value.a = 1)", json));
        assert!(check("NOT (value.a = 9)", json));
    }

    #[test]
    fn errors_give_the_position() {
        assert_eq!(filter("key MATCHES '('").unwrap_err().offset, Some(12));
        assert_eq!(filter("size = 1").unwrap_err().offset, Some(0));
        assert_eq!(filter("value.a ~ 1").unwrap_err().offset, Some(8));
        assert_eq!(filter("(value.a = 1").unwrap_err().offset, Some(12));
        assert_eq!(filter("key > 'a'").unwrap_err().offset, Some(4));
        assert_eq!(filter("key.a = 'b'").unwrap_err().offset, Some(0));
        assert_eq!(filter("value.a =").unwrap_err().offset, Some(9));
    }

    #[test]
    fn a_value_without_json_matches_only_text_conditions() {
        let message = Message {
            key: None,
            value: Some("plain text"),
            json: None,
            headers: "{}",
        };
        assert!(filter("value CONTAINS 'plain'").unwrap().matches(&message));
        assert!(!filter("value.a = 1").unwrap().matches(&message));
        assert!(!filter("key CONTAINS 'a'").unwrap().matches(&message));
    }
}
