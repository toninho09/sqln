//! SELECT maps to the same finite and continuous reads as CONSUME and TAIL.

use super::{Command, Cursor, Start, TokenKind, error_at, parse_read, parse_time};
use crate::connector::ExecError;
use crate::connector::kafka::filter::Text;
use crate::connector::kafka::filter::{self, Context, Evaluation, Expr, Message, Needs};
use crate::grid::{Row, RowWriter, Value};
use crate::ui::text;
use serde_json::Value as Json;

/// Where a column takes its cells.
#[derive(Debug)]
enum Source {
    /// A message column, with its original value and type.
    Column(usize),
    /// A value of the message, such as a field of the decoded value.
    Expr(Expr),
}

#[derive(Debug)]
struct Column {
    source: Source,
    name: String,
}

#[derive(Debug)]
pub struct Projection {
    columns: Vec<Column>,
    /// The parts of the message that the columns read as JSON.
    needs: Needs,
}

/// A cell borrows the source row or holds bounded generated text.
enum Cell<'a> {
    Original(Value<'a>),
    Generated(Text<'a>, bool),
}

impl Cell<'_> {
    fn value(&self) -> Value<'_> {
        match self {
            Cell::Original(value) => *value,
            Cell::Generated(text, true) => Value::Number(text.as_str()),
            Cell::Generated(text, false) => Value::Text(text.as_str()),
        }
    }
}

impl Projection {
    pub fn names(&self) -> Vec<String> {
        self.columns
            .iter()
            .map(|column| column.name.clone())
            .collect()
    }

    pub fn apply(&self, row: Row, evaluation: &Evaluation) -> Result<Row, ExecError> {
        evaluation.check()?;
        let cells: Vec<_> = row.cells().collect();
        let text = |index: usize| match cells.get(index) {
            Some(Value::Text(value)) => Some(*value),
            _ => None,
        };
        let number = |index: usize| match cells.get(index) {
            Some(Value::Number(value)) => value.parse().unwrap_or_default(),
            _ => 0,
        };
        let json = |need: bool, index: usize| {
            need.then(|| text(index).and_then(|value| serde_json::from_str::<Json>(value).ok()))
                .flatten()
        };
        let (key_json, value_json, headers_json) = (
            json(self.needs.key, 3),
            json(self.needs.value, 4),
            json(self.needs.headers, 5),
        );
        let message = Message {
            partition: number(0),
            offset: number(1),
            timestamp: text(2).and_then(parse_time),
            key: text(3),
            key_json: key_json.as_ref(),
            value: text(4),
            json: value_json.as_ref(),
            headers: text(5),
            headers_json: headers_json.as_ref(),
            schema: text(6),
        };
        let context = Context {
            message: &message,
            scope: None,
            evaluation,
        };
        let mut projected = Vec::with_capacity(self.columns.len());
        for column in &self.columns {
            evaluation.check()?;
            let cell = match &column.source {
                Source::Column(index) => Cell::Original(cells[*index]),
                Source::Expr(expr) => {
                    let mut text = Text::new(evaluation)?;
                    let list = expr.has_any();
                    let mut found = false;
                    let mut number = false;
                    let mut null = false;
                    if list {
                        text.push(b"[")?;
                    }
                    expr.visit(context, &mut |value| {
                        if list {
                            if found {
                                text.push(b",")?;
                            }
                            text.json(value)?;
                        } else {
                            number = value.is_number();
                            null = value.is_null();
                            match value {
                                Json::String(value) => text.push(value.as_bytes())?,
                                Json::Null => {}
                                other => text.json(other)?,
                            }
                        }
                        found = true;
                        Ok(!list)
                    })?;
                    if !found || null {
                        Cell::Original(Value::Null)
                    } else {
                        if list {
                            text.push(b"]")?;
                        }
                        Cell::Generated(text, number)
                    }
                }
            };
            projected.push(cell);
        }
        let bytes = projected.iter().try_fold(0usize, |bytes, cell| {
            bytes
                .checked_add(RowWriter::cell_bytes(cell.value()))
                .ok_or_else(|| ExecError::new(text::KAFKA_EVAL_MEMORY))
        })?;
        // Generated cells and the final row coexist until the row is complete.
        let _memory = evaluation.reserve(bytes)?;
        let mut result = RowWriter::with_capacity(bytes);
        for cell in &projected {
            evaluation.check()?;
            result.push(cell.value());
        }
        Ok(result.into_row())
    }
}

/// Reads one column of the list. A message column alone keeps its value and type. The
/// label is the written text.
fn source(cursor: &mut Cursor<'_>, line: &str) -> Result<(Source, String), ExecError> {
    let first = cursor.index;
    let expr = filter::expression(cursor)?;
    let written = line[cursor.tokens[first].start..cursor.tokens[cursor.index - 1].end].to_owned();
    let source = match expr {
        Expr::Field(root, path) if path.is_empty() => match root.index() {
            Some(index) => Source::Column(index),
            None => Source::Expr(Expr::Field(root, path)),
        },
        expr => Source::Expr(expr),
    };
    Ok((source, written))
}

fn expect(cursor: &mut Cursor<'_>, word: &str) -> Result<(), ExecError> {
    let offset = cursor.offset();
    if cursor.advance().is_some_and(|token| token.is_word(word)) {
        Ok(())
    } else {
        Err(error_at(text::kafka_needs(word), offset))
    }
}

fn projection(cursor: &mut Cursor<'_>, line: &str) -> Result<Option<Projection>, ExecError> {
    if cursor.peek().is_some_and(|token| token.is_operator("*")) {
        cursor.advance();
        expect(cursor, "FROM")?;
        return Ok(None);
    }
    let mut columns = Vec::new();
    loop {
        let (source, written) = source(cursor, line)?;
        let name = if cursor.peek().is_some_and(|token| token.is_word("AS")) {
            cursor.advance();
            cursor.name("a column alias")?
        } else {
            written
        };
        columns.push(Column { source, name });
        if cursor.peek().is_some_and(|token| token.is_operator(",")) {
            cursor.advance();
        } else {
            expect(cursor, "FROM")?;
            let mut needs = Needs::default();
            for column in &columns {
                if let Source::Expr(expr) = &column.source {
                    expr.needs(&mut needs);
                }
            }
            return Ok(Some(Projection { columns, needs }));
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
            .and_then(|token| parse_time(&token.text))
            .map(Start::Time)
            .ok_or_else(|| error_at(text::KAFKA_SELECT_START, offset));
    }
    Err(error_at(text::KAFKA_SELECT_START, offset))
}

pub(super) fn parse(cursor: &mut Cursor<'_>, line: &str) -> Result<Command, ExecError> {
    let projection = projection(cursor, line)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::sync::CancellationToken;

    fn apply(sql: &str, json: &str, evaluation: &Evaluation) -> Result<Row, ExecError> {
        let Command::Consume(read) = crate::connector::kafka::command::parse(sql).unwrap() else {
            panic!("expected a finite read");
        };
        let row = [
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Text(json),
            Value::Null,
            Value::Null,
        ]
        .into_iter()
        .collect();
        read.projection.unwrap().apply(row, evaluation)
    }

    #[test]
    fn lazy_projections_preserve_cartesian_order_and_coalesce_lists() {
        let evaluation = Evaluation::new(CancellationToken::new(), 1024);
        let row = apply(
            "SELECT value.a[*] + value.b[*], COALESCE(value.c[*], value.b[*]) FROM orders",
            r#"{"a":[1,2],"b":[10,20],"c":[null,3,4]}"#,
            &evaluation,
        )
        .unwrap();
        assert_eq!(row.get(0), Some(Value::Text("[11,21,12,22]")));
        assert_eq!(row.get(1), Some(Value::Text("[3,4]")));
        assert_eq!(evaluation.used(), 0);
    }

    #[test]
    fn an_expanded_projection_fails_before_it_collects_the_cartesian_product() {
        let evaluation = Evaluation::new(CancellationToken::new(), 512);
        let json = serde_json::json!({"items": vec![1; 10_000]}).to_string();
        let error = apply(
            "SELECT value.items[*] + value.items[*] FROM orders",
            &json,
            &evaluation,
        )
        .unwrap_err();
        assert!(error.message.contains("temporary memory limit"));
        assert_eq!(evaluation.used(), 0);
        assert!(apply("SELECT 1 FROM orders", "{}", &evaluation).is_ok());
        assert_eq!(evaluation.used(), 0);
    }

    #[test]
    fn the_memory_limit_counts_all_columns_and_the_final_row() {
        let json = serde_json::json!({"items": vec![1; 20]}).to_string();
        let evaluation = Evaluation::new(CancellationToken::new(), 128);
        assert!(apply("SELECT value.items[*] FROM orders", &json, &evaluation).is_ok());
        let error = apply(
            "SELECT value.items[*], value.items[*] FROM orders",
            &json,
            &evaluation,
        )
        .unwrap_err();
        assert!(error.message.contains("temporary memory limit"));
        assert_eq!(evaluation.used(), 0);
    }

    #[test]
    fn a_stopped_projection_releases_its_buffers() {
        let stop = CancellationToken::new();
        let evaluation = Evaluation::new(stop.clone(), 512);
        stop.cancel();
        assert!(
            apply(
                "SELECT value.items[*] FROM orders",
                r#"{"items":[1,2]}"#,
                &evaluation
            )
            .is_err()
        );
        assert_eq!(evaluation.used(), 0);
    }
}
