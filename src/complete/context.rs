//! Reads the text around the cursor and finds the expected kind of name.

use std::ops::Range;

use super::keywords::is_reserved;
use crate::sql::{Dialect, Statement, Token, TokenKind, tokenize};

const RELATION_WORDS: &[&str] = &["FROM", "JOIN", "UPDATE", "INTO", "TABLE"];
const EXPRESSION_WORDS: &[&str] = &[
    "SELECT",
    "DISTINCT",
    "WHERE",
    "ON",
    "BY",
    "SET",
    "HAVING",
    "AND",
    "OR",
    "NOT",
    "WHEN",
    "THEN",
    "ELSE",
    "CASE",
    "RETURNING",
    "VALUES",
    "BETWEEN",
    "LIKE",
    "IN",
    "USING",
    "COLUMN",
];
const LEADING_WORDS: &[&str] = &["EXPLAIN", "ANALYZE", "VERBOSE"];
const END_OF_FROM_WORDS: &[&str] = &[
    "WHERE",
    "GROUP",
    "ORDER",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "ON",
    "USING",
    "SET",
    "UNION",
    "INTERSECT",
    "EXCEPT",
    "RETURNING",
    "VALUES",
    "WINDOW",
    "FETCH",
    "FOR",
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
    /// The names of `WITH` that the cursor can see, the innermost first.
    pub ctes: Vec<Cte>,
}

/// A name of a `WITH` clause. The columns are only the ones that the text shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cte {
    pub name: String,
    pub columns: Vec<String>,
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
            && words
                .iter()
                .any(|word| raw(text, *token).eq_ignore_ascii_case(word))
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
    let close = if value.starts_with('[') {
        ']'
    } else {
        value.chars().next().unwrap_or('"')
    };
    let inner = &value[1..];
    let inner = if token.closed {
        inner.strip_suffix(close).unwrap_or(inner)
    } else {
        inner
    };
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
        && matches!(
            tokens[head - 2].kind,
            TokenKind::Word | TokenKind::QuotedIdentifier
        )
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
        ctes: visible_ctes(&tokens, text, head, partial),
    })
}

fn ends_operand(tokens: &[Token], text: &str) -> bool {
    let Some(&last) = tokens.last() else {
        return false;
    };
    match last.kind {
        TokenKind::Number | TokenKind::String | TokenKind::QuotedIdentifier => true,
        TokenKind::Word => {
            !is_reserved(raw(text, last)) || is_word(text, Some(&last), OPERAND_WORDS)
        }
        TokenKind::Punctuation if is_punctuation(text, last, b')') => true,
        TokenKind::Punctuation if is_punctuation(text, last, b'*') => {
            let before = tokens.len().checked_sub(2).map(|index| tokens[index]);
            before.is_none_or(|token| {
                is_punctuation(text, token, b',')
                    || is_word(text, Some(&token), &["SELECT", "DISTINCT"])
            })
        }
        _ => false,
    }
}

fn position(tokens: &[Token], text: &str) -> Position {
    let Some(last) = tokens.last() else {
        return Position::StatementStart;
    };
    if is_word(text, Some(last), &["AS"]) {
        return Position::NewName;
    }

    let operand = ends_operand(tokens, text);
    let after_expression = || {
        if operand {
            Position::AfterOperand
        } else {
            Position::Expression
        }
    };
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
    let after_comma = tokens
        .last()
        .is_some_and(|last| is_punctuation(text, *last, b','));
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
    Some(Reference {
        schema: parts.next(),
        name: Some(name),
        alias: None,
    })
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
    let reader = Reader {
        tokens,
        text,
        partial,
    };
    let mut groups = vec![Group {
        parent: None,
        in_from: false,
        derived: false,
    }];
    let mut current = 0;
    let mut cursor_group = None;
    let mut found: Vec<(usize, Reference)> = Vec::new();
    let mut index = 0;

    loop {
        if index >= boundary && cursor_group.is_none() {
            cursor_group = Some(current);
        }
        let Some(token) = tokens.get(index) else {
            break;
        };

        if is_punctuation(text, *token, b'(') {
            let after_from_word = index > 0
                && groups[current].in_from
                && (is_punctuation(text, tokens[index - 1], b',')
                    || is_word(text, Some(&tokens[index - 1]), &["FROM", "JOIN"]));
            groups.push(Group {
                parent: Some(current),
                in_from: false,
                derived: after_from_word,
            });
            current = groups.len() - 1;
        } else if is_punctuation(text, *token, b')') {
            if let Some(parent) = groups[current].parent {
                let derived = groups[current].derived;
                current = parent;
                if derived {
                    let (next, alias) = reader.alias(index + 1);
                    if alias.is_some() {
                        found.push((
                            current,
                            Reference {
                                schema: None,
                                name: None,
                                alias,
                            },
                        ));
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
    found
        .into_iter()
        .filter(|(id, _)| chain.contains(id))
        .map(|(_, found)| found)
        .collect()
}

struct Reader<'a> {
    tokens: &'a [Token],
    text: &'a str,
    partial: Option<usize>,
}

impl Reader<'_> {
    fn is_name(&self, index: usize) -> bool {
        Some(index) != self.partial
            && self
                .tokens
                .get(index)
                .is_some_and(|token| is_identifier(self.text, *token))
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
                return (
                    next,
                    Some(Reference {
                        schema,
                        name,
                        alias,
                    }),
                );
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

/// Returns the index of the `)` of the `(` at `open`, or the end of an incomplete text.
fn closing(tokens: &[Token], text: &str, open: usize) -> usize {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if is_punctuation(text, *token, b'(') {
            depth += 1;
        } else if is_punctuation(text, *token, b')') {
            depth -= 1;
            if depth == 0 {
                return index;
            }
        }
    }
    tokens.len()
}

/// Returns the end of the text in the parentheses around `start`.
fn group_end(tokens: &[Token], text: &str, start: usize) -> usize {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        if is_punctuation(text, *token, b'(') {
            depth += 1;
        } else if is_punctuation(text, *token, b')') {
            if depth == 0 {
                return index;
            }
            depth -= 1;
        }
    }
    tokens.len()
}

/// Reads the column list after a name. It returns the list and the next index.
fn column_list(reader: &Reader<'_>, open: usize) -> Option<(Vec<String>, usize)> {
    let mut columns = Vec::new();
    let mut index = open + 1;
    loop {
        if !reader.is_name(index) {
            return None;
        }
        columns.push(identifier(reader.text, reader.tokens[index]));
        let next = reader.tokens.get(index + 1)?;
        index += 2;
        if is_punctuation(reader.text, *next, b')') {
            return Some((columns, index));
        }
        if !is_punctuation(reader.text, *next, b',') {
            return None;
        }
    }
}

/// Reads the name of one item of a select list, when the text shows it.
fn item_name(tokens: &[Token], text: &str) -> Option<String> {
    let last = *tokens.last()?;
    if !is_identifier(text, last) {
        return None;
    }
    let before = tokens.len().checked_sub(2).map(|index| tokens[index]);
    // `a`, `t.a`, `expression AS a`, and `expression a` all end with the name.
    let plain = tokens.len() == 1
        || before.is_some_and(|token| {
            is_punctuation(text, token, b'.') || is_word(text, Some(&token), &["AS"])
        });
    let after_operand = before.is_some_and(|token| {
        matches!(
            token.kind,
            TokenKind::Number | TokenKind::String | TokenKind::QuotedIdentifier
        ) || is_punctuation(text, token, b')')
            || (token.kind == TokenKind::Word && !is_reserved(raw(text, token)))
    });
    (plain || after_operand).then(|| identifier(text, last))
}

/// Reads the names of the first select list of a `WITH` body. An item without a clear
/// name, such as `*`, adds nothing.
fn select_columns(tokens: &[Token], text: &str) -> Vec<String> {
    if !is_word(text, tokens.first(), &["SELECT"]) {
        return Vec::new();
    }
    let mut columns = Vec::new();
    let mut depth = 0usize;
    let mut start = 1;
    let mut index = 1;
    while index < tokens.len() {
        let token = tokens[index];
        if is_punctuation(text, token, b'(') {
            depth += 1;
        } else if is_punctuation(text, token, b')') {
            depth = depth.saturating_sub(1);
        } else if depth == 0
            && is_word(
                text,
                Some(&token),
                &["FROM", "UNION", "INTERSECT", "EXCEPT"],
            )
        {
            break;
        } else if depth == 0 && is_punctuation(text, token, b',') {
            columns.extend(item_name(&tokens[start..index], text));
            start = index + 1;
        }
        index += 1;
    }
    columns.extend(item_name(&tokens[start..index], text));
    columns
}

struct WithItem {
    cte: Cte,
    /// The tokens between the parentheses of the body.
    body: Range<usize>,
}

/// Reads `name [(columns)] AS [[NOT] MATERIALIZED] (body)`. A text with another shape,
/// such as `TIMESTAMP WITH TIME ZONE`, is not a `WITH` clause.
fn with_item(reader: &Reader<'_>, start: usize) -> Option<(WithItem, usize)> {
    let Reader { tokens, text, .. } = *reader;
    if !reader.is_name(start) {
        return None;
    }
    let name = identifier(text, tokens[start]);
    let mut index = start + 1;
    let mut listed = None;
    if tokens
        .get(index)
        .is_some_and(|token| is_punctuation(text, *token, b'('))
    {
        let (columns, next) = column_list(reader, index)?;
        listed = Some(columns);
        index = next;
    }
    if !is_word(text, tokens.get(index), &["AS"]) {
        return None;
    }
    index += 1;
    index += usize::from(is_word(text, tokens.get(index), &["NOT"]));
    index += usize::from(is_word(text, tokens.get(index), &["MATERIALIZED"]));
    if !tokens
        .get(index)
        .is_some_and(|token| is_punctuation(text, *token, b'('))
    {
        return None;
    }
    let end = closing(tokens, text, index);
    let body = index + 1..end;
    let columns = listed.unwrap_or_else(|| select_columns(&tokens[body.clone()], text));
    Some((
        WithItem {
            cte: Cte { name, columns },
            body,
        },
        end + 1,
    ))
}

/// Returns the names of `WITH` at the cursor. A name is visible in the later items and
/// in the query of its clause. With `RECURSIVE`, it is also visible in its own body.
fn visible_ctes(tokens: &[Token], text: &str, boundary: usize, partial: Option<usize>) -> Vec<Cte> {
    let reader = Reader {
        tokens,
        text,
        partial,
    };
    let mut found: Vec<Cte> = Vec::new();
    for start in 0..tokens.len() {
        if !is_word(text, tokens.get(start), &["WITH"]) {
            continue;
        }
        let end = group_end(tokens, text, start);
        if boundary <= start || boundary > end {
            continue;
        }
        let recursive = is_word(text, tokens.get(start + 1), &["RECURSIVE"]);
        let mut index = start + 1 + usize::from(recursive);
        while let Some((item, next)) = with_item(&reader, index) {
            let after = boundary > item.body.end;
            let inside = recursive && item.body.contains(&boundary);
            if after || inside {
                // An inner clause hides an outer name, as in the database.
                found.retain(|cte| !cte.name.eq_ignore_ascii_case(&item.cte.name));
                found.insert(0, item.cte);
            }
            if !tokens
                .get(next)
                .is_some_and(|token| is_punctuation(text, *token, b','))
            {
                break;
            }
            index = next + 1;
        }
    }
    found
}
