//! The filter of a Kafka read. Kafka does not filter on the server, so SQLN reads each
//! message and keeps the messages that match, after the decoding.

mod evaluation;
mod expr;

pub use self::evaluation::Evaluation;
pub(crate) use self::evaluation::Text;

use std::cmp::Ordering;

use regex::{Regex, RegexBuilder};
use serde_json::{Number, Value as Json};

pub use self::expr::{Context, Expr, Needs, Root, Segment};
use super::command::{Cursor, Token, TokenKind, error_at, parse_time};
use crate::connector::ExecError;
use crate::ui::text;

/// The functions of a filter and of the SELECT columns, for the completion.
pub const FUNCTION_NAMES: [&str; 7] = [
    "CAST", "COALESCE", "LENGTH", "LOWER", "SIZE", "TYPEOF", "UPPER",
];

/// The largest compiled regular expression. A larger one stops the read before it starts.
const REGEX_LIMIT: usize = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operator {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

#[derive(Debug)]
pub enum Condition {
    /// One value of the left side and one value of the right side match.
    Compare {
        left: Expr,
        operator: Operator,
        right: Expr,
    },
    /// `BETWEEN`: one value is in the range, with its ends.
    Between { value: Expr, low: Expr, high: Expr },
    /// `IS NULL` is true for an absent value or null. `IS MISSING` is true only for an
    /// absent value.
    Is { value: Expr, missing: bool },
    /// The text holds this text, without case. The text is in lowercase.
    Contains(Expr, String),
    /// `MATCHES`, `LIKE`, and `ILIKE`.
    Matches(Expr, Regex),
    /// `ANY` and `EVERY`: the items of a list. `EVERY` is true for an empty list.
    Quantified {
        every: bool,
        list: Expr,
        body: Box<Filter>,
    },
}

#[derive(Debug)]
pub enum Filter {
    Condition(Condition),
    Not(Box<Filter>),
    And(Box<Filter>, Box<Filter>),
    Or(Box<Filter>, Box<Filter>),
}

/// The parts of one message that a filter reads. A key or a value without UTF-8 text has
/// no text.
#[derive(Debug, Default)]
pub struct Message<'a> {
    pub partition: i64,
    pub offset: i64,
    /// Milliseconds since 1970, in UTC.
    pub timestamp: Option<i64>,
    pub key: Option<&'a str>,
    /// The key as JSON, when the filter reads a field of the key.
    pub key_json: Option<&'a Json>,
    pub value: Option<&'a str>,
    /// The decoded value. `None` for a value without JSON.
    pub json: Option<&'a Json>,
    pub headers: Option<&'a str>,
    pub headers_json: Option<&'a Json>,
    pub schema: Option<&'a str>,
}

/// Reads a filter. It stops before a word that does not continue the filter, such as
/// `LIMIT`, so the next clause can follow.
pub fn parse(cursor: &mut Cursor<'_>) -> Result<Filter, ExecError> {
    parse_or(cursor, &mut Vec::new())
}

/// Reads one value of a SELECT column.
pub fn expression(cursor: &mut Cursor<'_>) -> Result<Expr, ExecError> {
    expr::parse(cursor, &[])
}

fn parse_or(cursor: &mut Cursor<'_>, vars: &mut Vec<String>) -> Result<Filter, ExecError> {
    let mut left = parse_and(cursor, vars)?;
    while cursor.peek().is_some_and(|token| token.is_word("OR")) {
        cursor.advance();
        let right = parse_and(cursor, vars)?;
        left = Filter::Or(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_and(cursor: &mut Cursor<'_>, vars: &mut Vec<String>) -> Result<Filter, ExecError> {
    let mut left = parse_not(cursor, vars)?;
    while cursor.peek().is_some_and(|token| token.is_word("AND")) {
        cursor.advance();
        let right = parse_not(cursor, vars)?;
        left = Filter::And(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_not(cursor: &mut Cursor<'_>, vars: &mut Vec<String>) -> Result<Filter, ExecError> {
    if cursor.peek().is_some_and(|token| token.is_word("NOT")) {
        cursor.advance();
        return Ok(Filter::Not(Box::new(parse_not(cursor, vars)?)));
    }
    if cursor.peek().is_some_and(|token| token.is_operator("(")) && !value_group(cursor) {
        cursor.advance();
        let inner = parse_or(cursor, vars)?;
        expr::close(cursor)?;
        return Ok(inner);
    }
    parse_condition(cursor, vars)
}

/// Returns true when the parenthesis at the cursor holds a value, as in
/// `(value.a + 1) > 2`, and not a group of conditions.
fn value_group(cursor: &Cursor<'_>) -> bool {
    let mut depth = 0usize;
    let mut index = 0;
    while let Some(token) = cursor.peek_nth(index) {
        if token.is_operator("(") {
            depth += 1;
        } else if token.is_operator(")") {
            depth -= 1;
            if depth == 0 {
                return cursor.peek_nth(index + 1).is_some_and(continues_value);
            }
        }
        index += 1;
    }
    false
}

/// Returns true when the token can come after a value in a condition.
fn continues_value(token: &Token) -> bool {
    comparison(token).is_some()
        || expr::is_op(token)
        || [
            "IS", "IN", "NOT", "BETWEEN", "LIKE", "ILIKE", "CONTAINS", "MATCHES",
        ]
        .iter()
        .any(|word| token.is_word(word))
}

fn comparison(token: &Token) -> Option<Operator> {
    if token.kind != TokenKind::Operator {
        return None;
    }
    Some(match token.text.as_str() {
        "=" => Operator::Equal,
        "!=" => Operator::NotEqual,
        "<" => Operator::Less,
        "<=" => Operator::LessOrEqual,
        ">" => Operator::Greater,
        ">=" => Operator::GreaterOrEqual,
        _ => return None,
    })
}

/// Reads a text between quotes after `CONTAINS`, `MATCHES`, or `LIKE`.
fn text_operand(cursor: &mut Cursor<'_>) -> Result<(String, usize), ExecError> {
    let offset = cursor.offset();
    match cursor.advance() {
        Some(token) if token.kind == TokenKind::Text => Ok((token.text.clone(), offset)),
        _ => Err(error_at(text::KAFKA_NEEDS_TEXT, offset)),
    }
}

/// Reads an expression and checks it against the other side of the condition. A text
/// next to `timestamp` becomes a time. A value that gives only numbers refuses a text,
/// `true`, `false`, and `null`.
fn operand(
    cursor: &mut Cursor<'_>,
    vars: &[String],
    other: &Expr,
) -> Result<(Expr, usize), ExecError> {
    let offset = cursor.offset();
    let value = expr::parse(cursor, vars)?;
    Ok((check_operand(other, value, offset)?, offset))
}

fn check_operand(other: &Expr, value: Expr, offset: usize) -> Result<Expr, ExecError> {
    match value {
        Expr::Literal(Json::String(time)) if other.is_timestamp() => parse_time(&time)
            .map(|time| Expr::Literal(Json::from(time)))
            .ok_or_else(|| error_at(text::KAFKA_BAD_TIME, offset)),
        Expr::Literal(ref json) if other.number_only() && !json.is_number() => {
            Err(error_at(text::KAFKA_NEEDS_NUMBER, offset))
        }
        value => Ok(value),
    }
}

/// Reads a field that holds a list, after `IN` or in `ANY`.
fn list_field(cursor: &mut Cursor<'_>, vars: &[String]) -> Result<Expr, ExecError> {
    let offset = cursor.offset();
    let bad = || error_at(text::KAFKA_NEEDS_LIST, offset);
    let token = cursor.advance().ok_or_else(bad)?;
    match expr::field(token, vars).map_err(|_| bad())? {
        Expr::Field(root, _) if !root.has_json() => Err(bad()),
        field => Ok(field),
    }
}

/// Builds a regular expression with the size limit.
fn regex(source: &str, ignore_case: bool) -> Option<Regex> {
    RegexBuilder::new(source)
        .case_insensitive(ignore_case)
        .dot_matches_new_line(true)
        .size_limit(REGEX_LIMIT)
        .dfa_size_limit(REGEX_LIMIT)
        .build()
        .ok()
}

/// Converts a LIKE pattern: `%` is any text, `_` is one character, and `\` makes the
/// next character literal.
fn like_regex(pattern: &str, ignore_case: bool) -> Result<Regex, &'static str> {
    let mut source = String::from("^");
    let mut chars = pattern.chars();
    while let Some(char) = chars.next() {
        match char {
            '%' => source.push_str(".*"),
            '_' => source.push('.'),
            '\\' => {
                let next = chars.next().ok_or(text::KAFKA_LIKE_END)?;
                source.push_str(&regex::escape(next.encode_utf8(&mut [0; 4])));
            }
            other => source.push_str(&regex::escape(other.encode_utf8(&mut [0; 4]))),
        }
    }
    source.push('$');
    regex(&source, ignore_case).ok_or(text::KAFKA_BAD_REGEX)
}

/// Returns true when the name can name the item of `ANY` or `EVERY`.
fn item_name(name: &str, vars: &[String]) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && chars.all(|char| char.is_alphanumeric() || char == '_')
        && Root::column(name).is_none()
        && !["true", "false", "null"]
            .iter()
            .any(|word| word.eq_ignore_ascii_case(name))
        && !vars.iter().any(|var| var.eq_ignore_ascii_case(name))
}

/// Reads `ANY name IN field SATISFIES condition END`, or `EVERY` in place of `ANY`.
fn quantified(cursor: &mut Cursor<'_>, vars: &mut Vec<String>) -> Result<Filter, ExecError> {
    let every = cursor.advance().is_some_and(|token| token.is_word("EVERY"));
    let offset = cursor.offset();
    let name = cursor
        .advance()
        .map(|token| token.text.clone())
        .unwrap_or_default();
    if !item_name(&name, vars) {
        return Err(error_at(text::KAFKA_ITEM_NAME, offset));
    }
    let expect = |cursor: &mut Cursor<'_>, word: &str| {
        let offset = cursor.offset();
        if cursor.advance().is_some_and(|token| token.is_word(word)) {
            Ok(())
        } else {
            Err(error_at(text::KAFKA_QUANTIFIER, offset))
        }
    };
    expect(cursor, "IN")?;
    let list = list_field(cursor, vars)?;
    expect(cursor, "SATISFIES")?;
    vars.push(name);
    let body = parse_or(cursor, vars);
    vars.pop();
    let body = body?;
    expect(cursor, "END")?;
    Ok(Filter::Condition(Condition::Quantified {
        every,
        list,
        body: Box::new(body),
    }))
}

fn parse_condition(cursor: &mut Cursor<'_>, vars: &mut Vec<String>) -> Result<Filter, ExecError> {
    if cursor
        .peek()
        .is_some_and(|token| token.is_word("ANY") || token.is_word("EVERY"))
        && cursor
            .peek_nth(1)
            .is_some_and(|token| token.kind == TokenKind::Word)
    {
        return quantified(cursor, vars);
    }
    let start = cursor.offset();
    let left = expr::parse(cursor, vars)?;
    let offset = cursor.offset();
    let token = cursor
        .advance()
        .ok_or_else(|| error_at(text::KAFKA_NEEDS_OPERATOR, offset))?;
    if let Some(operator) = comparison(token) {
        let (right, _) = operand(cursor, vars, &left)?;
        let left = check_operand(&right, left, start)?;
        return Ok(Filter::Condition(Condition::Compare {
            left,
            operator,
            right,
        }));
    }
    if token.is_word("IS") {
        let negated = cursor.peek().is_some_and(|token| token.is_word("NOT"));
        cursor.index += usize::from(negated);
        let at = cursor.offset();
        let missing = match cursor.advance() {
            Some(token) if token.is_word("NULL") => false,
            Some(token) if token.is_word("MISSING") => true,
            _ => return Err(error_at(text::KAFKA_NEEDS_NULL, at)),
        };
        return Ok(negate(
            negated,
            Filter::Condition(Condition::Is {
                value: left,
                missing,
            }),
        ));
    }
    let negated = token.is_word("NOT");
    let (token, offset) = if negated {
        let offset = cursor.offset();
        let token = cursor
            .advance()
            .ok_or_else(|| error_at(text::KAFKA_NEEDS_OPERATOR, offset))?;
        (token, offset)
    } else {
        (token, offset)
    };
    let word = token.text.to_ascii_uppercase();
    let text_word = ["CONTAINS", "MATCHES", "LIKE", "ILIKE"].contains(&word.as_str());
    if token.kind != TokenKind::Word || text_word && left.number_only() {
        let message = if text_word {
            text::KAFKA_TEXT_OPERATOR
        } else {
            text::KAFKA_NEEDS_OPERATOR
        };
        return Err(error_at(message, offset));
    }
    let filter = match word.as_str() {
        "IN" => in_condition(cursor, vars, left, start)?,
        "BETWEEN" => {
            let (low, _) = operand(cursor, vars, &left)?;
            let at = cursor.offset();
            if !cursor.advance().is_some_and(|token| token.is_word("AND")) {
                return Err(error_at(text::KAFKA_NEEDS_AND, at));
            }
            let (high, _) = operand(cursor, vars, &left)?;
            Filter::Condition(Condition::Between {
                value: left,
                low,
                high,
            })
        }
        "CONTAINS" => {
            let (needle, _) = text_operand(cursor)?;
            Filter::Condition(Condition::Contains(left, needle.to_lowercase()))
        }
        "MATCHES" => {
            let (pattern, at) = text_operand(cursor)?;
            let regex =
                regex(&pattern, false).ok_or_else(|| error_at(text::KAFKA_BAD_REGEX, at))?;
            Filter::Condition(Condition::Matches(left, regex))
        }
        "LIKE" | "ILIKE" => {
            let (pattern, at) = text_operand(cursor)?;
            let regex =
                like_regex(&pattern, word == "ILIKE").map_err(|message| error_at(message, at))?;
            Filter::Condition(Condition::Matches(left, regex))
        }
        _ => return Err(error_at(text::KAFKA_NEEDS_OPERATOR, offset)),
    };
    Ok(negate(negated, filter))
}

fn negate(negated: bool, filter: Filter) -> Filter {
    if negated {
        Filter::Not(Box::new(filter))
    } else {
        filter
    }
}

/// Reads the rest of `IN`: a list of values between parentheses, or a field that holds
/// a list. `x IN (a, b)` is `x = a OR x = b`.
fn in_condition(
    cursor: &mut Cursor<'_>,
    vars: &[String],
    left: Expr,
    start: usize,
) -> Result<Filter, ExecError> {
    let compare = |left: Expr, right: Expr| {
        Filter::Condition(Condition::Compare {
            left,
            operator: Operator::Equal,
            right,
        })
    };
    if !cursor.peek().is_some_and(|token| token.is_operator("(")) {
        let Expr::Field(root, mut path) = list_field(cursor, vars)? else {
            unreachable!("a list field is a field");
        };
        path.push(Segment::Any);
        return Ok(compare(left, Expr::Field(root, path)));
    }
    cursor.advance();
    let mut filter: Option<Filter> = None;
    loop {
        let (right, _) = operand(cursor, vars, &left)?;
        let checked_left = check_operand(&right, left.clone(), start)?;
        let next = compare(checked_left, right);
        filter = Some(match filter {
            None => next,
            Some(filter) => Filter::Or(Box::new(filter), Box::new(next)),
        });
        if !cursor.peek().is_some_and(|token| token.is_operator(",")) {
            break;
        }
        cursor.advance();
    }
    expr::close(cursor)?;
    Ok(filter.expect("the list has one value or more"))
}

impl Filter {
    /// Returns the parts of the message that need JSON.
    pub fn needs(&self) -> Needs {
        let mut needs = Needs::default();
        self.add_needs(&mut needs);
        needs
    }

    fn add_needs(&self, needs: &mut Needs) {
        match self {
            Filter::Condition(condition) => condition.add_needs(needs),
            Filter::Not(inner) => inner.add_needs(needs),
            Filter::And(left, right) | Filter::Or(left, right) => {
                left.add_needs(needs);
                right.add_needs(needs);
            }
        }
    }

    pub fn matches(
        &self,
        message: &Message<'_>,
        evaluation: &Evaluation,
    ) -> Result<bool, ExecError> {
        self.matches_in(Context {
            message,
            scope: None,
            evaluation,
        })
    }

    fn matches_in(&self, context: Context<'_>) -> Result<bool, ExecError> {
        context.evaluation.check()?;
        match self {
            Filter::Condition(condition) => condition.matches(context),
            Filter::Not(inner) => Ok(!inner.matches_in(context)?),
            Filter::And(left, right) => Ok(left.matches_in(context)? && right.matches_in(context)?),
            Filter::Or(left, right) => Ok(left.matches_in(context)? || right.matches_in(context)?),
        }
    }
}

fn number_order(left: &Number, right: &Number) -> Option<Ordering> {
    if let (Some(left), Some(right)) = (left.as_i64(), right.as_i64()) {
        return Some(left.cmp(&right));
    }
    if let (Some(left), Some(right)) = (left.as_u64(), right.as_u64()) {
        return Some(left.cmp(&right));
    }
    left.as_f64()?.partial_cmp(&right.as_f64()?)
}

fn holds(order: Ordering, operator: Operator) -> bool {
    match operator {
        Operator::Equal => order == Ordering::Equal,
        Operator::NotEqual => order != Ordering::Equal,
        Operator::Less => order == Ordering::Less,
        Operator::LessOrEqual => order != Ordering::Greater,
        Operator::Greater => order == Ordering::Greater,
        Operator::GreaterOrEqual => order != Ordering::Less,
    }
}

/// Compares two values of the same type. `null` equals only `null` and has no order.
/// Lists and objects have only `=` and `!=`. Other types do not match.
fn compare(left: &Json, operator: Operator, right: &Json) -> bool {
    let equality = |same: bool| match operator {
        Operator::Equal => same,
        Operator::NotEqual => !same,
        _ => false,
    };
    let order = match (left, right) {
        (Json::Null, _) | (_, Json::Null) => return equality(left.is_null() && right.is_null()),
        (Json::Array(_), Json::Array(_)) | (Json::Object(_), Json::Object(_)) => {
            return equality(left == right);
        }
        (Json::Number(left), Json::Number(right)) => number_order(left, right),
        (Json::String(left), Json::String(right)) => Some(left.cmp(right)),
        (Json::Bool(left), Json::Bool(right)) => Some(left.cmp(right)),
        _ => None,
    };
    order.is_some_and(|order| holds(order, operator))
}

/// Checks text without collecting expression results.
fn texts(
    value: &Expr,
    context: Context<'_>,
    check: &mut dyn FnMut(&str) -> Result<bool, ExecError>,
) -> Result<bool, ExecError> {
    let message = context.message;
    let whole = match value.bare_part() {
        Some(Root::Key) => message.key,
        Some(Root::Value) => message.value,
        Some(_) => message.headers,
        None => {
            return value.visit(context, &mut |value| match value {
                Json::String(value) => check(value),
                _ => Ok(false),
            });
        }
    };
    match whole {
        Some(text) => check(text),
        None => Ok(false),
    }
}

impl Condition {
    fn add_needs(&self, needs: &mut Needs) {
        match self {
            Condition::Compare { left, right, .. } => {
                left.needs(needs);
                right.needs(needs);
            }
            Condition::Between { value, low, high } => {
                for value in [value, low, high] {
                    value.needs(needs);
                }
            }
            Condition::Is { value, .. } => value.needs(needs),
            Condition::Contains(value, _) | Condition::Matches(value, _) => {
                if value.bare_part().is_none() {
                    value.needs(needs);
                }
            }
            Condition::Quantified { list, body, .. } => {
                list.needs(needs);
                body.add_needs(needs);
            }
        }
    }

    fn matches(&self, context: Context<'_>) -> Result<bool, ExecError> {
        match self {
            Condition::Compare {
                left,
                operator,
                right,
            } => left.visit(context, &mut |left| {
                right.visit(context, &mut |right| Ok(compare(left, *operator, right)))
            }),
            Condition::Between { value, low, high } => value.visit(context, &mut |value| {
                Ok(low.visit(context, &mut |low| {
                    Ok(compare(value, Operator::GreaterOrEqual, low))
                })? && high.visit(context, &mut |high| {
                    Ok(compare(value, Operator::LessOrEqual, high))
                })?)
            }),
            Condition::Is { value, missing } => {
                let mut found = false;
                let null = value.visit(context, &mut |value| {
                    found = true;
                    Ok(*missing || value.is_null())
                })?;
                Ok(!found || !missing && null)
            }
            Condition::Contains(value, needle) => texts(value, context, &mut |value| {
                let (lower, _memory) = context.evaluation.case(value, false)?;
                Ok(lower.contains(needle.as_str()))
            }),
            Condition::Matches(value, regex) => texts(value, context, &mut |value| {
                context.evaluation.check()?;
                Ok(regex.is_match(value))
            }),
            Condition::Quantified { every, list, body } => {
                // Validate all roots first. A later non-list still makes the condition false.
                let mut found = false;
                let invalid = list.visit(context, &mut |value| {
                    found = true;
                    Ok(!value.is_array())
                })?;
                if invalid || !found {
                    return Ok(false);
                }
                let depth = context.scope.map_or(0, |scope| scope.depth + 1);
                let decisive = list.visit(context, &mut |value| {
                    let Json::Array(items) = value else {
                        unreachable!("validated list")
                    };
                    for item in items {
                        context.evaluation.check()?;
                        let scope = expr::Scope {
                            item,
                            depth,
                            parent: context.scope,
                        };
                        let matches = body.matches_in(Context {
                            message: context.message,
                            scope: Some(&scope),
                            evaluation: context.evaluation,
                        })?;
                        if matches != *every {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                })?;
                Ok(if *every { !decisive } else { decisive })
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

    const HEADERS: &str = r#"{"source":"web","tag":["a","b"]}"#;

    fn check(text: &str, json: &str) -> bool {
        let json: Json = serde_json::from_str(json).unwrap();
        let headers: Json = serde_json::from_str(HEADERS).unwrap();
        let value = json.to_string();
        let message = Message {
            partition: 3,
            offset: 42,
            timestamp: Some(1_767_312_000_000),
            key: Some("Order-17"),
            value: Some(&value),
            json: Some(&json),
            headers: Some(HEADERS),
            headers_json: Some(&headers),
            schema: Some("value 7 avro"),
            ..Message::default()
        };
        filter(text)
            .unwrap()
            .matches(&message, &Evaluation::default())
            .unwrap()
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
        assert!(!check("value.missing = null", json));
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
        assert_eq!(filter("partition > 'a'").unwrap_err().offset, Some(12));
        assert_eq!(filter("partition.a = 1").unwrap_err().offset, Some(0));
        assert_eq!(filter("value.a =").unwrap_err().offset, Some(9));
    }

    #[test]
    fn brackets_read_an_item_and_a_negative_index_counts_from_the_end() {
        let json = r#"{"items": [{"sku": "a"}, {"sku": "b"}, {"sku": "c"}]}"#;
        assert!(check("value.items[0].sku = 'a'", json));
        assert!(check("value.items.0.sku = 'a'", json), "the dot form stays");
        assert!(check("value.items[-1].sku = 'c'", json));
        assert!(check("value.items[-3].sku = 'a'", json));
        assert!(!check("value.items[3].sku = 'a'", json));
        assert!(!check("value.items[-4].sku != 'a'", json));
        assert!(!check("value.items[0] = 1", r#"{"items": {"0": 1}}"#));
        assert!(check("value[1] = 2", "[1, 2]"));
    }

    #[test]
    fn any_item_matches_when_one_item_matches() {
        let json = r#"{"items": [{"sku": "a", "n": 1}, {"sku": "b", "n": 5}], "empty": []}"#;
        assert!(check("value.items[*].sku = 'b'", json));
        assert!(!check("value.items[*].sku = 'z'", json));
        assert!(check("value.items[*].sku != 'a'", json), "one item differs");
        assert!(!check("NOT value.items[*].sku = 'a'", json));
        assert!(check("NOT value.items[*].sku = 'z'", json));
        assert!(check("value.items[*].n > 4", json));
        assert!(check("value.items[*].n <= 1", json));
        assert!(!check("value.items[*].n >= 6", json));
        assert!(!check("value.empty[*] = 1", json));
        assert!(
            !check("value.items[0].sku[*] = 'a'", json),
            "a text has no items"
        );
        let nested = r#"{"orders": [{"tags": ["x"]}, {"tags": ["y", "vip"]}]}"#;
        assert!(check("value.orders[*].tags[*] = 'vip'", nested));
        assert!(check("value.orders[-1].tags[*] = 'y'", nested));
    }

    #[test]
    fn in_finds_an_item_of_the_same_type() {
        let json = r#"{"tags": ["vip", "eu"], "codes": [7, null, true], "name": "vip"}"#;
        assert!(check("'vip' IN value.tags", json));
        assert!(!check("'VIP' IN value.tags", json), "IN compares with case");
        assert!(check("7 IN value.codes", json));
        assert!(!check("'7' IN value.codes", json), "a text is not a number");
        assert!(check("null IN value.codes", json));
        assert!(check("true IN value.codes", json));
        assert!(!check("false IN value.codes", json));
        assert!(!check("'vip' IN value.name", json), "a text is not a list");
        assert!(!check("'vip' IN value.missing", json));
        assert!(check("'a' IN value", r#"["a"]"#));
        assert!(check("NOT 'us' IN value.tags AND 'eu' IN value.tags", json));
        assert!(check("value.name IN value.tags", json));
        assert!(check("'b' IN headers.tag", json));
    }

    #[test]
    fn size_counts_the_items_of_a_list() {
        let json = r#"{"items": [1, 2, 3], "empty": [], "name": "abc", "o": {"a": 1}}"#;
        assert!(check("SIZE(value.items) = 3", json));
        assert!(check("size(value.items) != 2", json));
        assert!(check("SIZE(value.items) > 2", json));
        assert!(check("SIZE(value.items) >= 3", json));
        assert!(check("SIZE(value.items) < 4", json));
        assert!(check("SIZE(value.items) <= 3", json));
        assert!(check("SIZE(value.empty) = 0", json));
        assert!(!check("SIZE(value.name) = 3", json), "a text is not a list");
        assert!(!check("SIZE(value.o) = 1", json));
        assert!(!check("SIZE(value.missing) = 0", json));
        assert!(!check("SIZE(value.missing) != 0", json));
        assert!(check("SIZE(value) = 2", "[1, 2]"));
        assert!(check("(SIZE(value.items) = 3)", json));
    }

    #[test]
    fn array_errors_give_the_position() {
        assert_eq!(filter("SIZE(value.a[*]) = 1").unwrap_err().offset, Some(5));
        assert_eq!(filter("SIZE value.a = 1").unwrap_err().offset, Some(0));
        assert_eq!(filter("SIZE(value.a = 1").unwrap_err().offset, Some(13));
        assert_eq!(filter("SIZE(value.a) = 'x'").unwrap_err().offset, Some(16));
        assert_eq!(
            filter("SIZE(value.a) CONTAINS 'x'").unwrap_err().offset,
            Some(14)
        );
        assert_eq!(filter("value.a[x] = 1").unwrap_err().offset, Some(0));
        assert_eq!(filter("value.a[1 = 1").unwrap_err().offset, Some(0));
        assert_eq!(filter("value.a[] = 1").unwrap_err().offset, Some(0));
        assert_eq!(filter("'a' IN partition").unwrap_err().offset, Some(7));
        assert_eq!(filter("1 IN").unwrap_err().offset, Some(4));
        assert!(filter("SIZE(value.a) = 1").unwrap().needs().value);
        assert!(filter("'a' in value.b").unwrap().needs().value);
    }

    #[test]
    fn a_value_without_json_matches_only_text_conditions() {
        let message = Message {
            value: Some("plain text"),
            headers: Some("{}"),
            ..Message::default()
        };
        assert!(
            filter("value CONTAINS 'plain'")
                .unwrap()
                .matches(&message, &Evaluation::default())
                .unwrap()
        );
        assert!(
            !filter("value.a = 1")
                .unwrap()
                .matches(&message, &Evaluation::default())
                .unwrap()
        );
        assert!(
            !filter("'a' IN value")
                .unwrap()
                .matches(&message, &Evaluation::default())
                .unwrap()
        );
        assert!(
            !filter("SIZE(value) = 0")
                .unwrap()
                .matches(&message, &Evaluation::default())
                .unwrap()
        );
        assert!(
            !filter("key CONTAINS 'a'")
                .unwrap()
                .matches(&message, &Evaluation::default())
                .unwrap()
        );
        assert!(
            filter("value LIKE 'plain%'")
                .unwrap()
                .matches(&message, &Evaluation::default())
                .unwrap()
        );
        assert!(
            filter("value IS MISSING")
                .unwrap()
                .matches(&message, &Evaluation::default())
                .unwrap()
        );
    }

    #[test]
    fn text_operators_read_a_text_field() {
        let json = r#"{"email": "Ana@X.com", "code": "100%", "n": 5, "tags": ["red", "blue"]}"#;
        assert!(check("value.email CONTAINS 'ana@'", json));
        assert!(check(r"value.email MATCHES '@X\\.com$'", json));
        assert!(check("value.email LIKE 'Ana@%'", json));
        assert!(
            !check("value.email LIKE 'ana@%'", json),
            "LIKE keeps the case"
        );
        assert!(check("value.email ILIKE 'ana@%.COM'", json));
        assert!(check("value.email LIKE '___@X.com'", json));
        assert!(!check("value.email LIKE '__@X.com'", json));
        assert!(check("value.email NOT LIKE '%.org'", json));
        assert!(check(r"value.code LIKE '100\\%'", json));
        assert!(!check(r"value.code LIKE '10\\%'", json));
        assert!(check("value.tags[*] LIKE 'bl%'", json));
        assert!(!check("value.n LIKE '5'", json), "a number is not a text");
        assert!(!check("value.missing CONTAINS 'a'", json));
        assert!(check("LOWER(value.email) = 'ana@x.com'", json));
        assert!(check("schema LIKE 'value %'", json));
    }

    #[test]
    fn is_null_also_finds_an_absent_field_and_is_missing_only_an_absent_field() {
        let json = r#"{"note": null, "name": "Ana"}"#;
        assert!(check("value.note IS NULL", json));
        assert!(check("value.missing IS NULL", json));
        assert!(!check("value.name IS NULL", json));
        assert!(check("value.name IS NOT NULL", json));
        assert!(check("value.missing IS MISSING", json));
        assert!(!check("value.note IS MISSING", json));
        assert!(check("value.note IS NOT MISSING", json));
        assert!(check("key IS NOT NULL", json));
    }

    #[test]
    fn in_lists_and_between_compare_with_each_value() {
        let json = r#"{"status": "paid", "total": 15, "items": [{"n": 0}, {"n": 10}]}"#;
        assert!(check("value.status IN ('new', 'paid')", json));
        assert!(!check("value.status IN ('new')", json));
        assert!(check("value.status NOT IN ('new', 'lost')", json));
        assert!(check("value.total IN (1, 15, 'x')", json));
        assert!(check("value.total BETWEEN 10 AND 20", json));
        assert!(check("value.total BETWEEN 15 AND 15", json));
        assert!(!check("value.total BETWEEN 16 AND 20", json));
        assert!(check("value.total NOT BETWEEN 16 AND 20", json));
        assert!(
            !check("value.items[*].n BETWEEN 1 AND 5", json),
            "one item must be in the range"
        );
        assert!(check(
            "value.total BETWEEN 10 AND 20 AND value.status = 'paid'",
            json
        ));
    }

    #[test]
    fn in_lists_convert_timestamp_text_on_either_side_of_each_comparison() {
        for (condition, expected) in [
            ("'2026-01-02' IN (timestamp)", true),
            ("'2026-01-02' NOT IN (timestamp)", false),
            ("'2026-01-03' IN (timestamp)", false),
            ("'2026-01-02' IN ('other', timestamp)", true),
            ("timestamp IN ('2026-01-01', '2026-01-02')", true),
        ] {
            assert_eq!(check(condition, "{}"), expected, "{condition}");
        }
        let message = Message::default();
        assert!(
            filter("'2026-01-02' IN (timestamp, '2026-01-02')")
                .unwrap()
                .matches(&message, &Evaluation::default())
                .unwrap()
        );
    }

    #[test]
    fn in_lists_refuse_invalid_left_operands_before_evaluation() {
        for (condition, message) in [
            ("'never' IN (timestamp)", text::KAFKA_BAD_TIME),
            ("'text' IN (offset)", text::KAFKA_NEEDS_NUMBER),
            ("true IN (partition)", text::KAFKA_NEEDS_NUMBER),
        ] {
            let error = filter(condition).unwrap_err();
            assert_eq!(error.offset, Some(0), "{condition}");
            assert_eq!(error.message, message, "{condition}");
        }
    }

    #[test]
    fn a_field_compares_with_another_field_on_either_side() {
        let json = r#"{"paid": 15, "total": 20, "a": {"x": [1]}, "b": {"x": [1]}, "c": [2]}"#;
        assert!(check("value.paid < value.total", json));
        assert!(check("20 = value.total", json));
        assert!(check("value.a = value.b", json), "objects compare whole");
        assert!(check("value.a.x != value.c", json));
        assert!(!check("value.a < value.b", json), "objects have no order");
    }

    #[test]
    fn arithmetic_and_functions_give_values() {
        let json = r#"{"price": 2.5, "qty": 4, "zero": 0, "id": "17", "big": 9007199254740993,
            "name": "Ana", "nick": null, "ok": "TRUE", "list": [1, 2]}"#;
        assert!(check("value.price * value.qty = 10", json));
        assert!(
            check("value.qty + value.qty * 2 = 12", json),
            "* comes first"
        );
        assert!(check("(value.qty + value.qty) * 2 = 16", json));
        assert!(check("(value.qty + 1) > 4", json));
        assert!(check("value.qty / 8 = 0.5", json));
        assert!(check("value.qty % 3 = 1", json));
        assert!(check("value.qty - 5 = -1", json));
        assert!(!check("value.qty / value.zero = 0", json), "no value");
        assert!(check("value.qty / value.zero IS NULL", json));
        assert!(check("value.big + 0 = 9007199254740993", json));
        assert!(!check("value.big = 9007199254740992", json));
        assert!(check("CAST(value.id AS NUMBER) = 17", json));
        assert!(check("CAST(value.qty AS TEXT) = '4'", json));
        assert!(check("CAST(value.ok AS BOOLEAN) = true", json));
        assert!(check("CAST(value.name AS NUMBER) IS MISSING", json));
        assert!(check("CAST(value.nick AS NUMBER) IS NOT MISSING", json));
        assert!(check(
            "COALESCE(value.nick, value.missing, value.name) = 'Ana'",
            json
        ));
        assert!(check("UPPER(value.name) = 'ANA'", json));
        assert!(check("LENGTH(value.name) = 3", json));
        assert!(check("TYPEOF(value.list) = 'array'", json));
        assert!(check("TYPEOF(value.missing) IS NULL", json));
        assert!(check("value.name || '-' || value.qty = 'Ana-4'", json));
        assert!(check("value.list[*] * 10 = 20", json));
    }

    #[test]
    fn any_and_every_check_one_item_with_all_its_conditions() {
        let json = r#"{"items": [{"sku": "a", "qty": 1}, {"sku": "b", "qty": 5}], "empty": [],
            "orders": [{"tags": ["x"]}, {"tags": ["vip"]}], "name": "Ana"}"#;
        assert!(check(
            "value.items[*].sku = 'a' AND value.items[*].qty > 2",
            json
        ));
        assert!(!check(
            "ANY i IN value.items SATISFIES i.sku = 'a' AND i.qty > 2 END",
            json
        ));
        assert!(check(
            "ANY i IN value.items SATISFIES i.sku = 'b' AND i.qty > 2 END",
            json
        ));
        assert!(check(
            "EVERY i IN value.items SATISFIES i.qty >= 1 END",
            json
        ));
        assert!(!check(
            "EVERY i IN value.items SATISFIES i.qty > 1 END",
            json
        ));
        assert!(check("EVERY i IN value.empty SATISFIES i = 1 END", json));
        assert!(!check("ANY i IN value.empty SATISFIES i = 1 END", json));
        assert!(!check("EVERY i IN value.name SATISFIES i = 1 END", json));
        assert!(!check("EVERY i IN value.missing SATISFIES i = 1 END", json));
        assert!(check(
            "ANY o IN value.orders SATISFIES ANY t IN o.tags SATISFIES t = 'vip' END END",
            json
        ));
        assert!(check(
            "ANY i IN value.items SATISFIES i.sku = 'b' END AND value.name = 'Ana'",
            json
        ));
    }

    #[test]
    fn message_columns_and_fields_of_the_key_and_headers_filter() {
        let json = "{}";
        assert!(check("partition = 3 AND offset >= 42", json));
        assert!(check(
            "timestamp >= '2026-01-02' AND timestamp < '2026-01-02T00:00:01Z'",
            json
        ));
        assert!(check(
            "timestamp BETWEEN '2026-01-01' AND '2026-01-03'",
            json
        ));
        assert!(check("headers.source = 'web'", json));
        assert!(check("headers.tag[1] = 'b'", json));
        assert!(check("headers.missing IS MISSING", json));
        assert!(check("key > 'A' AND key != 'x'", json));
        let key: Json = serde_json::from_str(r#"{"id": 9}"#).unwrap();
        let message = Message {
            key: Some(r#"{"id": 9}"#),
            key_json: Some(&key),
            ..Message::default()
        };
        assert!(
            filter("key.id = 9")
                .unwrap()
                .matches(&message, &Evaluation::default())
                .unwrap()
        );
        assert!(filter("key.id = 9").unwrap().needs().key);
        assert!(!filter("key = 'a'").unwrap().needs().key);
        assert!(!filter("value CONTAINS 'a'").unwrap().needs().value);
    }

    #[test]
    fn a_quoted_name_holds_spaces_dots_and_brackets() {
        let json = r#"{"order id": {"a.b": 1, "x[0]": 2, "say \"hi\"": 3}}"#;
        assert!(check(r#"value."order id"."a.b" = 1"#, json));
        assert!(check(r#"value."order id"."x[0]" = 2"#, json));
        assert!(check(r#"value."order id"."say ""hi""" = 3"#, json));
        assert!(filter(r#"value."order id" = "order id""#).is_ok());
    }

    #[test]
    fn new_errors_give_the_position() {
        assert_eq!(filter("timestamp > 'never'").unwrap_err().offset, Some(12));
        assert_eq!(filter("value.a IS EMPTY").unwrap_err().offset, Some(11));
        assert_eq!(
            filter("value.a BETWEEN 1 OR 2").unwrap_err().offset,
            Some(18)
        );
        assert_eq!(filter(r"value.a LIKE 'a\\'").unwrap_err().offset, Some(13));
        assert_eq!(filter("value.a IN (1, 2").unwrap_err().offset, Some(16));
        assert_eq!(
            filter("CAST(value.a AS DATE) = 1").unwrap_err().offset,
            Some(16)
        );
        assert_eq!(
            filter("LOWER(value.a, 'b') = 1").unwrap_err().offset,
            Some(0)
        );
        assert_eq!(filter("COALESCE() = 1").unwrap_err().offset, Some(0));
        assert_eq!(filter("count(value.a) = 1").unwrap_err().offset, Some(0));
        assert_eq!(
            filter("LENGTH(value.a) LIKE 'x'").unwrap_err().offset,
            Some(16)
        );
        assert_eq!(
            filter("ANY value IN value.a SATISFIES 1 = 1 END")
                .unwrap_err()
                .offset,
            Some(4)
        );
        assert_eq!(
            filter("ANY i IN value.a SATISFIES ANY i IN i.b SATISFIES 1 = 1 END END")
                .unwrap_err()
                .offset,
            Some(31)
        );
        assert_eq!(filter("ANY i value.a").unwrap_err().offset, Some(6));
        assert_eq!(
            filter("ANY i IN value.a SATISFIES i = 1")
                .unwrap_err()
                .offset,
            Some(32)
        );
        assert_eq!(
            filter("ANY i IN offset SATISFIES i = 1 END")
                .unwrap_err()
                .offset,
            Some(9)
        );
        assert_eq!(filter("i.a = 1").unwrap_err().offset, Some(0));
        assert!(tokenize(r#"value."a = 1"#).is_err());
    }

    #[test]
    fn cartesian_filters_stop_at_the_first_matching_pair_without_collecting_values() {
        let json = serde_json::json!({"items": vec![1; 10_000]});
        let message = Message {
            json: Some(&json),
            ..Message::default()
        };
        let evaluation = Evaluation::new(tokio_util::sync::CancellationToken::new(), 64);
        for condition in [
            "value.items[*] + value.items[*] = 2",
            "value.items[*] + value.items[*] BETWEEN 2 AND 3",
            "value.items[*] + value.items[*] IS NOT MISSING",
            "ANY i IN value.items SATISFIES i + value.items[*] = 2 END",
        ] {
            assert!(
                filter(condition)
                    .unwrap()
                    .matches(&message, &evaluation)
                    .unwrap(),
                "{condition}"
            );
            assert_eq!(evaluation.used(), 0);
        }
    }

    #[test]
    fn lazy_coalesce_keeps_all_non_null_values_of_the_first_present_argument() {
        let json = r#"{"a":[null,1,2],"b":[3],"lists":[[1],42]}"#;
        assert!(check("COALESCE(value.a[*], value.b[*]) = 2", json));
        assert!(!check("COALESCE(value.a[*], value.b[*]) = 3", json));
        assert!(!check("ANY i IN value.lists[*] SATISFIES i = 1 END", json));
        assert!(!check(
            "EVERY i IN value.lists[*] SATISFIES i = 1 END",
            json
        ));
        assert!(check("LOWER('ΟΣ') = 'ος'", json));
        assert!(check("'ΟΣ' CONTAINS 'ος'", json));
    }

    #[test]
    fn intermediate_text_buffers_refuse_the_limit_and_release_their_reservations() {
        let json = serde_json::json!({"s": "a".repeat(64), "obj": {"s": "b".repeat(64)}});
        let message = Message {
            json: Some(&json),
            ..Message::default()
        };
        let evaluation = Evaluation::new(tokio_util::sync::CancellationToken::new(), 16);
        for condition in [
            "value.s || value.s = 'a'",
            "LOWER(value.s) = 'a'",
            "value.s CONTAINS 'a'",
            "CAST(value.obj AS TEXT) = 'a'",
            "LOWER(value.s || 'x') = 'a'",
        ] {
            let error = filter(condition)
                .unwrap()
                .matches(&message, &evaluation)
                .unwrap_err();
            assert!(
                error.message.contains("temporary memory limit"),
                "{condition}: {error:?}"
            );
            assert_eq!(evaluation.used(), 0, "{condition}");
        }
        assert!(
            filter("value.s = value.s")
                .unwrap()
                .matches(&message, &evaluation)
                .unwrap()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancellation_stops_a_cartesian_expression_that_produces_no_values() {
        use std::time::Duration;
        let stop = tokio_util::sync::CancellationToken::new();
        let evaluation = Evaluation::new(stop.clone(), 64);
        let (started, ready) = tokio::sync::oneshot::channel();
        let worker = tokio::task::spawn_blocking(move || {
            let json = serde_json::json!({"items": vec![0; 10_000]});
            let message = Message {
                json: Some(&json),
                ..Message::default()
            };
            let condition = filter("value.items[*] / value.items[*] = 1").unwrap();
            started.send(()).unwrap();
            let result = condition.matches(&message, &evaluation);
            assert_eq!(evaluation.used(), 0);
            result
        });
        ready.await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        stop.cancel();
        let error = tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(error.message.contains("canceled"));
    }

    #[test]
    fn words_with_signs_and_texts_in_double_quotes_keep_their_meaning() {
        let json = r#"{"order-id": 5, "n": -5}"#;
        assert!(check("value.order-id = 5", json));
        assert!(check("value.n = -5", json));
        assert!(check(r#"key = "Order-17""#, json));
    }
}
