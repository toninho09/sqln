//! Kafka reads use SELECT or the line commands. Keywords ignore case.

mod select;
pub use select::Projection;

use super::filter::{self, Filter};
use crate::connector::ExecError;
use crate::ui::text;

/// The first word of the text that a topic tab sends. It is not a console command.
pub const TOPIC_PAGE: &str = "SQLN.TOPIC";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Word,
    /// A text between quotes, without its quotes and with its escapes read.
    Text,
    /// `=`, `!=`, `<`, `<=`, `>`, `>=`, `(`, or `)`.
    Operator,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub text: String,
    /// The byte offset of the token in the line.
    pub start: usize,
}

impl Token {
    pub fn is_word(&self, word: &str) -> bool {
        self.kind == TokenKind::Word && self.text.eq_ignore_ascii_case(word)
    }

    pub fn is_operator(&self, operator: &str) -> bool {
        self.kind == TokenKind::Operator && self.text == operator
    }
}

pub fn error_at(message: impl Into<String>, offset: usize) -> ExecError {
    ExecError {
        message: message.into(),
        offset: Some(offset),
    }
}

/// Reads a text between quotes from `start`. It returns the text and the position
/// after the closing quote. A backslash escapes the next character, as in `redis-cli`.
fn read_text(line: &str, start: usize, select: bool) -> Result<(String, usize), ExecError> {
    let bytes = line.as_bytes();
    let quote = bytes[start];
    let mut text = Vec::new();
    let mut pos = start + 1;
    while let Some(&byte) = bytes.get(pos) {
        pos += 1;
        match byte {
            _ if byte == quote && select && bytes.get(pos) == Some(&quote) => {
                text.push(quote);
                pos += 1;
            }
            _ if byte == quote => {
                let text =
                    String::from_utf8(text).map_err(|_| error_at(text::KAFKA_BAD_TEXT, start))?;
                return Ok((text, pos));
            }
            b'\\' => {
                let Some(&next) = bytes.get(pos) else { break };
                pos += 1;
                match next {
                    b'n' => text.push(b'\n'),
                    b'r' => text.push(b'\r'),
                    b't' => text.push(b'\t'),
                    b'x' => {
                        let value = line
                            .get(pos..pos + 2)
                            .and_then(|digits| u8::from_str_radix(digits, 16).ok())
                            .ok_or_else(|| error_at(text::KAFKA_BAD_TEXT, pos - 2))?;
                        text.push(value);
                        pos += 2;
                    }
                    other => text.push(other),
                }
            }
            _ => text.push(byte),
        }
    }
    Err(error_at(text::REDIS_BAD_QUOTES, start))
}

fn is_operator_byte(byte: u8) -> bool {
    matches!(byte, b'=' | b'!' | b'<' | b'>' | b'(' | b')')
}

/// Splits a line in tokens. A word ends at a space, a quote, or an operator.
pub fn tokenize(line: &str) -> Result<Vec<Token>, ExecError> {
    tokenize_mode(line, false)
}

pub(crate) fn tokenize_mode(line: &str, select: bool) -> Result<Vec<Token>, ExecError> {
    let bytes = line.as_bytes();
    let mut tokens = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let byte = bytes[pos];
        if byte.is_ascii_whitespace() {
            pos += 1;
            continue;
        }
        if byte == b'#'
            && line[..pos]
                .rsplit('\n')
                .next()
                .unwrap_or("")
                .trim()
                .is_empty()
        {
            pos = line[pos..].find('\n').map_or(bytes.len(), |end| pos + end);
            continue;
        }
        let start = pos;
        let (kind, text) = match byte {
            b',' | b';' | b'*' if select => {
                pos += 1;
                (TokenKind::Operator, line[start..pos].to_owned())
            }
            b'\'' | b'"' => {
                let (text, end) = read_text(line, start, select)?;
                pos = end;
                (TokenKind::Text, text)
            }
            b'!' | b'<' | b'>' if bytes.get(pos + 1) == Some(&b'=') => {
                pos += 2;
                (TokenKind::Operator, line[start..pos].to_owned())
            }
            b'!' => return Err(error_at(text::KAFKA_BAD_OPERATOR, start)),
            _ if is_operator_byte(byte) => {
                pos += 1;
                (TokenKind::Operator, line[start..pos].to_owned())
            }
            _ => {
                while pos < bytes.len()
                    && !bytes[pos].is_ascii_whitespace()
                    && !matches!(bytes[pos], b'\'' | b'"')
                    && !is_operator_byte(bytes[pos])
                    && !(select && matches!(bytes[pos], b',' | b';' | b'*'))
                {
                    pos += 1;
                }
                (TokenKind::Word, line[start..pos].to_owned())
            }
        };
        tokens.push(Token { kind, text, start });
    }
    Ok(tokens)
}

/// Where a read starts in each partition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    Earliest,
    Latest,
    Offset(i64),
    /// Milliseconds since 1970, in UTC.
    Time(i64),
}

#[derive(Debug)]
pub struct Read {
    pub topic: String,
    pub partition: Option<i32>,
    pub start: Start,
    pub filter: Option<Filter>,
    /// The most messages that match. `None` keeps the row cap.
    pub limit: Option<usize>,
    /// The most messages that the read receives.
    pub scan: Option<u64>,
    /// `None` keeps all message columns.
    pub projection: Option<Projection>,
}

#[derive(Debug)]
pub enum Command {
    Topics,
    Partitions(String),
    Groups,
    Group(String),
    Consume(Read),
    Tail(Read),
    /// The last messages of a topic, for a topic tab.
    TopicPage {
        topic: String,
        count: usize,
        filter: Option<Filter>,
    },
}

/// Reads the tokens of a line, one by one.
pub struct Cursor<'a> {
    tokens: &'a [Token],
    pub index: usize,
    /// The offset of the end of the line, for an error at the end.
    end: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(tokens: &'a [Token], end: usize) -> Self {
        Self {
            tokens,
            index: 0,
            end,
        }
    }

    pub fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.index)
    }

    pub fn advance(&mut self) -> Option<&'a Token> {
        let token = self.tokens.get(self.index);
        self.index += usize::from(token.is_some());
        token
    }

    /// Returns the offset of the next token, or the end of the line.
    pub fn offset(&self) -> usize {
        self.peek().map_or(self.end, |token| token.start)
    }

    /// Reads a name: a word or a text between quotes.
    fn name(&mut self, what: &str) -> Result<String, ExecError> {
        match self.advance() {
            Some(token) if token.kind != TokenKind::Operator => Ok(token.text.clone()),
            Some(token) => Err(error_at(text::kafka_needs(what), token.start)),
            None => Err(error_at(text::kafka_needs(what), self.end)),
        }
    }

    fn number<T: std::str::FromStr>(&mut self, what: &str) -> Result<(T, usize), ExecError> {
        let offset = self.offset();
        self.advance()
            .filter(|token| token.kind == TokenKind::Word)
            .and_then(|token| token.text.parse().ok())
            .map(|value| (value, offset))
            .ok_or_else(|| error_at(text::kafka_needs(what), offset))
    }

    fn finish(&self) -> Result<(), ExecError> {
        match self.peek() {
            None => Ok(()),
            Some(token) => Err(error_at(text::KAFKA_EXTRA_WORDS, token.start)),
        }
    }
}

/// Reads the time of `@time`: RFC 3339, or a date and a time in UTC.
fn parse_time(text: &str) -> Option<i64> {
    if let Ok(time) = chrono::DateTime::parse_from_rfc3339(text) {
        return Some(time.timestamp_millis());
    }
    let naive = chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%.f"))
        .ok()
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
                .ok()?
                .and_hms_opt(0, 0, 0)
        })?;
    Some(naive.and_utc().timestamp_millis())
}

fn parse_start(cursor: &mut Cursor<'_>) -> Result<Start, ExecError> {
    let offset = cursor.offset();
    let bad = || error_at(text::KAFKA_BAD_START, offset);
    let token = cursor
        .advance()
        .filter(|token| token.kind == TokenKind::Word);
    let token = token.ok_or_else(bad)?;
    if token.is_word("earliest") {
        return Ok(Start::Earliest);
    }
    if token.is_word("latest") {
        return Ok(Start::Latest);
    }
    if let Some(time) = token.text.strip_prefix('@') {
        return parse_time(time).map(Start::Time).ok_or_else(bad);
    }
    token
        .text
        .parse::<i64>()
        .ok()
        .filter(|value| *value >= 0)
        .map(Start::Offset)
        .ok_or_else(bad)
}

/// Reads the clauses of `CONSUME` and `TAIL`, in any order, each one time.
fn parse_read(cursor: &mut Cursor<'_>, tail: bool, select: bool) -> Result<Read, ExecError> {
    let topic = cursor.name(text::KAFKA_WHAT_TOPIC)?;
    let mut read = Read {
        topic,
        partition: None,
        start: if tail { Start::Latest } else { Start::Earliest },
        filter: None,
        limit: None,
        scan: None,
        projection: None,
    };
    let mut seen: Vec<String> = Vec::new();
    while let Some(token) = cursor.advance() {
        let clause = token.text.to_ascii_uppercase();
        let allowed = match clause.as_str() {
            "PARTITION" | "WHERE" => true,
            "FROM" => !select,
            "START" => select,
            "LIMIT" | "SCAN" => !tail,
            _ => false,
        };
        if token.kind != TokenKind::Word || !allowed {
            let message = if select {
                text::KAFKA_SELECT_CLAUSES
            } else if tail {
                text::KAFKA_TAIL_CLAUSES
            } else {
                text::KAFKA_CONSUME_CLAUSES
            };
            return Err(error_at(message, token.start));
        }
        if seen.contains(&clause) {
            return Err(error_at(text::KAFKA_REPEATED_CLAUSE, token.start));
        }
        seen.push(clause.clone());
        match clause.as_str() {
            "PARTITION" => {
                let (partition, offset) = cursor.number::<i32>(text::KAFKA_WHAT_PARTITION)?;
                if partition < 0 {
                    return Err(error_at(
                        text::kafka_needs(text::KAFKA_WHAT_PARTITION),
                        offset,
                    ));
                }
                read.partition = Some(partition);
            }
            "FROM" => read.start = parse_start(cursor)?,
            "START" => read.start = select::parse_start(cursor)?,
            "WHERE" => read.filter = Some(filter::parse(cursor)?),
            "LIMIT" => {
                let (limit, offset) = cursor.number::<usize>(text::KAFKA_WHAT_COUNT)?;
                if limit == 0 {
                    return Err(error_at(text::kafka_needs(text::KAFKA_WHAT_COUNT), offset));
                }
                read.limit = Some(limit);
            }
            _ => {
                let (scan, offset) = cursor.number::<u64>(text::KAFKA_WHAT_COUNT)?;
                if scan == 0 {
                    return Err(error_at(text::kafka_needs(text::KAFKA_WHAT_COUNT), offset));
                }
                read.scan = Some(scan);
            }
        }
    }
    Ok(read)
}

/// Reads a SELECT statement or one command line.
pub fn parse(line: &str) -> Result<Command, ExecError> {
    let select = crate::sql::first_keyword(line, crate::sql::Dialect::Kafka)
        .is_some_and(|word| word.eq_ignore_ascii_case("SELECT"));
    let mut tokens = tokenize_mode(line, select)?;
    if select && tokens.last().is_some_and(|token| token.is_operator(";")) {
        tokens.pop();
    }
    let mut cursor = Cursor::new(&tokens, line.trim_end().len());
    let Some(first) = cursor.advance() else {
        return Err(ExecError::new(text::REDIS_NO_COMMAND));
    };
    let name = first.text.to_ascii_uppercase();
    let command = match name.as_str() {
        _ if first.kind != TokenKind::Word => {
            return Err(error_at(text::KAFKA_UNKNOWN_COMMAND, first.start));
        }
        "TOPICS" => Command::Topics,
        "GROUPS" => Command::Groups,
        "PARTITIONS" => Command::Partitions(cursor.name(text::KAFKA_WHAT_TOPIC)?),
        "GROUP" => Command::Group(cursor.name(text::KAFKA_WHAT_GROUP)?),
        "SELECT" => select::parse(&mut cursor)?,
        "CONSUME" => Command::Consume(parse_read(&mut cursor, false, false)?),
        "TAIL" => Command::Tail(parse_read(&mut cursor, true, false)?),
        TOPIC_PAGE => {
            let topic = cursor.name(text::KAFKA_WHAT_TOPIC)?;
            let (count, _) = cursor.number::<usize>(text::KAFKA_WHAT_COUNT)?;
            let filter = match cursor.advance() {
                Some(token) if token.is_word("WHERE") => Some(filter::parse(&mut cursor)?),
                Some(token) => return Err(error_at(text::KAFKA_EXTRA_WORDS, token.start)),
                None => None,
            };
            Command::TopicPage {
                topic,
                count,
                filter,
            }
        }
        _ => return Err(error_at(text::KAFKA_UNKNOWN_COMMAND, first.start)),
    };
    cursor.finish()?;
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn consume(line: &str) -> Read {
        match parse(line).unwrap() {
            Command::Consume(read) => read,
            other => panic!("not a read: {other:?}"),
        }
    }

    #[test]
    fn clauses_come_in_any_order_and_the_filter_ends_at_the_next_clause() {
        let read = consume("consume orders where key = 'a' limit 5 from 10 partition 2 scan 99");
        assert_eq!(read.topic, "orders");
        assert_eq!(read.partition, Some(2));
        assert_eq!(read.start, Start::Offset(10));
        assert_eq!(read.limit, Some(5));
        assert_eq!(read.scan, Some(99));
        assert!(read.filter.is_some());
    }

    #[test]
    fn a_tail_starts_at_the_end_and_refuses_a_limit() {
        match parse("TAIL orders").unwrap() {
            Command::Tail(read) => assert_eq!(read.start, Start::Latest),
            other => panic!("not a tail: {other:?}"),
        }
        let error = parse("TAIL orders LIMIT 3").unwrap_err();
        assert_eq!(error.offset, Some(12));
    }

    #[test]
    fn a_time_start_reads_utc() {
        let read = consume("CONSUME t FROM @2026-01-02");
        assert_eq!(read.start, Start::Time(1_767_312_000_000));
        let read = consume("CONSUME t FROM @2026-01-02T00:00:01Z");
        assert_eq!(read.start, Start::Time(1_767_312_001_000));
    }

    #[test]
    fn errors_give_the_position_of_the_bad_word() {
        assert_eq!(parse("CONSUME t FROM never").unwrap_err().offset, Some(15));
        assert_eq!(
            parse("CONSUME t LIMIT 1 LIMIT 2").unwrap_err().offset,
            Some(18)
        );
        assert_eq!(parse("TOPICS now").unwrap_err().offset, Some(7));
        assert_eq!(parse("DELETE t").unwrap_err().offset, Some(0));
        assert_eq!(parse("CONSUME").unwrap_err().offset, Some(7));
    }

    #[test]
    fn a_quoted_name_keeps_its_spaces() {
        match parse(r#"GROUP "my group""#).unwrap() {
            Command::Group(name) => assert_eq!(name, "my group"),
            other => panic!("not a group: {other:?}"),
        }
    }
}
