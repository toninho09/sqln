//! The SQL formatter. It moves only line breaks and spaces, so each token stays the same.
//!
//! Outside a line break, a gap between two tokens stays: one space, or no space. The
//! lexer keeps `>=` as two tokens, so this rule keeps the operators whole.

use super::Dialect;
use super::classify::is_any;
use super::lexer::{Token, TokenKind, tokenize};

/// The spaces of one level of indent.
const STEP: usize = 2;

/// A statement with another first word stays as it is.
const FORMATTED: &[&str] = &["SELECT", "WITH", "VALUES", "INSERT", "UPDATE", "DELETE"];

const JOIN_WORDS: &[&str] = &[
    "JOIN", "OUTER", "INNER", "LEFT", "RIGHT", "FULL", "NATURAL", "CROSS",
];

/// What a comma, `AND`, and `OR` do in the current part of the statement.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Clause {
    Other,
    /// The queries of `WITH`. A comma starts a line with no indent.
    With,
    /// A comma starts an indented line.
    List,
    /// `AND` and `OR` start an indented line.
    Condition,
}

/// One level of parentheses, or the statement itself.
struct Frame {
    /// A subquery breaks into lines. Other parentheses, such as a call, stay in one line.
    subquery: bool,
    clause: Clause,
    /// True after `BETWEEN`, because its `AND` starts no line.
    between: bool,
    /// The open `CASE` expressions. Their text stays in one line.
    cases: usize,
}

impl Frame {
    fn new(subquery: bool) -> Self {
        Self {
            subquery,
            clause: Clause::Other,
            between: false,
            cases: 0,
        }
    }
}

fn is(word: &str, expected: &str) -> bool {
    word.eq_ignore_ascii_case(expected)
}

/// Returns the clause that a word starts. Each exception is a word with another role,
/// such as `FROM` in `IS DISTINCT FROM`.
fn clause_of(word: &str, previous: &str, next: &str, current: Clause) -> Option<Clause> {
    let upper = word.to_ascii_uppercase();
    let clause = match upper.as_str() {
        "SELECT" | "SET" | "RETURNING" | "ON" => Clause::List,
        "VALUES" if !is(previous, "DEFAULT") => Clause::List,
        "FROM" if !is_any(previous, &["DELETE", "DISTINCT"]) => Clause::List,
        "GROUP" | "ORDER" if is(next, "BY") => Clause::List,
        "WHERE" | "HAVING" => Clause::Condition,
        "JOIN" | "STRAIGHT_JOIN" if !is_any(previous, JOIN_WORDS) => Clause::Condition,
        "INNER" | "CROSS" | "NATURAL" | "FULL" | "LEFT" | "RIGHT"
            if is_any(next, JOIN_WORDS) && !is_any(previous, JOIN_WORDS) =>
        {
            Clause::Condition
        }
        "LIMIT" | "OFFSET" | "FETCH" | "UNION" | "INTERSECT" | "EXCEPT" | "MINUS" => Clause::Other,
        "FOR" if is_any(next, &["UPDATE", "SHARE", "NO", "KEY"]) => Clause::Other,
        "INSERT" | "UPDATE" | "DELETE" | "MERGE" if current == Clause::With => Clause::Other,
        _ => return None,
    };
    Some(clause)
}

struct Writer<'a> {
    sql: &'a str,
    tokens: &'a [Token],
    out: String,
    frames: Vec<Frame>,
    /// The indent of a line break that the next token waits for.
    pending: Option<usize>,
}

impl<'a> Writer<'a> {
    fn new(sql: &'a str, tokens: &'a [Token]) -> Self {
        Self {
            sql,
            tokens,
            out: String::with_capacity(sql.len() + sql.len() / 4),
            frames: vec![Frame::new(true)],
            pending: None,
        }
    }

    fn text(&self, index: usize) -> &'a str {
        let token = self.tokens[index];
        &self.sql[token.start()..token.end()]
    }

    /// Returns the text of the nearest token that is not a comment.
    fn neighbor(&self, index: usize, forward: bool) -> &'a str {
        let found = if forward {
            (index + 1..self.tokens.len()).find(|at| self.tokens[*at].kind != TokenKind::Comment)
        } else {
            (0..index)
                .rev()
                .find(|at| self.tokens[*at].kind != TokenKind::Comment)
        };
        found.map_or("", |at| self.text(at))
    }

    fn frame(&mut self) -> &mut Frame {
        let last = self.frames.len() - 1;
        &mut self.frames[last]
    }

    fn indent(&self) -> usize {
        let levels = self.frames.iter().filter(|frame| frame.subquery).count();
        STEP * (levels - 1)
    }

    fn separate(&mut self, line: Option<usize>, gap: &str) {
        match line {
            Some(_) if self.out.is_empty() => {}
            Some(indent) => {
                self.out.push('\n');
                self.out.push_str(&" ".repeat(indent));
            }
            None if !gap.is_empty() => self.out.push(' '),
            None => {}
        }
    }

    fn comment(&mut self, index: usize, gap: &str) {
        let indent = self.pending.unwrap_or(self.indent());
        self.separate(gap.contains('\n').then_some(indent), gap);
        self.out.push_str(self.text(index));
        // A line comment ends with its line, so the next token needs a new line.
        if !self.tokens[index].closed {
            self.pending = Some(indent);
        }
    }

    fn close_parenthesis(&mut self) -> Option<usize> {
        if self.frames.len() == 1 {
            return None;
        }
        let frame = self.frames.pop()?;
        frame.subquery.then(|| self.indent())
    }

    fn place_word(&mut self, index: usize, word: &str) -> Option<usize> {
        let indent = self.indent();
        let previous = self.neighbor(index, false);
        let next = self.neighbor(index, true);
        let frame = self.frame();
        if is(word, "CASE") {
            frame.cases += 1;
            return None;
        }
        if is(word, "END") {
            frame.cases = frame.cases.saturating_sub(1);
            return None;
        }
        if frame.cases > 0 || !frame.subquery || previous == "." {
            return None;
        }
        if is(word, "WITH") && matches!(previous, "" | "(") {
            frame.clause = Clause::With;
            return None;
        }
        if is(word, "BETWEEN") {
            frame.between = true;
            return None;
        }
        if is(word, "ON") && !is_any(next, &["CONFLICT", "DUPLICATE"]) {
            frame.clause = Clause::Condition;
            return None;
        }
        if is_any(word, &["AND", "OR"]) {
            if frame.clause != Clause::Condition
                || (is(word, "AND") && std::mem::take(&mut frame.between))
            {
                return None;
            }
            return Some(indent + STEP);
        }
        frame.clause = clause_of(word, previous, next, frame.clause)?;
        Some(indent)
    }

    /// Updates the state for a token. It returns the indent of a line before the token.
    fn place(&mut self, index: usize) -> Option<usize> {
        let text = self.text(index);
        match self.tokens[index].kind {
            TokenKind::Punctuation if text == ")" => self.close_parenthesis(),
            TokenKind::Word => self.place_word(index, text),
            _ => None,
        }
    }

    /// Updates the state after a token that changes the next line.
    fn after(&mut self, index: usize) {
        if self.tokens[index].kind != TokenKind::Punctuation {
            return;
        }
        match self.text(index) {
            "(" => {
                let next = self.neighbor(index, true);
                let subquery = self.frame().cases == 0 && is_any(next, &["SELECT", "WITH"]);
                self.frames.push(Frame::new(subquery));
                if subquery {
                    self.pending = Some(self.indent());
                }
            }
            "," => {
                let indent = self.indent();
                let frame = self.frame();
                if frame.subquery && frame.cases == 0 {
                    match frame.clause {
                        Clause::With => self.pending = Some(indent),
                        Clause::List => self.pending = Some(indent + STEP),
                        Clause::Other | Clause::Condition => {}
                    }
                }
            }
            _ => {}
        }
    }

    /// Two strings on two lines are one value in SQL, and on one line they are an error.
    fn joins_strings(&self, index: usize, gap: &str) -> bool {
        index > 0
            && gap.contains('\n')
            && self.tokens[index - 1].kind == TokenKind::String
            && self.tokens[index].kind == TokenKind::String
    }

    fn run(mut self) -> String {
        for index in 0..self.tokens.len() {
            let gap = match index {
                0 => "",
                _ => &self.sql[self.tokens[index - 1].end()..self.tokens[index].start()],
            };
            if self.tokens[index].kind == TokenKind::Comment {
                self.comment(index, gap);
                continue;
            }
            let line = self.place(index).or(self.pending.take());
            let line = line.or(self.joins_strings(index, gap).then(|| self.indent()));
            self.pending = None;
            self.separate(line, gap);
            self.out.push_str(self.text(index));
            self.after(index);
        }
        self.out
    }
}

fn token_texts(sql: &str, dialect: Dialect) -> Vec<&str> {
    let tokens = tokenize(sql, dialect);
    tokens
        .iter()
        .map(|token| &sql[token.start()..token.end()])
        .collect()
}

/// Returns true for a token that the lexer did not close, other than a line comment.
fn is_open(sql: &str, token: &Token) -> bool {
    !token.closed && (token.kind != TokenKind::Comment || sql[token.start()..].starts_with("/*"))
}

/// Formats one statement, without its delimiter. A statement of another kind stays as
/// it is. Without a sure result, such as with an open string, it returns `None`.
pub fn format_statement(sql: &str, dialect: Dialect) -> Option<String> {
    let tokens = tokenize(sql, dialect);
    if tokens.iter().any(|token| is_open(sql, token)) {
        return None;
    }
    let first = tokens
        .iter()
        .find(|token| token.kind != TokenKind::Comment)?;
    if dialect.is_command_line() || !is_any(&sql[first.start()..first.end()], FORMATTED) {
        return Some(sql.to_owned());
    }
    let formatted = Writer::new(sql, &tokens).run();
    (token_texts(sql, dialect) == token_texts(&formatted, dialect)).then_some(formatted)
}
