//! The values of a Kafka filter and of the SELECT columns: message columns, fields of
//! JSON, literals, arithmetic, and functions.

use std::borrow::Cow;

use serde_json::{Number, Value as Json};

use super::evaluation::{Evaluation, Text};
use super::functions::{self, Function, Prepared};
use super::{Filter, Message, time};

pub type Check<'a> = dyn FnMut(&Json) -> Result<bool, ExecError> + 'a;
use crate::connector::ExecError;
use crate::connector::kafka::MESSAGE_COLUMNS;
use crate::connector::kafka::command::{Cursor, Token, TokenKind, error_at};
use crate::ui::text;

/// The start of a field: a message column, or the item of `ANY` and `EVERY`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Root {
    Partition,
    Offset,
    Timestamp,
    Key,
    Value,
    Headers,
    Schema,
    /// The item of the `ANY` or `EVERY` at this depth. The outer one has depth 0.
    Var(usize),
}

/// The roots of the message columns, in the order of `MESSAGE_COLUMNS`.
const COLUMNS: [Root; 7] = [
    Root::Partition,
    Root::Offset,
    Root::Timestamp,
    Root::Key,
    Root::Value,
    Root::Headers,
    Root::Schema,
];

impl Root {
    pub fn column(word: &str) -> Option<Root> {
        MESSAGE_COLUMNS
            .iter()
            .position(|name| name.eq_ignore_ascii_case(word))
            .map(|index| COLUMNS[index])
    }

    /// The index of the message column in a row.
    pub fn index(self) -> Option<usize> {
        COLUMNS.iter().position(|root| *root == self)
    }

    /// Returns true when the root can hold JSON, so a path can follow it.
    pub fn has_json(self) -> bool {
        matches!(self, Root::Key | Root::Value | Root::Headers | Root::Var(_))
    }
}

/// One step of a field path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    /// A field of an object. A number also reads an item of a list, as in `value.items.0`.
    Name(String),
    /// `[n]`: an item of a list. A negative index counts from the end.
    Index(i64),
    /// `[*]`: each item of a list.
    Any,
    /// `[a:b]`: each item from `a` to before `b`. A negative position counts from the end.
    /// No position means the start or the end of the list.
    Slice(Option<i64>, Option<i64>),
}

impl Segment {
    /// Returns true when the step reads each item of a range: `[*]` or `[a:b]`.
    pub fn is_many(&self) -> bool {
        matches!(self, Segment::Any | Segment::Slice(..))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Concat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    Number,
    Text,
    Boolean,
}

const TYPES: [(&str, Type); 3] = [
    ("NUMBER", Type::Number),
    ("TEXT", Type::Text),
    ("BOOLEAN", Type::Boolean),
];

#[derive(Clone, Debug)]
pub enum Expr {
    /// A message column, or a field of the JSON of a root.
    Field(Root, Vec<Segment>),
    Literal(Json),
    Binary(Op, Box<Expr>, Box<Expr>),
    Call(Function, Vec<Expr>, Option<Box<Prepared>>),
    Cast(Box<Expr>, Type),
    /// `CASE WHEN condition THEN value … ELSE value END`. Without `ELSE`, the value is null.
    Case(Vec<(Filter, Expr)>, Option<Box<Expr>>),
}

/// The parts of the message that need JSON.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Needs {
    pub key: bool,
    pub value: bool,
    pub headers: bool,
}

/// The item of one `ANY` or `EVERY`, with the items of the outer ones.
#[derive(Clone, Copy)]
pub struct Scope<'a> {
    pub item: &'a Json,
    pub depth: usize,
    pub parent: Option<&'a Scope<'a>>,
}

/// What an expression reads: one message, and the items of `ANY` and `EVERY`.
#[derive(Clone, Copy)]
pub struct Context<'a> {
    pub message: &'a Message<'a>,
    pub scope: Option<&'a Scope<'a>>,
    pub evaluation: &'a Evaluation,
}

impl<'a> Context<'a> {
    fn item(self, depth: usize) -> Option<&'a Json> {
        let mut scope = self.scope;
        while let Some(current) = scope {
            if current.depth == depth {
                return Some(current.item);
            }
            scope = current.parent;
        }
        None
    }
}

impl Expr {
    /// Visits values in order. A true result stops the evaluation.
    /// Intermediate results stay on the stack, not in a list.
    pub fn visit(&self, context: Context<'_>, check: &mut Check<'_>) -> Result<bool, ExecError> {
        context.evaluation.check()?;
        match self {
            Expr::Field(root, path) => field_values(*root, path, context, check),
            Expr::Literal(json) => check(json),
            Expr::Binary(op, left, right) => left.visit(context, &mut |left| {
                right.visit(context, &mut |right| {
                    context.evaluation.check()?;
                    if *op == Op::Concat {
                        if left.is_null() || right.is_null() {
                            return Ok(false);
                        }
                        let mut text = Text::new(context.evaluation)?;
                        for value in [left, right] {
                            match value {
                                Json::String(value) => text.push(value.as_bytes())?,
                                other => text.json(other)?,
                            }
                        }
                        let (text, _memory) = text.into_parts();
                        check(&Json::String(text))
                    } else {
                        match op.apply(left, right) {
                            Some(value) => check(&value),
                            None => Ok(false),
                        }
                    }
                })
            }),
            Expr::Call(function, arguments, prepared) => {
                functions::visit(*function, arguments, prepared.as_deref(), context, check)
            }
            Expr::Case(branches, otherwise) => {
                for (condition, value) in branches {
                    if condition.matches_in(context)? {
                        return value.visit(context, check);
                    }
                }
                match otherwise {
                    Some(value) => value.visit(context, check),
                    None => check(&Json::Null),
                }
            }
            Expr::Cast(inner, kind) => inner.visit(context, &mut |value| {
                if *kind == Type::Text && !value.is_null() && !value.is_string() {
                    let mut text = Text::new(context.evaluation)?;
                    text.json(value)?;
                    let (text, _memory) = text.into_parts();
                    return check(&Json::String(text));
                }
                match kind.cast(Cow::Borrowed(value)) {
                    Some(value) => check(&value),
                    None => Ok(false),
                }
            }),
        }
    }

    /// Returns true when a field path has `[*]` or a slice, so the expression can give
    /// many values.
    pub fn has_any(&self) -> bool {
        match self {
            Expr::Field(_, path) => path.iter().any(Segment::is_many),
            Expr::Literal(_) => false,
            Expr::Binary(_, left, right) => left.has_any() || right.has_any(),
            Expr::Call(function, arguments, _) => function.has_any(arguments),
            Expr::Cast(inner, _) => inner.has_any(),
            Expr::Case(..) => self.results().any(Expr::has_any),
        }
    }

    /// Returns true when the expression gives only numbers.
    pub fn number_only(&self) -> bool {
        match self {
            Expr::Field(root, _) => {
                matches!(root, Root::Partition | Root::Offset | Root::Timestamp)
            }
            Expr::Binary(op, _, _) => *op != Op::Concat,
            Expr::Call(function, arguments, _) => function.number_only(arguments),
            Expr::Cast(_, kind) => *kind == Type::Number,
            Expr::Literal(_) => false,
            Expr::Case(..) => self.results().all(Expr::number_only),
        }
    }

    /// Returns true when the expression gives a time: milliseconds since 1970, in UTC. A
    /// text next to a time becomes a time, and a column shows a time as UTC text.
    pub fn is_time(&self) -> bool {
        match self {
            Expr::Field(root, _) => *root == Root::Timestamp,
            Expr::Binary(Op::Add, left, right) => left.is_time() != right.is_time(),
            Expr::Binary(Op::Subtract, left, right) => left.is_time() && !right.is_time(),
            Expr::Call(function, arguments, _) => function.is_time(arguments),
            Expr::Case(..) => self.results().all(Expr::is_time),
            _ => false,
        }
    }

    /// The values that a `CASE` can give. A `CASE` without `ELSE` can also give null.
    fn results(&self) -> impl Iterator<Item = &Expr> {
        let (branches, otherwise) = match self {
            Expr::Case(branches, otherwise) => (&branches[..], otherwise.as_deref()),
            _ => (&[][..], None),
        };
        branches.iter().map(|(_, value)| value).chain(otherwise)
    }

    /// Returns `key`, `value`, or `headers` without a path. A text condition reads the
    /// text of such a part.
    pub fn bare_part(&self) -> Option<Root> {
        match self {
            Expr::Field(root @ (Root::Key | Root::Value | Root::Headers), path)
                if path.is_empty() =>
            {
                Some(*root)
            }
            _ => None,
        }
    }

    pub fn needs(&self, needs: &mut Needs) {
        match self {
            Expr::Field(Root::Key, path) => needs.key |= !path.is_empty(),
            Expr::Field(Root::Value, _) => needs.value = true,
            Expr::Field(Root::Headers, _) => needs.headers = true,
            Expr::Field(..) | Expr::Literal(_) => {}
            Expr::Binary(_, left, right) => {
                left.needs(needs);
                right.needs(needs);
            }
            Expr::Call(Function::OctetLength, arguments, _)
                if functions::raw_part(&arguments[0]).is_some() => {}
            Expr::Call(_, arguments, _) => {
                arguments.iter().for_each(|argument| argument.needs(needs));
            }
            Expr::Cast(inner, _) => inner.needs(needs),
            Expr::Case(branches, _) => {
                for (condition, _) in branches {
                    condition.add_needs(needs);
                }
                self.results().for_each(|value| value.needs(needs));
            }
        }
    }
}

fn field_values(
    root: Root,
    path: &[Segment],
    context: Context<'_>,
    check: &mut Check<'_>,
) -> Result<bool, ExecError> {
    let message = context.message;
    let copied_text = match root {
        Root::Schema => message.schema,
        Root::Key if path.is_empty() => message.key,
        _ => None,
    };
    let _memory = context
        .evaluation
        .reserve(copied_text.map_or(0, str::len))?;
    let text = |text: Option<&str>| text.map(|text| Cow::Owned(Json::String(text.to_owned())));
    let base = match root {
        Root::Partition => Some(Cow::Owned(Json::from(message.partition))),
        Root::Offset => Some(Cow::Owned(Json::from(message.offset))),
        Root::Timestamp => message.timestamp.map(|time| Cow::Owned(Json::from(time))),
        Root::Schema => text(message.schema),
        Root::Key if path.is_empty() => text(message.key),
        Root::Key => message.key_json.map(Cow::Borrowed),
        Root::Value => message.json.map(Cow::Borrowed),
        Root::Headers => message.headers_json.map(Cow::Borrowed),
        Root::Var(depth) => context.item(depth).map(Cow::Borrowed),
    };
    match base {
        None => Ok(false),
        Some(base) => any_field(&base, path, context.evaluation, check),
    }
}

/// Takes one step of a path without `[*]`.
fn step<'a>(json: &'a Json, segment: &Segment) -> Option<&'a Json> {
    match (segment, json) {
        (Segment::Name(name), Json::Object(map)) => map.get(name),
        (Segment::Name(name), Json::Array(items)) => items.get(name.parse::<usize>().ok()?),
        (Segment::Index(index), Json::Array(items)) => {
            let index = match usize::try_from(*index) {
                Ok(index) => index,
                Err(_) => items
                    .len()
                    .checked_sub(usize::try_from(index.unsigned_abs()).ok()?)?,
            };
            items.get(index)
        }
        _ => None,
    }
}

/// Visits a path without collecting its items. Each step checks cancellation.
fn any_field(
    json: &Json,
    path: &[Segment],
    evaluation: &Evaluation,
    check: &mut Check<'_>,
) -> Result<bool, ExecError> {
    evaluation.check()?;
    match path.split_first() {
        None => check(json),
        Some((segment @ (Segment::Any | Segment::Slice(..)), rest)) => {
            if let Json::Array(items) = json {
                let range = match segment {
                    Segment::Slice(start, end) => slice_range(items.len(), *start, *end),
                    _ => 0..items.len(),
                };
                for item in &items[range] {
                    if any_field(item, rest, evaluation, check)? {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }
        Some((segment, rest)) => match step(json, segment) {
            Some(next) => any_field(next, rest, evaluation, check),
            None => Ok(false),
        },
    }
}

/// The items of a slice. A negative position counts from the end. A range that ends
/// before its start is empty.
fn slice_range(length: usize, start: Option<i64>, end: Option<i64>) -> std::ops::Range<usize> {
    let position = |value: i64| match usize::try_from(value) {
        Ok(value) => value.min(length),
        Err(_) => {
            length.saturating_sub(usize::try_from(value.unsigned_abs()).unwrap_or(usize::MAX))
        }
    };
    let start = start.map_or(0, position);
    let end = end.map_or(length, position).max(start);
    start..end
}

/// A finite number as JSON. A whole number has no fraction.
pub(super) fn number(value: f64) -> Option<Json> {
    if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        return Some(Json::from(value as i64));
    }
    Number::from_f64(value).map(Json::Number)
}

/// Reads a number literal. A whole number keeps all its digits.
pub fn parse_number(text: &str) -> Option<Json> {
    if let Ok(value) = text.parse::<i64>() {
        return Some(Json::from(value));
    }
    text.parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .and_then(number)
}

impl Op {
    fn apply(self, left: &Json, right: &Json) -> Option<Json> {
        let (Json::Number(left), Json::Number(right)) = (left, right) else {
            return None;
        };
        if let (Some(left), Some(right)) = (left.as_i64(), right.as_i64()) {
            let exact = match self {
                Op::Add => left.checked_add(right),
                Op::Subtract => left.checked_sub(right),
                Op::Multiply => left.checked_mul(right),
                Op::Remainder => left.checked_rem(right),
                Op::Divide | Op::Concat => None,
            };
            if let Some(value) = exact {
                return Some(Json::from(value));
            }
        }
        let (left, right) = (left.as_f64()?, right.as_f64()?);
        number(match self {
            Op::Add => left + right,
            Op::Subtract => left - right,
            Op::Multiply => left * right,
            Op::Divide => left / right,
            Op::Remainder => left % right,
            Op::Concat => return None,
        })
    }
}

impl Type {
    /// Converts a value. A value that does not convert is absent; null stays null.
    fn cast(self, value: Cow<'_, Json>) -> Option<Cow<'_, Json>> {
        match (self, value.as_ref()) {
            (_, Json::Null)
            | (Type::Number, Json::Number(_))
            | (Type::Text, Json::String(_))
            | (Type::Boolean, Json::Bool(_)) => Some(value),
            (Type::Number, Json::String(text)) => parse_number(text.trim()).map(Cow::Owned),
            (Type::Boolean, Json::String(text)) => ["false", "true"]
                .iter()
                .position(|word| text.eq_ignore_ascii_case(word))
                .map(|index| Cow::Owned(Json::Bool(index == 1))),
            _ => None,
        }
    }
}

/// Reads a field word, such as `value.items[0].sku` or `value."order id"`. It returns
/// the root word and the path, or `None` for an empty name or a bad index.
pub fn parse_path(word: &str) -> Option<(&str, Vec<Segment>)> {
    let name_end = |text: &str| text.find(['.', '[']).unwrap_or(text.len());
    let (root, mut rest) = word.split_at(name_end(word));
    let mut path = Vec::new();
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix(".\"") {
            let (name, next) = quoted_name(after)?;
            path.push(Segment::Name(name));
            rest = next;
        } else if let Some(after) = rest.strip_prefix('.') {
            let (name, next) = after.split_at(name_end(after));
            if name.is_empty() {
                return None;
            }
            path.push(Segment::Name(name.to_owned()));
            rest = next;
        } else {
            let (inside, next) = rest.strip_prefix('[')?.split_once(']')?;
            let position = |text: &str| match text {
                "" => Some(None),
                text => text.parse().ok().map(Some),
            };
            path.push(match inside.split_once(':') {
                _ if inside == "*" => Segment::Any,
                Some((start, end)) => Segment::Slice(position(start)?, position(end)?),
                None => Segment::Index(inside.parse().ok()?),
            });
            rest = next;
        }
    }
    Some((root, path))
}

/// Writes one name of a field path, so that `parse_path` reads the same name. A name
/// that can end a word or a path step gets double quotes, with `""` for one quote.
pub fn path_name(name: &str) -> Cow<'_, str> {
    let plain = !name.is_empty()
        && !name.chars().any(|c| {
            c.is_whitespace()
                || c.is_control()
                || matches!(
                    c,
                    '"' | '\''
                        | ','
                        | ';'
                        | '*'
                        | '.'
                        | '['
                        | ']'
                        | '='
                        | '!'
                        | '<'
                        | '>'
                        | '('
                        | ')'
                )
        });
    if plain {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(format!("\"{}\"", name.replace('"', "\"\"")))
    }
}

/// Reads a name between double quotes, after the opening quote. `""` is one quote.
fn quoted_name(text: &str) -> Option<(String, &str)> {
    let mut name = String::new();
    let mut rest = text;
    loop {
        let end = rest.find('"')?;
        name.push_str(&rest[..end]);
        rest = &rest[end + 1..];
        match rest.strip_prefix('"') {
            Some(after) => {
                name.push('"');
                rest = after;
            }
            None => return Some((name, rest)),
        }
    }
}

/// Reads the `)` that closes a parenthesis.
pub fn close(cursor: &mut Cursor<'_>) -> Result<(), ExecError> {
    let offset = cursor.offset();
    if cursor.advance().is_some_and(|token| token.is_operator(")")) {
        Ok(())
    } else {
        Err(error_at(text::KAFKA_NO_CLOSE, offset))
    }
}

/// Reads a field word. `vars` holds the names of the `ANY` and `EVERY` items, from the
/// outer one to the inner one.
pub fn field(token: &Token, vars: &[String]) -> Result<Expr, ExecError> {
    let bad = || error_at(text::KAFKA_NEEDS_FIELD, token.start);
    if token.kind != TokenKind::Word {
        return Err(bad());
    }
    let (root, path) = parse_path(&token.text).ok_or_else(bad)?;
    let root = vars
        .iter()
        .position(|name| name.eq_ignore_ascii_case(root))
        .map(Root::Var)
        .or_else(|| Root::column(root))
        .ok_or_else(bad)?;
    if !path.is_empty() && !root.has_json() {
        return Err(bad());
    }
    Ok(Expr::Field(root, path))
}

/// Reads an arithmetic operator. `*` is an operator token in SELECT and a word in the
/// line commands.
fn op(token: &Token, table: &[(&str, Op)]) -> Option<Op> {
    (token.kind != TokenKind::Text)
        .then(|| table.iter().find(|(text, _)| token.text == *text))
        .flatten()
        .map(|(_, op)| *op)
}

const ADDITIVE: [(&str, Op); 3] = [("+", Op::Add), ("-", Op::Subtract), ("||", Op::Concat)];
const MULTIPLICATIVE: [(&str, Op); 3] =
    [("*", Op::Multiply), ("/", Op::Divide), ("%", Op::Remainder)];

/// Returns true when the token is an arithmetic operator.
pub fn is_op(token: &Token) -> bool {
    op(token, &ADDITIVE).is_some() || op(token, &MULTIPLICATIVE).is_some()
}

/// Reads an expression. `* / %` come before `+ - ||`.
pub fn parse(cursor: &mut Cursor<'_>, vars: &[String]) -> Result<Expr, ExecError> {
    let mut left = term(cursor, vars)?;
    while let Some(op) = cursor.peek().and_then(|token| op(token, &ADDITIVE)) {
        cursor.advance();
        left = Expr::Binary(op, Box::new(left), Box::new(term(cursor, vars)?));
    }
    Ok(left)
}

fn term(cursor: &mut Cursor<'_>, vars: &[String]) -> Result<Expr, ExecError> {
    let mut left = primary(cursor, vars)?;
    while let Some(op) = cursor.peek().and_then(|token| op(token, &MULTIPLICATIVE)) {
        cursor.advance();
        left = Expr::Binary(op, Box::new(left), Box::new(primary(cursor, vars)?));
    }
    Ok(left)
}

fn primary(cursor: &mut Cursor<'_>, vars: &[String]) -> Result<Expr, ExecError> {
    let offset = cursor.offset();
    let token = cursor
        .advance()
        .ok_or_else(|| error_at(text::KAFKA_NEEDS_VALUE, offset))?;
    match token.kind {
        TokenKind::Text => return Ok(Expr::Literal(Json::String(token.text.clone()))),
        TokenKind::Operator if token.is_operator("(") => {
            let inner = parse(cursor, vars)?;
            close(cursor)?;
            return Ok(inner);
        }
        TokenKind::Operator => return Err(error_at(text::KAFKA_NEEDS_VALUE, offset)),
        TokenKind::Word => {}
    }
    for (word, value) in [
        ("true", Json::Bool(true)),
        ("false", Json::Bool(false)),
        ("null", Json::Null),
    ] {
        if token.is_word(word) {
            return Ok(Expr::Literal(value));
        }
    }
    if let Some(number) = parse_number(&token.text) {
        return Ok(Expr::Literal(number));
    }
    if token.is_word("CASE") && !vars.iter().any(|name| token.is_word(name)) {
        return case(cursor, vars);
    }
    if token.is_word("INTERVAL")
        && let Some(literal) = cursor.peek().filter(|next| next.kind == TokenKind::Text)
    {
        cursor.advance();
        return time::interval(&literal.text)
            .map(|millis| Expr::Literal(Json::from(millis)))
            .map_err(|message| error_at(message, literal.start));
    }
    if cursor.peek().is_some_and(|next| next.is_operator("(")) {
        return call(cursor, token, vars);
    }
    field(token, vars)
}

/// Reads `CASE WHEN condition THEN value … [ELSE value] END`, after `CASE`.
fn case(cursor: &mut Cursor<'_>, vars: &[String]) -> Result<Expr, ExecError> {
    let expect = |cursor: &mut Cursor<'_>, word: &str| {
        let offset = cursor.offset();
        if cursor.advance().is_some_and(|token| token.is_word(word)) {
            Ok(())
        } else {
            Err(error_at(text::KAFKA_CASE, offset))
        }
    };
    let mut scope = vars.to_vec();
    let mut branches = Vec::new();
    expect(cursor, "WHEN")?;
    loop {
        let condition = super::parse_or(cursor, &mut scope)?;
        expect(cursor, "THEN")?;
        branches.push((condition, parse(cursor, vars)?));
        if !cursor.peek().is_some_and(|token| token.is_word("WHEN")) {
            break;
        }
        cursor.advance();
    }
    let otherwise = if cursor.peek().is_some_and(|token| token.is_word("ELSE")) {
        cursor.advance();
        Some(Box::new(parse(cursor, vars)?))
    } else {
        None
    };
    expect(cursor, "END")?;
    Ok(Expr::Case(branches, otherwise))
}

/// Reads a function or `CAST`, after its name.
fn call(cursor: &mut Cursor<'_>, name: &Token, vars: &[String]) -> Result<Expr, ExecError> {
    cursor.advance();
    if name.is_word("CAST") {
        let inner = parse(cursor, vars)?;
        let offset = cursor.offset();
        if !cursor.advance().is_some_and(|token| token.is_word("AS")) {
            return Err(error_at(text::KAFKA_CAST_TYPE, offset));
        }
        let offset = cursor.offset();
        let kind = cursor
            .advance()
            .and_then(|token| TYPES.iter().find(|(word, _)| token.is_word(word)))
            .map(|(_, kind)| *kind)
            .ok_or_else(|| error_at(text::KAFKA_CAST_TYPE, offset))?;
        close(cursor)?;
        return Ok(Expr::Cast(Box::new(inner), kind));
    }
    let spec =
        functions::find(name).ok_or_else(|| error_at(text::KAFKA_NEEDS_FIELD, name.start))?;
    let mut arguments = Vec::new();
    let mut offsets = Vec::new();
    if !cursor.peek().is_some_and(|token| token.is_operator(")")) {
        loop {
            offsets.push(cursor.offset());
            arguments.push(parse(cursor, vars)?);
            if !cursor.peek().is_some_and(|token| token.is_operator(",")) {
                break;
            }
            cursor.advance();
        }
    }
    close(cursor)?;
    if arguments.len() < spec.min || arguments.len() > spec.max {
        return Err(error_at(
            text::kafka_function_needs(&name.text, spec.min, spec.max),
            name.start,
        ));
    }
    let prepared = functions::prepare(spec.function, &arguments, &offsets)?;
    Ok(Expr::Call(spec.function, arguments, prepared))
}
