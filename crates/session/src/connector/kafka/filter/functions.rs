//! The functions of a Kafka filter and of the SELECT columns. Each function follows the
//! Postgres function with the same name. A strict function gives no value for a wrong
//! type, null, or an absent value.

use chrono::DateTime;
use regex::Regex;
use serde_json::{Number, Value as Json};

use super::evaluation::{Evaluation, Memory, Text};
use super::expr::{Check, Context, Expr, Root, number};
use super::time::{self, Piece};
use super::{Operator, compare, regex};
use crate::connector::ExecError;
use crate::connector::kafka::command::{Token, error_at};
use crate::text;

/// The value that an argument without a value gives to a function that ignores nulls.
static NULL: Json = Json::Null;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Function {
    Lower,
    Upper,
    Length,
    Size,
    Coalesce,
    TypeOf,
    SplitPart,
    Substring,
    Left,
    Right,
    Trim,
    LTrim,
    RTrim,
    Replace,
    RegexpReplace,
    Concat,
    ConcatWs,
    OctetLength,
    Abs,
    Round,
    Floor,
    Ceil,
    Greatest,
    Least,
    NullIf,
    Keys,
    ArrayToString,
    Now,
    ToTimestamp,
    ToTimestampMs,
    Epoch,
    EpochMs,
    ToChar,
}

/// A function with its name and the least and the most values between its parentheses.
pub struct Spec {
    pub name: &'static str,
    pub function: Function,
    pub min: usize,
    pub max: usize,
}

/// The most values of a function without a limit.
pub const MANY: usize = usize::MAX;

const fn spec(name: &'static str, function: Function, min: usize, max: usize) -> Spec {
    Spec {
        name,
        function,
        min,
        max,
    }
}

pub const FUNCTIONS: &[Spec] = &[
    spec("LOWER", Function::Lower, 1, 1),
    spec("UPPER", Function::Upper, 1, 1),
    spec("LENGTH", Function::Length, 1, 1),
    spec("SIZE", Function::Size, 1, 1),
    spec("COALESCE", Function::Coalesce, 1, MANY),
    spec("TYPEOF", Function::TypeOf, 1, 1),
    spec("SPLIT_PART", Function::SplitPart, 3, 3),
    spec("SUBSTRING", Function::Substring, 2, 3),
    spec("LEFT", Function::Left, 2, 2),
    spec("RIGHT", Function::Right, 2, 2),
    spec("TRIM", Function::Trim, 1, 2),
    spec("LTRIM", Function::LTrim, 1, 2),
    spec("RTRIM", Function::RTrim, 1, 2),
    spec("REPLACE", Function::Replace, 3, 3),
    spec("REGEXP_REPLACE", Function::RegexpReplace, 3, 4),
    spec("CONCAT", Function::Concat, 1, MANY),
    spec("CONCAT_WS", Function::ConcatWs, 2, MANY),
    spec("OCTET_LENGTH", Function::OctetLength, 1, 1),
    spec("ABS", Function::Abs, 1, 1),
    spec("ROUND", Function::Round, 1, 2),
    spec("FLOOR", Function::Floor, 1, 1),
    spec("CEIL", Function::Ceil, 1, 1),
    spec("GREATEST", Function::Greatest, 1, MANY),
    spec("LEAST", Function::Least, 1, MANY),
    spec("NULLIF", Function::NullIf, 2, 2),
    spec("KEYS", Function::Keys, 1, 1),
    spec("ARRAY_TO_STRING", Function::ArrayToString, 2, 3),
    spec("NOW", Function::Now, 0, 0),
    spec("TO_TIMESTAMP", Function::ToTimestamp, 1, 1),
    spec("TO_TIMESTAMP_MS", Function::ToTimestampMs, 1, 1),
    spec("EPOCH", Function::Epoch, 1, 1),
    spec("EPOCH_MS", Function::EpochMs, 1, 1),
    spec("TO_CHAR", Function::ToChar, 2, 2),
];

pub fn find(name: &Token) -> Option<&'static Spec> {
    FUNCTIONS.iter().find(|spec| name.is_word(spec.name))
}

/// What SQLN reads from the text arguments before the read starts.
#[derive(Clone, Debug)]
pub enum Prepared {
    /// `SUBSTRING(x, 'regex')`.
    Find(Regex),
    /// `REGEXP_REPLACE`: the replacement uses the syntax of `regex`.
    Replace {
        regex: Regex,
        replacement: String,
        global: bool,
    },
    /// `ARRAY_TO_STRING`: the separator, and the text for a null item.
    Join {
        separator: String,
        null: Option<String>,
    },
    /// `TO_CHAR`.
    Format(Vec<Piece>),
}

impl Function {
    /// Returns true when the function gives only numbers.
    pub fn number_only(self, arguments: &[Expr]) -> bool {
        match self {
            Function::Length
            | Function::Size
            | Function::OctetLength
            | Function::Abs
            | Function::Round
            | Function::Floor
            | Function::Ceil
            | Function::Now
            | Function::ToTimestamp
            | Function::ToTimestampMs
            | Function::Epoch
            | Function::EpochMs => true,
            Function::Greatest | Function::Least => arguments.iter().all(Expr::number_only),
            _ => false,
        }
    }

    /// Returns true when the function gives a time.
    pub fn is_time(self, arguments: &[Expr]) -> bool {
        match self {
            Function::Now | Function::ToTimestamp | Function::ToTimestampMs => true,
            Function::Coalesce | Function::Greatest | Function::Least => {
                arguments.iter().all(Expr::is_time)
            }
            Function::NullIf => arguments[0].is_time(),
            _ => false,
        }
    }

    /// Returns true when the function can give many values. `ARRAY_TO_STRING` joins the
    /// values of its list in one text.
    pub fn has_any(self, arguments: &[Expr]) -> bool {
        self != Function::ArrayToString && arguments.iter().any(Expr::has_any)
    }
}

/// Reads a text between quotes at an argument.
fn literal<'a>(
    arguments: &'a [Expr],
    offsets: &[usize],
    index: usize,
) -> Result<&'a str, ExecError> {
    match &arguments[index] {
        Expr::Literal(Json::String(text)) => Ok(text),
        _ => Err(error_at(text::KAFKA_NEEDS_LITERAL, offsets[index])),
    }
}

/// Converts a Postgres replacement: `\1` to `\9` are groups and `\&` is the match.
fn replacement(source: &str) -> String {
    let mut out = String::new();
    let mut chars = source.chars().peekable();
    while let Some(char) = chars.next() {
        match (char, chars.peek().copied()) {
            ('\\', Some(digit @ '1'..='9')) => {
                chars.next();
                out.push_str(&format!("${{{digit}}}"));
            }
            ('\\', Some('&')) => {
                chars.next();
                out.push_str("${0}");
            }
            ('\\', Some('\\')) => {
                chars.next();
                out.push('\\');
            }
            ('$', _) => out.push_str("$$"),
            (char, _) => out.push(char),
        }
    }
    out
}

/// Checks the arguments of a function and reads its text arguments before the read.
pub fn prepare(
    function: Function,
    arguments: &[Expr],
    offsets: &[usize],
) -> Result<Option<Box<Prepared>>, ExecError> {
    let compile = |index: usize, ignore_case: bool| {
        let pattern = literal(arguments, offsets, index)?;
        regex(pattern, ignore_case).ok_or_else(|| error_at(text::KAFKA_BAD_REGEX, offsets[index]))
    };
    Ok(match function {
        Function::Size if arguments[0].has_any() => {
            return Err(error_at(text::KAFKA_SIZE_FIELD, offsets[0]));
        }
        Function::Epoch | Function::EpochMs | Function::ToChar if !arguments[0].is_time() => {
            return Err(error_at(text::KAFKA_NEEDS_TIME, offsets[0]));
        }
        Function::Substring
            if arguments.len() == 2 && matches!(arguments[1], Expr::Literal(Json::String(_))) =>
        {
            Prepared::Find(compile(1, false)?)
        }
        Function::RegexpReplace => {
            let flags = match arguments.get(3) {
                Some(_) => literal(arguments, offsets, 3)?,
                None => "",
            };
            if !flags.chars().all(|flag| flag == 'g' || flag == 'i') {
                return Err(error_at(text::KAFKA_BAD_FLAGS, offsets[3]));
            }
            Prepared::Replace {
                regex: compile(1, flags.contains('i'))?,
                replacement: replacement(literal(arguments, offsets, 2)?),
                global: flags.contains('g'),
            }
        }
        Function::ArrayToString => Prepared::Join {
            separator: literal(arguments, offsets, 1)?.to_owned(),
            null: match arguments.get(2) {
                Some(_) => Some(literal(arguments, offsets, 2)?.to_owned()),
                None => None,
            },
        },
        Function::ToChar => Prepared::Format(time::format(literal(arguments, offsets, 1)?)),
        _ => return Ok(None),
    })
    .map(|prepared| Some(Box::new(prepared)))
}

/// Visits each combination of the values of the arguments, in order, without a list of
/// the combinations. With `missing_as_null`, an argument without a value gives null.
fn visit_args(
    arguments: &[Expr],
    context: Context<'_>,
    missing_as_null: bool,
    done: &[&Json],
    check: &mut dyn FnMut(&[&Json]) -> Result<bool, ExecError>,
) -> Result<bool, ExecError> {
    let Some((first, rest)) = arguments.split_first() else {
        return check(done);
    };
    let mut found = false;
    let stopped = first.visit(context, &mut |value| {
        found = true;
        let mut next: Vec<&Json> = done.to_vec();
        next.push(value);
        visit_args(rest, context, missing_as_null, &next, check)
    })?;
    if found || !missing_as_null {
        return Ok(stopped);
    }
    let mut next: Vec<&Json> = done.to_vec();
    next.push(&NULL);
    visit_args(rest, context, missing_as_null, &next, check)
}

/// Visits the values of a function call.
pub fn visit(
    function: Function,
    arguments: &[Expr],
    prepared: Option<&Prepared>,
    context: Context<'_>,
    check: &mut Check<'_>,
) -> Result<bool, ExecError> {
    match function {
        Function::Coalesce => coalesce(arguments, context, check),
        Function::NullIf => null_if(arguments, context, check),
        Function::ArrayToString => array_to_string(arguments, prepared, context, check),
        Function::OctetLength if raw_part(&arguments[0]).is_some() => {
            let meta = context.message.meta;
            let bytes = match raw_part(&arguments[0]) {
                Some(Root::Key) => meta.key_bytes,
                _ => meta.value_bytes,
            };
            match bytes {
                Some(bytes) => check(&Json::from(bytes)),
                None => Ok(false),
            }
        }
        _ => {
            let lenient = matches!(
                function,
                Function::Concat | Function::ConcatWs | Function::Greatest | Function::Least
            );
            visit_args(arguments, context, lenient, &[], &mut |values| match apply(
                function, values, prepared, context,
            )? {
                None => Ok(false),
                Some(Out::Value(value)) => check(&value),
                Some(Out::Held(value, _memory)) => check(&value),
                Some(Out::Same(value)) => check(value),
                Some(Out::Text(text, _memory)) => check(&Json::String(text)),
            })
        }
    }
}

/// Returns `key` or `value` without a path. `OCTET_LENGTH` counts their received bytes.
pub fn raw_part(argument: &Expr) -> Option<Root> {
    argument
        .bare_part()
        .filter(|root| matches!(root, Root::Key | Root::Value))
}

fn coalesce(
    arguments: &[Expr],
    context: Context<'_>,
    check: &mut Check<'_>,
) -> Result<bool, ExecError> {
    for argument in arguments {
        let mut found = false;
        let stopped = argument.visit(context, &mut |value| {
            if value.is_null() {
                return Ok(false);
            }
            found = true;
            check(value)
        })?;
        if found {
            return Ok(stopped);
        }
    }
    Ok(false)
}

fn null_if(
    arguments: &[Expr],
    context: Context<'_>,
    check: &mut Check<'_>,
) -> Result<bool, ExecError> {
    arguments[0].visit(context, &mut |first| {
        let mut found = false;
        let stopped = arguments[1].visit(context, &mut |second| {
            found = true;
            if compare(first, Operator::Equal, second) {
                check(&Json::Null)
            } else {
                check(first)
            }
        })?;
        if found { Ok(stopped) } else { check(first) }
    })
}

/// Joins items in one text.
struct Joiner<'a, 'e> {
    text: Text<'e>,
    separator: &'a str,
    null: Option<&'a str>,
    first: bool,
}

impl<'a, 'e> Joiner<'a, 'e> {
    fn new(
        separator: &'a str,
        null: Option<&'a str>,
        evaluation: &'e Evaluation,
    ) -> Result<Self, ExecError> {
        Ok(Self {
            text: Text::new(evaluation)?,
            separator,
            null,
            first: true,
        })
    }

    fn push(&mut self, item: &Json) -> Result<(), ExecError> {
        if item.is_null() && self.null.is_none() {
            return Ok(());
        }
        if !self.first {
            self.text.push(self.separator.as_bytes())?;
        }
        self.first = false;
        match (item, self.null) {
            (Json::Null, Some(null)) => self.text.push(null.as_bytes()),
            (item, _) => push_text(&mut self.text, item),
        }
    }
}

fn array_to_string(
    arguments: &[Expr],
    prepared: Option<&Prepared>,
    context: Context<'_>,
    check: &mut Check<'_>,
) -> Result<bool, ExecError> {
    let Some(Prepared::Join { separator, null }) = prepared else {
        unreachable!("ARRAY_TO_STRING has a separator");
    };
    let joiner = || Joiner::new(separator, null.as_deref(), context.evaluation);
    if arguments[0].has_any() {
        let mut join = joiner()?;
        let mut found = false;
        arguments[0].visit(context, &mut |value| {
            found = true;
            join.push(value)?;
            Ok(false)
        })?;
        if !found {
            return Ok(false);
        }
        let (text, _memory) = join.text.into_parts();
        return check(&Json::String(text));
    }
    arguments[0].visit(context, &mut |value| {
        let Json::Array(items) = value else {
            return Ok(false);
        };
        let mut join = joiner()?;
        for item in items {
            join.push(item)?;
        }
        let (text, _memory) = join.text.into_parts();
        check(&Json::String(text))
    })
}

/// A result: a new value, the value of an argument, or generated text.
enum Out<'v, 'e> {
    Value(Json),
    Same(&'v Json),
    Text(String, Memory<'e>),
    /// A new value with the reservation of its memory.
    Held(Json, Memory<'e>),
}

fn new_value<'v, 'e>(json: Option<Json>) -> Result<Option<Out<'v, 'e>>, ExecError> {
    Ok(json.map(Out::Value))
}

/// Writes a value as text: a text stays the same, another value becomes JSON text.
fn push_text(out: &mut Text<'_>, value: &Json) -> Result<(), ExecError> {
    match value {
        Json::String(text) => out.push(text.as_bytes()),
        other => out.json(other),
    }
}

fn copied<'e>(
    evaluation: &'e Evaluation,
    text: &str,
) -> Result<Option<Out<'static, 'e>>, ExecError> {
    let mut out = Text::new(evaluation)?;
    out.push(text.as_bytes())?;
    let (text, memory) = out.into_parts();
    Ok(Some(Out::Text(text, memory)))
}

fn finished<'e>(out: Text<'e>) -> Result<Option<Out<'static, 'e>>, ExecError> {
    let (text, memory) = out.into_parts();
    Ok(Some(Out::Text(text, memory)))
}

/// Reads a whole number, such as `3` or `3.0`.
fn whole(value: &Json) -> Option<i64> {
    let number = value.as_number()?;
    number.as_i64().or_else(|| {
        let value = number.as_f64()?;
        (value.fract() == 0.0 && value.abs() < 9.0e18).then_some(value as i64)
    })
}

/// The byte offset of a character position. A position after the end gives the length.
fn char_offset(text: &str, position: i64) -> usize {
    match usize::try_from(position) {
        Ok(position) => text
            .char_indices()
            .nth(position)
            .map_or(text.len(), |(offset, _)| offset),
        Err(_) => 0,
    }
}

fn char_count(text: &str) -> i64 {
    i64::try_from(text.chars().count()).unwrap_or(i64::MAX)
}

/// Rounds a number to `places` decimal places, half away from zero. It reads the shortest
/// decimal text of the number, so `ROUND(2.675, 2)` gives 2.68, as Postgres numeric does.
fn round(number: &Number, places: i64) -> Option<Json> {
    let text = match (number.as_i64(), number.as_u64()) {
        (Some(value), _) => value.to_string(),
        (_, Some(value)) => value.to_string(),
        _ => number.as_f64()?.to_string(),
    };
    let (negative, text) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.as_str()),
    };
    let (whole_part, fraction) = text.split_once('.').unwrap_or((text, ""));
    let digits: Vec<u8> = whole_part.bytes().chain(fraction.bytes()).collect();
    let keep = i64::try_from(whole_part.len()).ok()?.checked_add(places)?;
    let Ok(keep) = usize::try_from(keep) else {
        return Some(Json::from(0));
    };
    if keep >= digits.len() {
        return Some(Json::Number(number.clone()));
    }
    let mut kept = digits[..keep].to_vec();
    if digits[keep] >= b'5' {
        let mut index = kept.len();
        loop {
            if index == 0 {
                kept.insert(0, b'1');
                break;
            }
            index -= 1;
            if kept[index] == b'9' {
                kept[index] = b'0';
            } else {
                kept[index] += 1;
                break;
            }
        }
    }
    if kept.is_empty() {
        kept.push(b'0');
    }
    let mut result = String::from_utf8(kept).ok()?;
    if places > 0 {
        let places = usize::try_from(places).ok()?;
        if result.len() <= places {
            result.insert_str(0, &"0".repeat(places + 1 - result.len()));
        }
        result.insert(result.len() - places, '.');
    } else {
        result.push_str(&"0".repeat(usize::try_from(places.unsigned_abs()).ok()?));
    }
    if negative {
        result.insert(0, '-');
    }
    super::expr::parse_number(&result)
}

/// Reads a number of seconds or milliseconds as a time.
fn time_from(value: &Json, scale: i64) -> Option<i64> {
    let number = value.as_number()?;
    let millis = match number.as_i64() {
        Some(value) => value.checked_mul(scale)?,
        None => {
            let value = (number.as_f64()? * scale as f64).round();
            if value.abs() >= 9.0e18 {
                return None;
            }
            value as i64
        }
    };
    DateTime::from_timestamp_millis(millis).map(|_| millis)
}

/// Reads a time: a number of milliseconds.
fn time_of(value: &Json) -> Option<i64> {
    time_from(value, 1)
}

/// Applies a function to one combination of values. It gives `None` for no value.
fn apply<'v, 'e>(
    function: Function,
    values: &[&'v Json],
    prepared: Option<&Prepared>,
    context: Context<'e>,
) -> Result<Option<Out<'v, 'e>>, ExecError> {
    let evaluation = context.evaluation;
    let text = |index: usize| values.get(index).and_then(|value| value.as_str());
    let integer = |index: usize| values.get(index).and_then(|value| whole(value));
    match function {
        Function::Lower | Function::Upper => {
            let Some(source) = text(0) else {
                return Ok(None);
            };
            let (text, memory) = evaluation.case(source, function == Function::Upper)?;
            Ok(Some(Out::Text(text, memory)))
        }
        Function::Length => new_value(text(0).map(|text| Json::from(text.chars().count()))),
        Function::OctetLength => new_value(text(0).map(|text| Json::from(text.len()))),
        Function::Size => new_value(values[0].as_array().map(|items| Json::from(items.len()))),
        Function::TypeOf => {
            let name = match values[0] {
                Json::Null => "null",
                Json::Bool(_) => "boolean",
                Json::Number(_) => "number",
                Json::String(_) => "string",
                Json::Array(_) => "array",
                Json::Object(_) => "object",
            };
            copied(evaluation, name)
        }
        Function::SplitPart => {
            let (Some(source), Some(separator), Some(position)) = (text(0), text(1), integer(2))
            else {
                return Ok(None);
            };
            if position == 0 {
                return Ok(None);
            }
            let index = usize::try_from(position.unsigned_abs() - 1).unwrap_or(usize::MAX);
            let part = if separator.is_empty() {
                (index == 0).then_some(source)
            } else if position > 0 {
                source.split(separator).nth(index)
            } else {
                source.rsplit(separator).nth(index)
            };
            copied(evaluation, part.unwrap_or(""))
        }
        Function::Substring => {
            let Some(source) = text(0) else {
                return Ok(None);
            };
            if let Some(Prepared::Find(regex)) = prepared {
                let found = regex.captures(source).map(|captures| {
                    captures
                        .get(if captures.len() > 1 { 1 } else { 0 })
                        .map(|found| found.as_str())
                });
                return match found.flatten() {
                    Some(found) => copied(evaluation, found),
                    None => new_value(Some(Json::Null)),
                };
            }
            let Some(start) = integer(1) else {
                return Ok(None);
            };
            let end = match values.get(2) {
                None => i64::MAX,
                Some(count) => match whole(count) {
                    Some(count) if count >= 0 => start.saturating_add(count),
                    _ => return Ok(None),
                },
            };
            let from = start.max(1);
            if end <= from {
                return copied(evaluation, "");
            }
            let begin = char_offset(source, from - 1);
            let finish = begin + char_offset(&source[begin..], end - from);
            copied(evaluation, &source[begin..finish])
        }
        Function::Left | Function::Right => {
            let (Some(source), Some(count)) = (text(0), integer(1)) else {
                return Ok(None);
            };
            let total = char_count(source);
            let keep = if count >= 0 {
                count.min(total)
            } else {
                total.saturating_add(count).max(0)
            };
            let part = if function == Function::Left {
                &source[..char_offset(source, keep)]
            } else {
                &source[char_offset(source, total - keep)..]
            };
            copied(evaluation, part)
        }
        Function::Trim | Function::LTrim | Function::RTrim => {
            let Some(source) = text(0) else {
                return Ok(None);
            };
            let characters = match values.get(1) {
                None => " ",
                Some(characters) => match characters.as_str() {
                    Some(characters) => characters,
                    None => return Ok(None),
                },
            };
            let trimmed = |char: char| characters.contains(char);
            copied(
                evaluation,
                match function {
                    Function::Trim => source.trim_matches(trimmed),
                    Function::LTrim => source.trim_start_matches(trimmed),
                    _ => source.trim_end_matches(trimmed),
                },
            )
        }
        Function::Replace => {
            let (Some(source), Some(from), Some(to)) = (text(0), text(1), text(2)) else {
                return Ok(None);
            };
            if from.is_empty() {
                return Ok(Some(Out::Same(values[0])));
            }
            let mut out = Text::new(evaluation)?;
            let mut last = 0;
            for (index, _) in source.match_indices(from) {
                out.push(&source.as_bytes()[last..index])?;
                out.push(to.as_bytes())?;
                last = index + from.len();
            }
            out.push(&source.as_bytes()[last..])?;
            finished(out)
        }
        Function::RegexpReplace => {
            let Some(Prepared::Replace {
                regex,
                replacement,
                global,
            }) = prepared
            else {
                unreachable!("REGEXP_REPLACE has a pattern");
            };
            let Some(source) = text(0) else {
                return Ok(None);
            };
            let mut out = Text::new(evaluation)?;
            let mut expanded = String::new();
            let mut last = 0;
            for captures in regex.captures_iter(source) {
                let found = captures.get(0).expect("a match has group 0");
                out.push(&source.as_bytes()[last..found.start()])?;
                expanded.clear();
                captures.expand(replacement, &mut expanded);
                out.push(expanded.as_bytes())?;
                last = found.end();
                if !global {
                    break;
                }
            }
            out.push(&source.as_bytes()[last..])?;
            finished(out)
        }
        Function::Concat => {
            let mut out = Text::new(evaluation)?;
            for value in values.iter().filter(|value| !value.is_null()) {
                push_text(&mut out, value)?;
            }
            finished(out)
        }
        Function::ConcatWs => {
            let Some(separator) = text(0) else {
                return new_value(values[0].is_null().then_some(Json::Null));
            };
            let mut out = Text::new(evaluation)?;
            for (index, value) in values[1..]
                .iter()
                .filter(|value| !value.is_null())
                .enumerate()
            {
                if index > 0 {
                    out.push(separator.as_bytes())?;
                }
                push_text(&mut out, value)?;
            }
            finished(out)
        }
        Function::Abs | Function::Floor | Function::Ceil | Function::Round => {
            let Some(amount) = values[0].as_number() else {
                return Ok(None);
            };
            let exact = amount.is_i64() || amount.is_u64();
            let float = amount.as_f64().unwrap_or_default();
            match function {
                Function::Abs if amount.is_u64() || float >= 0.0 => Ok(Some(Out::Same(values[0]))),
                Function::Abs => new_value(
                    amount
                        .as_i64()
                        .and_then(i64::checked_abs)
                        .map(Json::from)
                        .or_else(|| number(float.abs())),
                ),
                Function::Floor | Function::Ceil if exact => Ok(Some(Out::Same(values[0]))),
                Function::Floor => new_value(number(float.floor())),
                Function::Ceil => new_value(number(float.ceil())),
                _ => {
                    let places = match values.get(1) {
                        None => 0,
                        Some(places) => match whole(places) {
                            Some(places) => places,
                            None => return Ok(None),
                        },
                    };
                    new_value(round(amount, places))
                }
            }
        }
        Function::Greatest | Function::Least => {
            let wanted = if function == Function::Greatest {
                Operator::Greater
            } else {
                Operator::Less
            };
            let mut best: Option<&'v Json> = None;
            for value in values.iter().copied().filter(|value| !value.is_null()) {
                if matches!(value, Json::Array(_) | Json::Object(_)) {
                    return Ok(None);
                }
                best = match best {
                    None => Some(value),
                    Some(current)
                        if std::mem::discriminant(current) != std::mem::discriminant(value) =>
                    {
                        return Ok(None);
                    }
                    Some(current) if compare(value, wanted, current) => Some(value),
                    current => current,
                };
            }
            Ok(Some(best.map_or(Out::Value(Json::Null), Out::Same)))
        }
        Function::Keys => {
            let Some(map) = values[0].as_object() else {
                return Ok(None);
            };
            let bytes = map.keys().try_fold(0usize, |bytes, key| {
                bytes.checked_add(key.len() + std::mem::size_of::<Json>())
            });
            let memory = evaluation.reserve(bytes.unwrap_or(usize::MAX))?;
            let keys = map.keys().map(|key| Json::String(key.clone())).collect();
            Ok(Some(Out::Held(Json::Array(keys), memory)))
        }
        Function::Now => new_value(Some(Json::from(context.message.meta.now))),
        Function::ToTimestamp => new_value(time_from(values[0], 1_000).map(Json::from)),
        Function::ToTimestampMs => new_value(time_of(values[0]).map(Json::from)),
        Function::Epoch => new_value(time_of(values[0]).and_then(|millis| {
            if millis % 1_000 == 0 {
                Some(Json::from(millis / 1_000))
            } else {
                number(millis as f64 / 1_000.0)
            }
        })),
        Function::EpochMs => new_value(time_of(values[0]).map(Json::from)),
        Function::ToChar => {
            let Some(Prepared::Format(pieces)) = prepared else {
                unreachable!("TO_CHAR has a format");
            };
            let Some(time) = time_of(values[0]).and_then(DateTime::from_timestamp_millis) else {
                return Ok(None);
            };
            let mut out = Text::new(evaluation)?;
            time::write(pieces, time, &mut out)?;
            finished(out)
        }
        Function::Coalesce | Function::NullIf | Function::ArrayToString => {
            unreachable!("these functions have their own visit")
        }
    }
}
