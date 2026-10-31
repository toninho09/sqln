//! Reads the text around the cursor and finds the expected kind of name.

use std::ops::Range;

use super::keywords::is_reserved;
use crate::sql::{Dialect, Statement, Token, TokenKind, tokenize};

const RELATION_WORDS: &[&str] = &["FROM", "JOIN", "UPDATE", "INTO", "TABLE"];
const EXPRESSION_WORDS: &[&str] = &[
    "SELECT", "DISTINCT", "WHERE", "ON", "BY", "SET", "HAVING", "AND", "OR", "NOT", "WHEN", "THEN",
    "ELSE", "CASE", "RETURNING", "VALUES", "BETWEEN", "LIKE", "IN", "USING", "COLUMN",
];
const LEADING_WORDS: &[&str] = &["EXPLAIN", "ANALYZE", "VERBOSE"];
const END_OF_FROM_WORDS: &[&str] = &[
    "WHERE", "GROUP", "ORDER", "HAVING", "LIMIT", "OFFSET", "ON", "USING", "SET", "UNION",
    "INTERSECT", "EXCEPT", "RETURNING", "VALUES", "WINDOW", "FETCH", "FOR",
];
const OPERAND_WORDS: &[&str] = &["NULL", "TRUE", "FALSE", "END"];

/// One table or subquery that a statement reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    pub schema: Option<String>,
    /// `None` for a subquery in a `FROM` clause.
    pub name: Option<String>,
    pub alias: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Position {
    StatementStart,
    Relation,
    Expression,
    /// An operand ends before the cursor, so the text expects a keyword.
    AfterOperand,
    /// The column list of `INSERT INTO relation (...)`.
    ColumnList(Reference),
    /// The text expects a new name, and no suggestion applies.
    NewName,
}

#[derive(Debug)]
pub struct Analysis {
    /// Byte range of the partial name.
    pub replace: Range<usize>,
    /// The partial name, without its opening quote.
    pub prefix: String,
    pub quote: Option<char>,
    pub qualifiers: Vec<String>,
    pub position: Position,
    pub scope: Vec<Reference>,
}

fn raw(text: &str, token: Token) -> &str {
    &text[token.start..token.end]
}

fn is_punctuation(text: &str, token: Token, value: u8) -> bool {
    token.kind == TokenKind::Punctuation && text.as_bytes()[token.start] == value
}

fn is_word(text: &str, token: Option<&Token>, words: &[&str]) -> bool {
    token.is_some_and(|token| {
        token.kind == TokenKind::Word
            && words.iter().any(|word| raw(text, *token).eq_ignore_ascii_case(word))
    })
}

fn is_identifier(text: &str, token: Token) -> bool {
    match token.kind {
        TokenKind::Word => !is_reserved(raw(text, token)),
        TokenKind::QuotedIdentifier => true,
        _ => false,
    }
}

/// Returns the name in a token, without quotes.
fn identifier(text: &str, token: Token) -> String {
    let value = raw(text, token);
    if token.kind != TokenKind::QuotedIdentifier {
        return value.to_owned();
    }
    let close = if value.starts_with('[') { ']' } else { value.chars().next().unwrap_or('"') };
    let inner = &value[1..];
    let inner = if token.closed { inner.strip_suffix(close).unwrap_or(inner) } else { inner };
    if close == ']' {
        return inner.to_owned();
    }
    inner.replace(&format!("{close}{close}"), &close.to_string())
}

/// Returns the text of the statement at the cursor, with its blank space.
fn statement_region(sql: &str, statements: &[Statement], cursor: usize) -> Range<usize> {
    let mut start = 0;
    for statement in statements {
        if !statement.terminated {
            break;
        }
        if statement.end_with_delimiter > cursor {
            return start..statement.end;
        }
        start = statement.end_with_delimiter;
    }
    start..sql.len()
}

/// Returns `None` when the cursor is in a comment, in a string, or in a number.
pub fn analyze(
    sql: &str,
    statements: &[Statement],
    cursor: usize,
    dialect: Dialect,
) -> Option<Analysis> {
    let region = statement_region(sql, statements, cursor);
    let text = &sql[region.clone()];
    let offset = cursor.saturating_sub(region.start).min(text.len());

    let mut tokens = tokenize(text, dialect);
    let in_literal = tokens.iter().any(|token| {
        matches!(token.kind, TokenKind::Comment | TokenKind::String)
            && token.start < offset
            && (offset < token.end || (offset == token.end && !token.closed))
    });
    if in_literal {
        return None;
    }
    tokens.retain(|token| token.kind != TokenKind::Comment);

    let partial = tokens.iter().position(|token| {
        token.start < offset
            && match token.kind {
                TokenKind::Word | TokenKind::Number => offset <= token.end,
                TokenKind::QuotedIdentifier => offset < token.end || !token.closed,
                _ => false,
            }
    });
    if partial.is_some_and(|index| tokens[index].kind == TokenKind::Number) {
        return None;
    }

    let boundary =
        partial.unwrap_or_else(|| tokens.iter().filter(|token| token.end <= offset).count());

    let mut head = boundary;
    let mut qualifiers = Vec::new();
    while head >= 2
        && is_punctuation(text, tokens[head - 1], b'.')
        && matches!(tokens[head - 2].kind, TokenKind::Word | TokenKind::QuotedIdentifier)
    {
        qualifiers.push(identifier(text, tokens[head - 2]));
        head -= 2;
    }
    if head >= 1 && is_punctuation(text, tokens[head - 1], b'.') {
        return None;
    }
    qualifiers.reverse();

    let (replace, prefix, quote) = match partial {
        Some(index) => {
            let token = tokens[index];
            let quoted = token.kind == TokenKind::QuotedIdentifier;
            let begin = token.start + usize::from(quoted);
            (
                region.start + token.start..region.start + token.end,
                text[begin..offset.max(begin)].to_owned(),
                quoted.then(|| text[token.start..].chars().next()).flatten(),
            )
        }
        None => (cursor..cursor, String::new(), None),
    };

    Some(Analysis {
        replace,
        prefix,
        quote,
        qualifiers,
        position: position(&tokens[..head], text),
        scope: visible_references(&tokens, text, head, partial),
    })
}

fn ends_operand(tokens: &[Token], text: &str) -> bool {
    let Some(&last) = tokens.last() else { return false };
    match last.kind {
        TokenKind::Number | TokenKind::String | TokenKind::QuotedIdentifier => true,
        TokenKind::Word => !is_reserved(raw(text, last)) || is_word(text, Some(&last), OPERAND_WORDS),
        TokenKind::Punctuation if is_punctuation(text, last, b')') => true,
        TokenKind::Punctuation if is_punctuation(text, last, b'*') => {
            let before = tokens.len().checked_sub(2).map(|index| tokens[index]);
            before.is_none_or(|token| {
                is_punctuation(text, token, b',') || is_word(text, Some(&token), &["SELECT", "DISTINCT"])
            })
        }
        _ => false,
    }
}

fn position(tokens: &[Token], text: &str) -> Position {
    let Some(last) = tokens.last() else { return Position::StatementStart };
    if is_word(text, Some(last), &["AS"]) {
        return Position::NewName;
    }

    let operand = ends_operand(tokens, text);
    let after_expression = || if operand { Position::AfterOperand } else { Position::Expression };
    let mut depth = 0usize;

    for (index, token) in tokens.iter().enumerate().rev() {
        if is_punctuation(text, *token, b')') {
            depth += 1;
        } else if is_punctuation(text, *token, b'(') {
            if depth == 0 {
                return match insert_target(&tokens[..index], text) {
                    Some(target) if !operand => Position::ColumnList(target),
                    _ => after_expression(),
                };
            }
            depth -= 1;
        } else if depth == 0 {
            if is_word(text, Some(token), RELATION_WORDS) {
                return relation_position(tokens, index, text);
            }
            if is_word(text, Some(token), EXPRESSION_WORDS) {
                return after_expression();
            }
            if index + 1 == tokens.len() && is_word(text, Some(token), LEADING_WORDS) {
                return Position::StatementStart;
            }
        }
    }
    Position::AfterOperand
}

fn relation_position(tokens: &[Token], index: usize, text: &str) -> Position {
    let creates = index > 0
        && is_word(text, Some(&tokens[index]), &["TABLE"])
        && is_word(text, Some(&tokens[index - 1]), &["CREATE"]);
    if creates {
        return Position::NewName;
    }
    if index + 1 == tokens.len() {
        return Position::Relation;
    }
    let after_comma = tokens.last().is_some_and(|last| is_punctuation(text, *last, b','));
    if after_comma && is_word(text, Some(&tokens[index]), &["FROM"]) {
        return Position::Relation;
    }
    Position::AfterOperand
}

/// Reads `INTO schema.relation` backwards, from the end of `tokens`.
fn insert_target(tokens: &[Token], text: &str) -> Option<Reference> {
    let mut index = tokens.len();
    let mut parts = Vec::new();
    while index >= 1 && is_identifier(text, tokens[index - 1]) {
        parts.push(identifier(text, tokens[index - 1]));
        index -= 1;
        if index >= 1 && is_punctuation(text, tokens[index - 1], b'.') {
            index -= 1;
        } else {
            break;
        }
    }
    if index == 0 || !is_word(text, Some(&tokens[index - 1]), &["INTO"]) {
        return None;
    }
    let mut parts = parts.into_iter();
    let name = parts.next()?;
    Some(Reference { schema: parts.next(), name: Some(name), alias: None })
}

struct Group {
    parent: Option<usize>,
    in_from: bool,
    derived: bool,
}

/// Returns the references that the cursor can see. A group is the text in one
/// pair of parentheses, and the cursor sees its own group and the outer groups.
fn visible_references(
    tokens: &[Token],
    text: &str,
    boundary: usize,
    partial: Option<usize>,
) -> Vec<Reference> {
    let reader = Reader { tokens, text, partial };
    let mut groups = vec![Group { parent: None, in_from: false, derived: false }];
    let mut current = 0;
    let mut cursor_group = None;
    let mut found: Vec<(usize, Reference)> = Vec::new();
    let mut index = 0;

    loop {
        if index >= boundary && cursor_group.is_none() {
            cursor_group = Some(current);
        }
        let Some(token) = tokens.get(index) else { break };

        if is_punctuation(text, *token, b'(') {
            let after_from_word = index > 0
                && groups[current].in_from
                && (is_punctuation(text, tokens[index - 1], b',')
                    || is_word(text, Some(&tokens[index - 1]), &["FROM", "JOIN"]));
            groups.push(Group { parent: Some(current), in_from: false, derived: after_from_word });
            current = groups.len() - 1;
        } else if is_punctuation(text, *token, b')') {
            if let Some(parent) = groups[current].parent {
                let derived = groups[current].derived;
                current = parent;
                if derived {
                    let (next, alias) = reader.alias(index + 1);
                    if alias.is_some() {
                        found.push((current, Reference { schema: None, name: None, alias }));
                    }
                    index = next;
                    continue;
                }
            }
        } else if is_punctuation(text, *token, b',') && groups[current].in_from {
            let (next, reference) = reader.reference(index + 1);
            found.extend(reference.map(|reference| (current, reference)));
            index = next;
            continue;
        } else if is_word(text, Some(token), RELATION_WORDS) {
            groups[current].in_from = is_word(text, Some(token), &["FROM", "JOIN"]);
            let (next, reference) = reader.reference(index + 1);
            found.extend(reference.map(|reference| (current, reference)));
            index = next;
            continue;
        } else if is_word(text, Some(token), END_OF_FROM_WORDS) {
            groups[current].in_from = false;
        }
        index += 1;
    }

    let mut chain = Vec::new();
    let mut group = cursor_group;
    while let Some(id) = group {
        chain.push(id);
        group = groups[id].parent;
    }
    found.into_iter().filter(|(id, _)| chain.contains(id)).map(|(_, found)| found).collect()
}

struct Reader<'a> {
    tokens: &'a [Token],
    text: &'a str,
    partial: Option<usize>,
}

impl Reader<'_> {
    fn is_name(&self, index: usize) -> bool {
        Some(index) != self.partial
            && self.tokens.get(index).is_some_and(|token| is_identifier(self.text, *token))
    }

    /// Reads `[schema.]name [[AS] alias]`.
    fn reference(&self, start: usize) -> (usize, Option<Reference>) {
        let mut index = start;
        if is_word(self.text, self.tokens.get(index), &["ONLY", "LATERAL"]) {
            index += 1;
        }
        let mut parts = Vec::new();
        while self.is_name(index) {
            parts.push(identifier(self.text, self.tokens[index]));
            index += 1;
            let dot = self.tokens.get(index);
            if !dot.is_some_and(|token| is_punctuation(self.text, *token, b'.')) {
                let (next, alias) = self.alias(index);
                let name = parts.pop();
                let schema = parts.pop();
                return (next, Some(Reference { schema, name, alias }));
            }
            index += 1;
        }
        (index, None)
    }

    fn alias(&self, start: usize) -> (usize, Option<String>) {
        let index = start + usize::from(is_word(self.text, self.tokens.get(start), &["AS"]));
        if self.is_name(index) {
            (index + 1, Some(identifier(self.text, self.tokens[index])))
        } else {
            (index, None)
        }
    }
}
