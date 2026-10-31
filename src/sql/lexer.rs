use super::Dialect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    /// A keyword or an identifier without quotes.
    Word,
    Number,
    String,
    QuotedIdentifier,
    Comment,
    /// One ASCII character, such as an operator or a delimiter.
    Punctuation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
    /// False for an unterminated token and for a line comment.
    pub closed: bool,
}

/// Produces tokens from text. A statement splitter can set `pos` to skip text.
pub struct Lexer<'a> {
    sql: &'a str,
    dialect: Dialect,
    pub pos: usize,
    previous: Option<Token>,
}

impl<'a> Lexer<'a> {
    pub fn new(sql: &'a str, dialect: Dialect) -> Self {
        Self {
            sql,
            dialect,
            pos: 0,
            previous: None,
        }
    }

    fn read_token(&self, start: usize) -> (TokenKind, usize, bool) {
        use TokenKind::{Comment, Number, Punctuation, QuotedIdentifier, String, Word};

        let bytes = self.sql.as_bytes();
        let dialect = self.dialect;
        let byte = bytes[start];

        if bytes[start..].starts_with(b"--")
            && (dialect != Dialect::MySql
                || bytes.get(start + 2).is_none_or(u8::is_ascii_whitespace))
        {
            return (Comment, skip_line(bytes, start), false);
        }
        if dialect == Dialect::MySql && byte == b'#' {
            return (Comment, skip_line(bytes, start), false);
        }
        if bytes[start..].starts_with(b"/*") {
            let (end, closed) = skip_block_comment(bytes, start, dialect == Dialect::Postgres);
            return (Comment, end, closed);
        }

        match byte {
            b'\'' => {
                let backslash = dialect == Dialect::MySql
                    || (dialect == Dialect::Postgres && self.after_escape_prefix(start));
                let (end, closed) = skip_quoted(bytes, start, b'\'', backslash);
                (String, end, closed)
            }
            b'"' => {
                let mysql = dialect == Dialect::MySql;
                let (end, closed) = skip_quoted(bytes, start, b'"', mysql);
                (if mysql { String } else { QuotedIdentifier }, end, closed)
            }
            b'`' if dialect != Dialect::Postgres => {
                let (end, closed) = skip_quoted(bytes, start, b'`', false);
                (QuotedIdentifier, end, closed)
            }
            b'[' if dialect == Dialect::Sqlite => {
                let (end, closed) = skip_bracket(bytes, start);
                (QuotedIdentifier, end, closed)
            }
            b'$' if dialect == Dialect::Postgres && !self.after_word(start) => {
                match dollar_tag_end(bytes, start) {
                    Some(tag_end) => {
                        let (end, closed) = skip_dollar_quote(bytes, start, tag_end);
                        (String, end, closed)
                    }
                    None => (Punctuation, start + 1, true),
                }
            }
            _ if is_word_byte(byte) => {
                let mut end = start;
                while end < bytes.len() && is_word_byte(bytes[end]) {
                    end += 1;
                }
                (if byte.is_ascii_digit() { Number } else { Word }, end, true)
            }
            _ => (Punctuation, start + 1, true),
        }
    }

    fn after_word(&self, start: usize) -> bool {
        start > 0 && is_word_byte(self.sql.as_bytes()[start - 1])
    }

    /// Returns true for the PostgreSQL form `E'text'`.
    fn after_escape_prefix(&self, start: usize) -> bool {
        self.previous.is_some_and(|token| {
            token.end == start
                && token.kind == TokenKind::Word
                && self.sql[token.start..token.end].eq_ignore_ascii_case("e")
        })
    }
}

impl Iterator for Lexer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        let bytes = self.sql.as_bytes();
        while self.pos < bytes.len() && bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        if self.pos >= bytes.len() {
            return None;
        }
        let start = self.pos;
        let (kind, end, closed) = self.read_token(start);
        let token = Token {
            kind,
            start,
            end,
            closed,
        };
        self.pos = end;
        self.previous = Some(token);
        Some(token)
    }
}

pub fn tokenize(sql: &str, dialect: Dialect) -> Vec<Token> {
    Lexer::new(sql, dialect).collect()
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

fn skip_quoted(bytes: &[u8], open: usize, quote: u8, backslash: bool) -> (usize, bool) {
    let mut pos = open + 1;
    while pos < bytes.len() {
        let byte = bytes[pos];
        if backslash && byte == b'\\' {
            pos += 2;
        } else if byte == quote {
            if bytes.get(pos + 1) == Some(&quote) {
                pos += 2;
            } else {
                return (pos + 1, true);
            }
        } else {
            pos += 1;
        }
    }
    (bytes.len(), false)
}

fn skip_bracket(bytes: &[u8], open: usize) -> (usize, bool) {
    match bytes[open..].iter().position(|byte| *byte == b']') {
        Some(offset) => (open + offset + 1, true),
        None => (bytes.len(), false),
    }
}

fn skip_block_comment(bytes: &[u8], open: usize, nested: bool) -> (usize, bool) {
    let mut depth = 1;
    let mut pos = open + 2;
    while pos < bytes.len() {
        if nested && bytes[pos..].starts_with(b"/*") {
            depth += 1;
            pos += 2;
        } else if bytes[pos..].starts_with(b"*/") {
            depth -= 1;
            pos += 2;
            if depth == 0 {
                return (pos, true);
            }
        } else {
            pos += 1;
        }
    }
    (bytes.len(), false)
}

fn skip_line(bytes: &[u8], open: usize) -> usize {
    bytes[open..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |offset| open + offset)
}

fn dollar_tag_end(bytes: &[u8], open: usize) -> Option<usize> {
    let mut pos = open + 1;
    while pos < bytes.len() && is_word_byte(bytes[pos]) {
        pos += 1;
    }
    let tag_starts_with_digit = bytes.get(open + 1).is_some_and(u8::is_ascii_digit);
    (bytes.get(pos) == Some(&b'$') && !tag_starts_with_digit).then_some(pos + 1)
}

fn skip_dollar_quote(bytes: &[u8], open: usize, tag_end: usize) -> (usize, bool) {
    let tag = &bytes[open..tag_end];
    let mut pos = tag_end;
    while pos + tag.len() <= bytes.len() {
        if &bytes[pos..pos + tag.len()] == tag {
            return (pos + tag.len(), true);
        }
        pos += 1;
    }
    (bytes.len(), false)
}
