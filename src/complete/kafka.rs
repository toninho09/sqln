//! Local completion for the Kafka SELECT subset.

use super::{Builder, Completion};
use crate::catalog::Catalog;
use crate::connector::kafka::{MESSAGE_COLUMNS, TOPICS, command};
use crate::sql::Dialect;

pub(super) fn suggest(
    sql: &str,
    start: usize,
    cursor: usize,
    catalog: Option<&Catalog>,
) -> Completion {
    let input = &sql[start..cursor];
    let empty = || Completion {
        replace: cursor..cursor,
        ..Completion::default()
    };
    let Ok(mut tokens) = command::tokenize_mode(input, true) else {
        return empty();
    };
    let typed = if !input.ends_with(char::is_whitespace)
        && tokens
            .last()
            .is_some_and(|token| token.kind == command::TokenKind::Word)
    {
        let token = tokens.pop().unwrap();
        &input[token.start..]
    } else {
        ""
    };
    let mut builder = Builder::new(typed, None, Dialect::Kafka);
    let last = tokens.last();
    let last_is = |word| last.is_some_and(|token| token.is_word(word));
    let from = tokens.iter().enumerate().find_map(|(index, token)| {
        (token.is_word("FROM")
            && index
                .checked_sub(1)
                .is_none_or(|before| !tokens[before].is_word("AS")))
        .then_some(index)
    });
    let clauses = from
        .and_then(|index| tokens.get(index + 2..))
        .unwrap_or(&[]);
    let has = |word| clauses.iter().any(|token| token.is_word(word));
    let clause_last_is = |word| !clauses.is_empty() && last_is(word);
    let mut words: Vec<&str> = Vec::new();
    if from.is_some_and(|index| index + 1 == tokens.len()) {
        for relation in catalog
            .and_then(|catalog| catalog.schema(TOPICS))
            .map_or(&[][..], |schema| schema.relations.as_slice())
        {
            builder.add_command_name(&relation.name);
        }
    } else if from.is_none() {
        if last_is("SELECT") || last.is_some_and(|token| token.is_operator(",")) {
            words.extend(MESSAGE_COLUMNS);
            words.push("*");
        } else if !last_is("AS") {
            if !last.is_some_and(|token| token.is_operator("*"))
                && !tokens
                    .iter()
                    .rev()
                    .nth(1)
                    .is_some_and(|token| token.is_word("AS"))
            {
                words.push("AS");
            }
            words.push("FROM");
        }
    } else if clause_last_is("START") {
        words.push("AT");
    } else if clause_last_is("AT") {
        words.extend(["EARLIEST", "LATEST", "OFFSET", "TIMESTAMP"]);
    } else if clause_last_is("EMIT") {
        words.push("CHANGES");
    } else if ["PARTITION", "OFFSET", "TIMESTAMP", "LIMIT", "SCAN"]
        .iter()
        .any(|word| clause_last_is(word))
        || clause_last_is("CHANGES") && has("EMIT")
    {
        return empty();
    } else {
        if has("WHERE") {
            words.extend([
                "KEY", "VALUE", "HEADERS", "CONTAINS", "MATCHES", "AND", "OR", "NOT", "NULL",
                "TRUE", "FALSE",
            ]);
        }
        for clause in ["PARTITION", "START", "WHERE", "LIMIT", "SCAN", "EMIT"] {
            if !has(clause) && !(clause == "EMIT" && (has("LIMIT") || has("SCAN"))) {
                words.push(clause);
            }
        }
    }
    for word in words {
        builder.add_keyword(word);
    }
    builder.finish(cursor - typed.len()..cursor, typed)
}
