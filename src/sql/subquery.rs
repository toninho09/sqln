//! Finds a parenthesized SELECT or WITH query without changing its text.

use super::{Statement, Token, TokenKind};

/// Returns the smallest complete subquery at the cursor. Tokens belong to one statement.
/// Strings, quoted identifiers, and comments do not supply parentheses or keywords.
pub fn subquery_at(sql: &str, tokens: &[Token], cursor: usize) -> Option<Statement> {
    let mut opens = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Punctuation {
            continue;
        }
        match &sql[token.start()..token.end()] {
            "(" => opens.push(index),
            ")" => {
                let Some(open) = opens.pop() else {
                    continue;
                };
                if cursor < tokens[open].start() || cursor > token.start() {
                    continue;
                }
                let inner = &tokens[open + 1..index];
                let Some(first) = inner.iter().find(|item| item.kind != TokenKind::Comment) else {
                    continue;
                };
                let word = &sql[first.start()..first.end()];
                if first.kind != TokenKind::Word
                    || !(word.eq_ignore_ascii_case("SELECT") || word.eq_ignore_ascii_case("WITH"))
                {
                    continue;
                }
                if inner.iter().any(|item| {
                    (item.kind == TokenKind::Punctuation && &sql[item.start()..item.end()] == ";")
                        || (!item.closed && item.kind != TokenKind::Comment)
                }) {
                    return None;
                }
                let end = first.start() + sql[first.start()..token.start()].trim_end().len();
                return Some(Statement {
                    start: first.start(),
                    end,
                    end_with_delimiter: end,
                    terminated: false,
                });
            }
            _ => {}
        }
    }
    None
}
