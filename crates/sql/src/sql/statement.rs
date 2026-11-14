use super::Dialect;
use super::lexer::{Lexer, Token, TokenKind, is_word_byte};

/// A sequence of tokens that the splitter can move forward.
trait TokenSource {
    fn next_token(&mut self) -> Option<Token>;
    fn seek(&mut self, position: usize);
}

impl TokenSource for Lexer<'_> {
    fn next_token(&mut self) -> Option<Token> {
        self.next()
    }

    fn seek(&mut self, position: usize) {
        self.pos = position;
    }
}

struct TokenSlice<'a> {
    tokens: &'a [Token],
    index: usize,
}

impl TokenSource for TokenSlice<'_> {
    fn next_token(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.index).copied();
        self.index += 1;
        token
    }

    fn seek(&mut self, position: usize) {
        while self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.start() < position)
        {
            self.index += 1;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    /// Byte offset of the first token.
    pub start: usize,
    /// Byte offset after the last token, without the delimiter.
    pub end: usize,
    /// Byte offset after the delimiter, or `end` without a delimiter.
    pub end_with_delimiter: usize,
    pub terminated: bool,
}

impl Statement {
    pub fn text<'a>(&self, sql: &'a str) -> &'a str {
        &sql[self.start..self.end]
    }
}

fn is_any(word: &str, list: &[&str]) -> bool {
    list.iter().any(|item| word.eq_ignore_ascii_case(item))
}

/// Tracks `BEGIN ... END` bodies, in which a delimiter does not end the statement.
#[derive(Default)]
struct Compound {
    words_seen: usize,
    is_create: bool,
    is_routine: bool,
    depth: usize,
}

impl Compound {
    /// Returns false when no later word of the statement can change this state.
    fn reads_words(&self) -> bool {
        self.words_seen == 0 || self.is_create
    }

    /// Returns true when the caller must skip the next word.
    fn on_word(&mut self, word: &str, next_word: &str, dialect: Dialect) -> bool {
        self.words_seen += 1;
        if self.words_seen == 1 {
            self.is_create = word.eq_ignore_ascii_case("CREATE");
            return false;
        }
        if self.is_create
            && !self.is_routine
            && is_any(word, &["TRIGGER", "PROCEDURE", "FUNCTION", "EVENT"])
        {
            self.is_routine = true;
            return false;
        }
        if !self.is_routine {
            return false;
        }
        if word.eq_ignore_ascii_case("BEGIN") {
            if dialect != Dialect::Postgres || next_word.eq_ignore_ascii_case("ATOMIC") {
                self.depth += 1;
            }
        } else if word.eq_ignore_ascii_case("CASE") {
            self.depth += 1;
        } else if word.eq_ignore_ascii_case("END") {
            if is_any(next_word, &["IF", "LOOP", "WHILE", "REPEAT"]) {
                return true;
            }
            self.depth = self.depth.saturating_sub(1);
            return next_word.eq_ignore_ascii_case("CASE");
        }
        false
    }
}

/// Returns the byte range of the first word at or after `from`. The word bytes are
/// the ones of the lexer, so a word with non-ASCII letters stays whole.
fn next_word(sql: &str, from: usize) -> (usize, usize) {
    let bytes = sql.as_bytes();
    let mut pos = from;
    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    let start = pos;
    while pos < bytes.len() && is_word_byte(bytes[pos]) {
        pos += 1;
    }
    (start, pos)
}

pub fn split(sql: &str, dialect: Dialect) -> Vec<Statement> {
    split_source(sql, dialect, &mut Lexer::new(sql, dialect))
}

/// Splits text that has its tokens already, so a key does not analyze the text twice.
/// In MySQL, `DELIMITER` changes how the text after it reads, so the text needs a new read.
pub fn split_tokens(sql: &str, dialect: Dialect, tokens: &[Token]) -> Vec<Statement> {
    if dialect == Dialect::MySql {
        return split(sql, dialect);
    }
    split_source(sql, dialect, &mut TokenSlice { tokens, index: 0 })
}

/// Splits Redis text. Each line with a token is one command.
fn split_lines(sql: &str, source: &mut impl TokenSource) -> Vec<Statement> {
    let mut statements: Vec<Statement> = Vec::new();
    while let Some(token) = source.next_token() {
        if token.kind == TokenKind::Comment {
            continue;
        }
        match statements.last_mut() {
            Some(last) if !sql[last.end..token.start()].contains('\n') => {
                last.end = token.end();
                last.end_with_delimiter = token.end();
            }
            _ => statements.push(Statement {
                start: token.start(),
                end: token.end(),
                end_with_delimiter: token.end(),
                terminated: false,
            }),
        }
    }
    statements
}

/// SELECT ends at a semicolon. Other Kafka commands keep their line boundaries.
fn split_kafka(sql: &str, source: &mut impl TokenSource) -> Vec<Statement> {
    let mut statements = Vec::new();
    let mut current: Option<Statement> = None;
    let mut select = false;
    while let Some(token) = source.next_token() {
        if token.kind == TokenKind::Comment {
            continue;
        }
        if !select
            && current
                .as_ref()
                .is_some_and(|last| sql[last.end..token.start()].contains('\n'))
        {
            statements.push(current.take().unwrap());
        }
        let word = &sql[token.start()..token.end()];
        if select && token.kind == TokenKind::Punctuation && word == ";" {
            if let Some(mut statement) = current.take() {
                statement.end_with_delimiter = token.end();
                statement.terminated = true;
                statements.push(statement);
            }
            select = false;
            continue;
        }
        if let Some(statement) = &mut current {
            statement.end = token.end();
            statement.end_with_delimiter = token.end();
        } else {
            select = token.kind == TokenKind::Word && word.eq_ignore_ascii_case("SELECT");
            current = Some(Statement {
                start: token.start(),
                end: token.end(),
                end_with_delimiter: token.end(),
                terminated: false,
            });
        }
    }
    statements.extend(current);
    statements
}

/// Returns the byte after the delimiter at `start`, when the text holds it there.
/// A delimiter that ends with a word character must end at a word boundary, so it
/// cannot cut an identifier that starts with the same characters.
fn delimiter_end(sql: &str, delimiter: &str, start: usize) -> Option<usize> {
    if !sql[start..].starts_with(delimiter) {
        return None;
    }
    let last = *delimiter.as_bytes().last()?;
    let end = start + delimiter.len();
    let whole_word = !is_word_byte(last)
        || sql
            .as_bytes()
            .get(end)
            .is_none_or(|byte| !is_word_byte(*byte));
    whole_word.then_some(end)
}

fn split_source(sql: &str, dialect: Dialect, source: &mut impl TokenSource) -> Vec<Statement> {
    if dialect == Dialect::Kafka {
        return split_kafka(sql, source);
    }
    if dialect.is_command_line() {
        return split_lines(sql, source);
    }
    let mut statements = Vec::new();
    let mut delimiter = ";";
    let mut start = None;
    let mut last_end = 0;
    let mut compound = Compound::default();

    while let Some(token) = source.next_token() {
        if token.kind == TokenKind::Comment {
            continue;
        }
        let word = &sql[token.start()..token.end()];

        if dialect == Dialect::MySql
            && start.is_none()
            && token.kind == TokenKind::Word
            && word.eq_ignore_ascii_case("DELIMITER")
        {
            let line_end = sql[token.start()..]
                .find('\n')
                .map_or(sql.len(), |at| token.start() + at);
            let value = sql[token.end()..line_end].trim();
            if !value.is_empty() {
                delimiter = value;
            }
            source.seek(line_end);
            continue;
        }

        if let Some(after) = delimiter_end(sql, delimiter, token.start()) {
            source.seek(after);
            if compound.depth == 0 {
                if let Some(begin) = start.take() {
                    statements.push(Statement {
                        start: begin,
                        end: last_end,
                        end_with_delimiter: after,
                        terminated: true,
                    });
                }
                compound = Compound::default();
            }
            continue;
        }

        start.get_or_insert(token.start());
        last_end = token.end();

        if matches!(token.kind, TokenKind::Word | TokenKind::Number) && compound.reads_words() {
            let (next_start, next_end) = next_word(sql, token.end());
            if compound.on_word(word, &sql[next_start..next_end], dialect) {
                source.seek(next_end);
                last_end = next_end;
            }
        }
    }

    if let Some(begin) = start {
        statements.push(Statement {
            start: begin,
            end: last_end,
            end_with_delimiter: last_end,
            terminated: false,
        });
    }
    statements
}

/// Returns the statement under the cursor.
/// Blank lines and comments between statements return `None`.
pub fn statement_at(sql: &str, dialect: Dialect, cursor: usize) -> Option<Statement> {
    split(sql, dialect)
        .into_iter()
        .find(|statement| cursor >= statement.start && cursor <= statement.end_with_delimiter)
}
