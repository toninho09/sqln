//! Local completion for the Kafka SELECT subset.

use super::{Builder, Completion};
use crate::catalog::{Catalog, Columns, RelationRef};
use crate::connector::kafka::command::{Token, TokenKind};
use crate::connector::kafka::filter::{Segment, parse_path};
use crate::connector::kafka::{MESSAGE_COLUMNS, TOPICS, command, filter};
use crate::sql::Dialect;

/// Suggests the words of a SELECT statement from `start` to `end`. A field path, as
/// `value.customer.`, reads the topic after `FROM`, also when `FROM` follows the cursor.
pub(super) fn suggest(
    sql: &str,
    (start, end): (usize, usize),
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
    if typed.contains('.') {
        let all = command::tokenize_mode(&sql[start..end.max(cursor)], true).ok();
        let topic = all
            .as_deref()
            .and_then(from_topic)
            .or_else(|| from_topic(&tokens));
        if let Some(completion) = suggest_path(typed, cursor, topic, catalog) {
            return completion;
        }
    }
    let mut builder = Builder::new(typed, None, Dialect::Kafka);
    let last = tokens.last();
    let last_is = |word| last.is_some_and(|token| token.is_word(word));
    let from = from_index(&tokens);
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
        if last_is("SELECT")
            || last.is_some_and(|token| token.is_operator(",") || token.is_operator("("))
        {
            words.extend(MESSAGE_COLUMNS);
            words.extend(filter::FUNCTION_NAMES);
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
                "KEY",
                "VALUE",
                "HEADERS",
                "TIMESTAMP",
                "OFFSET",
                "SCHEMA",
                "CONTAINS",
                "MATCHES",
                "LIKE",
                "ILIKE",
                "IN",
                "BETWEEN",
                "IS",
                "MISSING",
                "ANY",
                "EVERY",
                "SATISFIES",
                "END",
                "AND",
                "OR",
                "NOT",
                "NULL",
                "TRUE",
                "FALSE",
            ]);
            words.extend(filter::FUNCTION_NAMES);
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

/// Returns the index of the `FROM` of a SELECT. A `FROM` after `AS` is an alias.
fn from_index(tokens: &[Token]) -> Option<usize> {
    tokens.iter().enumerate().find_map(|(index, token)| {
        (token.is_word("FROM")
            && index
                .checked_sub(1)
                .is_none_or(|before| !tokens[before].is_word("AS")))
        .then_some(index)
    })
}

/// Returns the topic after the `FROM` of a SELECT.
fn from_topic(tokens: &[Token]) -> Option<&str> {
    let topic = tokens.get(from_index(tokens)? + 1)?;
    matches!(topic.kind, TokenKind::Word | TokenKind::Text).then_some(topic.text.as_str())
}

/// Suggests the names of a field path in a `CONSUME` or `TAIL` line that ends at the
/// cursor. The filter of a topic tab uses this form.
pub(super) fn suggest_line_path(
    line: &str,
    cursor: usize,
    catalog: Option<&Catalog>,
) -> Option<Completion> {
    let tokens = command::tokenize(line).ok()?;
    let [command, topic, .., word] = tokens.as_slice() else {
        return None;
    };
    let read = command.is_word("CONSUME") || command.is_word("TAIL");
    let named = matches!(topic.kind, TokenKind::Word | TokenKind::Text);
    if !read || !named || word.kind != TokenKind::Word || word.end != line.len() {
        return None;
    }
    suggest_path(&word.text, cursor, Some(&topic.text), catalog)
}

/// Suggests the next name of a field path, such as `customer` after `value.cu`. The names
/// come from the columns of the topic in the catalog. A topic without loaded columns asks
/// for them. It returns `None` when the word is not a path of `key` or `value`.
fn suggest_path(
    word: &str,
    cursor: usize,
    topic: Option<&str>,
    catalog: Option<&Catalog>,
) -> Option<Completion> {
    let (parent, typed) = word.rsplit_once('.')?;
    let (root, path) = parse_path(parent)?;
    if !["key", "value"]
        .iter()
        .any(|name| root.eq_ignore_ascii_case(name))
    {
        return None;
    }
    let replace = cursor - typed.len()..cursor;
    let mut completion = Completion {
        replace: replace.clone(),
        ..Completion::default()
    };
    if typed.contains(['"', '[', ']']) {
        return Some(completion);
    }
    let Some((_, relation)) = topic
        .zip(catalog)
        .and_then(|(topic, catalog)| catalog.relation(Some(TOPICS), topic))
    else {
        return Some(completion);
    };
    let columns = match &relation.columns {
        Columns::Loaded(columns) => columns,
        Columns::NotLoaded => {
            completion.missing_columns.push(RelationRef {
                schema: TOPICS.to_owned(),
                relation: relation.name.clone(),
            });
            return Some(completion);
        }
    };
    let mut builder = Builder::new(typed, None, Dialect::Kafka);
    for column in columns {
        let Some((column_root, steps)) = parse_path(column) else {
            continue;
        };
        if let Some((Segment::Name(name), before)) = steps.split_last()
            && column_root.eq_ignore_ascii_case(root)
            && before.len() == path.len()
            && path
                .iter()
                .zip(before)
                .all(|(typed, known)| same_step(typed, known))
        {
            builder.add_path_name(name);
        }
    }
    Some(builder.finish(replace, typed))
}

/// Returns true when a typed step reads the step of a known path. A known list item
/// `[*]` accepts `[*]`, `[n]`, and `.n`.
fn same_step(typed: &Segment, known: &Segment) -> bool {
    match (typed, known) {
        (Segment::Name(typed), Segment::Name(known)) => typed == known,
        (Segment::Any | Segment::Index(_), Segment::Any) => true,
        (Segment::Name(typed), Segment::Any) => typed.parse::<i64>().is_ok(),
        _ => false,
    }
}
