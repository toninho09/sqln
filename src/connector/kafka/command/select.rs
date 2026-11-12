//! SELECT maps to the same finite and continuous reads as CONSUME and TAIL.

use super::{Command, Cursor, Start, TokenKind, error_at, parse_read};
use crate::connector::ExecError;
use crate::connector::kafka::{MESSAGE_COLUMNS, filter};
use crate::grid::{Row, RowWriter, Value};
use crate::ui::text;
use serde_json::Value as Json;

#[derive(Debug)]
struct Column {
    index: usize,
    path: Vec<String>,
    name: String,
}

#[derive(Debug)]
pub struct Projection {
    columns: Vec<Column>,
}

impl Projection {
    pub fn names(&self) -> Vec<String> {
        self.columns
            .iter()
            .map(|column| column.name.clone())
            .collect()
    }

    pub fn apply(&self, row: Row) -> Row {
        let cells: Vec<_> = row.cells().collect();
        let json = self
            .columns
            .iter()
            .any(|column| !column.path.is_empty())
            .then(|| match cells.get(4) {
                Some(Value::Text(value)) => serde_json::from_str::<Json>(value).ok(),
                _ => None,
            })
            .flatten();
        let mut result = RowWriter::default();
        for column in &self.columns {
            if column.path.is_empty() {
                result.push(cells[column.index]);
                continue;
            }
            match json
                .as_ref()
                .and_then(|json| filter::field(json, &column.path))
            {
                None | Some(Json::Null) => result.push(Value::Null),
                Some(Json::String(value)) => result.push(Value::Text(value)),
                Some(Json::Number(value)) => result.push(Value::Number(&value.to_string())),
                Some(value) => result.push(Value::Text(&value.to_string())),
            }
        }
        result.finish()
    }
}

fn expect(cursor: &mut Cursor<'_>, word: &str) -> Result<(), ExecError> {
    let offset = cursor.offset();
    if cursor.advance().is_some_and(|token| token.is_word(word)) {
        Ok(())
    } else {
        Err(error_at(text::kafka_needs(word), offset))
    }
}

fn projection(cursor: &mut Cursor<'_>) -> Result<Option<Projection>, ExecError> {
    if cursor.peek().is_some_and(|token| token.is_operator("*")) {
        cursor.advance();
        expect(cursor, "FROM")?;
        return Ok(None);
    }
    let mut columns = Vec::new();
    loop {
        let offset = cursor.offset();
        let token = cursor
            .advance()
            .filter(|token| token.kind == TokenKind::Word)
            .ok_or_else(|| error_at(text::KAFKA_SELECT_COLUMN, offset))?;
        let mut parts = token.text.split('.');
        let root = parts.next().unwrap_or("");
        let index = MESSAGE_COLUMNS
            .iter()
            .position(|name| root.eq_ignore_ascii_case(name))
            .ok_or_else(|| error_at(text::KAFKA_SELECT_COLUMN, offset))?;
        let path: Vec<String> = parts.map(str::to_owned).collect();
        if path.iter().any(String::is_empty)
            || (!path.is_empty() && !root.eq_ignore_ascii_case("value"))
        {
            return Err(error_at(text::KAFKA_SELECT_COLUMN, offset));
        }
        let name = if cursor.peek().is_some_and(|token| token.is_word("AS")) {
            cursor.advance();
            cursor.name("a column alias")?
        } else {
            token.text.clone()
        };
        columns.push(Column { index, path, name });
        if cursor.peek().is_some_and(|token| token.is_operator(",")) {
            cursor.advance();
        } else {
            expect(cursor, "FROM")?;
            return Ok(Some(Projection { columns }));
        }
    }
}

pub(super) fn parse_start(cursor: &mut Cursor<'_>) -> Result<Start, ExecError> {
    expect(cursor, "AT")?;
    let offset = cursor.offset();
    let token = cursor
        .advance()
        .ok_or_else(|| error_at(text::KAFKA_SELECT_START, offset))?;
    if token.is_word("EARLIEST") {
        return Ok(Start::Earliest);
    }
    if token.is_word("LATEST") {
        return Ok(Start::Latest);
    }
    if token.is_word("OFFSET") {
        let (value, offset) = cursor.number::<i64>("an offset of 0 or more")?;
        if value >= 0 {
            return Ok(Start::Offset(value));
        }
        return Err(error_at(text::KAFKA_SELECT_START, offset));
    }
    if token.is_word("TIMESTAMP") {
        let offset = cursor.offset();
        return cursor
            .advance()
            .filter(|token| token.kind == TokenKind::Text)
            .and_then(|token| super::parse_time(&token.text))
            .map(Start::Time)
            .ok_or_else(|| error_at(text::KAFKA_SELECT_START, offset));
    }
    Err(error_at(text::KAFKA_SELECT_START, offset))
}

pub(super) fn parse(cursor: &mut Cursor<'_>) -> Result<Command, ExecError> {
    let projection = projection(cursor)?;
    let rest = &cursor.tokens[cursor.index..];
    let tail = rest.len() >= 3
        && rest[rest.len() - 2].is_word("EMIT")
        && rest[rest.len() - 1].is_word("CHANGES");
    let end = rest.len() - if tail { 2 } else { 0 };
    let mut clauses = Cursor::new(
        &rest[..end],
        rest.get(end).map_or(cursor.end, |token| token.start),
    );
    let mut read = parse_read(&mut clauses, tail, true)?;
    read.projection = projection;
    cursor.index = cursor.tokens.len();
    Ok(if tail {
        Command::Tail(read)
    } else {
        Command::Consume(read)
    })
}
