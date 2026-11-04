//! Context rules for incomplete definitions, casts, and query clauses.

use super::context::{Position, is_identifier, is_punctuation, is_word};
use crate::sql::{Dialect, Token};

fn words(words: &[&'static str]) -> Position {
    Position::Keywords(words.to_vec())
}

/// Keeps the tokens at one depth. Closed groups count as a single operand.
fn level(tokens: &[Token], text: &str) -> Vec<Token> {
    let mut depth = 0usize;
    let mut result = Vec::new();
    for &token in tokens {
        if is_punctuation(text, token, b'(') {
            depth += 1;
        } else if is_punctuation(text, token, b')') {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                result.push(token);
            }
        } else if depth == 0 {
            result.push(token);
        }
    }
    result
}

pub fn position(tokens: &[Token], text: &str, dialect: Dialect, fallback: Position) -> Position {
    let mut opens = Vec::new();
    for (index, &token) in tokens.iter().enumerate() {
        if is_punctuation(text, token, b'(') {
            opens.push(index);
        } else if is_punctuation(text, token, b')') {
            opens.pop();
        }
    }

    if let Some(&open) = opens.last()
        && open > 0
        && is_word(text, tokens.get(open - 1), &["CAST"])
    {
        let body = level(&tokens[open + 1..], text);
        if let Some(index) = body
            .iter()
            .rposition(|token| is_word(text, Some(token), &["AS"]))
        {
            if index + 1 == body.len() {
                return Position::DataType { cast: true };
            }
            return type_continuation(&body[index + 1..], text, dialect, true)
                .unwrap_or(Position::NewName);
        }
    }

    if let Some(position) = definition(tokens, text, dialect, &opens) {
        return position;
    }

    let start = opens.last().map_or(0, |open| open + 1);
    let current = level(&tokens[start..], text);
    query(&current, text, dialect, &fallback).unwrap_or_else(|| {
        if !opens.is_empty() && matches!(fallback, Position::AfterOperand) {
            Position::Keywords(operators(dialect))
        } else {
            fallback
        }
    })
}

fn type_continuation(
    tokens: &[Token],
    text: &str,
    dialect: Dialect,
    cast: bool,
) -> Option<Position> {
    let mut prefix = tokens
        .iter()
        .map(|token| &text[token.start()..token.end()])
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase();
    prefix.push(' ');
    let suffixes: Vec<_> = super::vocabulary::available_types(dialect, cast)
        .filter_map(|word| word.strip_prefix(&prefix))
        .collect();
    (!suffixes.is_empty()).then_some(Position::Keywords(suffixes))
}

fn operators(dialect: Dialect) -> Vec<&'static str> {
    let mut words = vec![
        "AND",
        "OR",
        "BETWEEN",
        "IN",
        "NOT IN",
        "LIKE",
        "NOT LIKE",
        "IS NULL",
        "IS NOT NULL",
    ];
    if dialect == Dialect::Postgres {
        words.extend(["ILIKE", "NOT ILIKE"]);
    }
    words
}

fn table_name_end(tokens: &[Token], text: &str, mut index: usize) -> Option<usize> {
    while is_word(text, tokens.get(index), &["IF", "NOT", "EXISTS", "ONLY"]) {
        index += 1;
    }
    if !tokens
        .get(index)
        .is_some_and(|&token| is_identifier(text, token))
    {
        return None;
    }
    index += 1;
    while tokens
        .get(index)
        .is_some_and(|&token| is_punctuation(text, token, b'.'))
        && tokens
            .get(index + 1)
            .is_some_and(|&token| is_identifier(text, token))
    {
        index += 2;
    }
    Some(index)
}

fn definition(tokens: &[Token], text: &str, dialect: Dialect, opens: &[usize]) -> Option<Position> {
    let creates = is_word(text, tokens.first(), &["CREATE"]);
    let alters = is_word(text, tokens.first(), &["ALTER"]);
    if !creates && !alters {
        return None;
    }
    let table = tokens
        .iter()
        .position(|token| is_word(text, Some(token), &["TABLE"]))?;
    if table > 2 {
        return None;
    }
    let Some(end) = table_name_end(tokens, text, table + 1) else {
        return Some(if creates {
            Position::NewName
        } else {
            Position::Relation
        });
    };
    if creates {
        let &open = opens.first()?;
        if open != end {
            return None;
        }
        let body = level(&tokens[open + 1..], text);
        let segment = body
            .rsplit(|token| is_punctuation(text, *token, b','))
            .next()?;
        if opens.len() > 1 {
            // Expressions in DEFAULT and CHECK use the regular expression context.
            return if segment
                .iter()
                .any(|token| is_word(text, Some(token), &["DEFAULT", "CHECK"]))
            {
                None
            } else {
                Some(Position::NewName)
            };
        }
        return Some(column(segment, text, dialect));
    }

    let tail = &tokens[end..];
    if tail.is_empty() {
        return Some(words(match dialect {
            Dialect::Postgres => &[
                "ADD COLUMN",
                "DROP COLUMN",
                "ALTER COLUMN",
                "RENAME COLUMN",
                "RENAME TO",
                "ADD CONSTRAINT",
                "DROP CONSTRAINT",
            ],
            Dialect::MySql => &[
                "ADD COLUMN",
                "DROP COLUMN",
                "MODIFY COLUMN",
                "CHANGE COLUMN",
                "RENAME COLUMN",
                "RENAME TO",
                "ADD CONSTRAINT",
                "DROP FOREIGN KEY",
            ],
            Dialect::Sqlite => &["ADD COLUMN", "DROP COLUMN", "RENAME COLUMN", "RENAME TO"],
            Dialect::Redis => &[],
        }));
    }
    if is_word(text, tail.first(), &["ADD"])
        || (dialect == Dialect::MySql && is_word(text, tail.first(), &["MODIFY", "CHANGE"]))
    {
        let mut start = 1;
        if is_word(text, tail.get(start), &["COLUMN"]) {
            start += 1;
        }
        while is_word(text, tail.get(start), &["IF", "NOT", "EXISTS"]) {
            start += 1;
        }
        if is_word(text, tail.first(), &["CHANGE"]) && tail.get(start).is_some() {
            start += 1;
        }
        if opens.is_empty() {
            return Some(column(&tail[start..], text, dialect));
        }
        if !tail[start..]
            .iter()
            .any(|token| is_word(text, Some(token), &["DEFAULT", "CHECK"]))
        {
            return Some(Position::NewName);
        }
    }
    if dialect == Dialect::Postgres && is_word(text, tail.first(), &["ALTER"]) {
        if !opens.is_empty()
            && tail
                .iter()
                .any(|token| is_word(text, Some(token), &["TYPE"]))
            && !tail
                .iter()
                .any(|token| is_word(text, Some(token), &["USING"]))
        {
            return Some(Position::NewName);
        }
        if is_word(text, tail.last(), &["TYPE"]) {
            return Some(Position::DataType { cast: true });
        }
        if is_word(text, tail.last(), &["COLUMN"]) {
            return Some(Position::NewName);
        }
        if tail.len() >= 2 && is_identifier(text, *tail.last()?) {
            return Some(words(&[
                "TYPE",
                "SET DEFAULT",
                "DROP DEFAULT",
                "SET NOT NULL",
                "DROP NOT NULL",
            ]));
        }
    }
    None
}

fn column(tokens: &[Token], text: &str, dialect: Dialect) -> Position {
    let Some(&first) = tokens.first() else {
        return Position::NewName;
    };
    if is_word(text, tokens.last(), &["PRIMARY", "FOREIGN"]) {
        return words(&["KEY"]);
    }
    if is_word(text, tokens.last(), &["NOT"]) {
        return words(&["NULL"]);
    }
    if is_word(text, tokens.last(), &["REFERENCES"]) {
        return Position::Relation;
    }
    if !is_identifier(text, first) {
        return Position::NewName;
    }
    if tokens.len() == 1 {
        return Position::DataType { cast: false };
    }
    if is_word(text, tokens.last(), &["DEFAULT"]) {
        return Position::Expression;
    }
    if let Some(position) = type_continuation(&tokens[1..], text, dialect, false) {
        return position;
    }
    let integer = is_word(
        text,
        tokens.get(1),
        &[
            "TINYINT",
            "SMALLINT",
            "MEDIUMINT",
            "INT",
            "INTEGER",
            "BIGINT",
        ],
    );
    let mut options = vec![
        "NOT NULL",
        "NULL",
        "DEFAULT",
        "PRIMARY KEY",
        "UNIQUE",
        "CHECK",
        "REFERENCES",
        "COLLATE",
    ];
    match dialect {
        Dialect::Postgres if integer => options.push("GENERATED ALWAYS AS IDENTITY"),
        Dialect::MySql => {
            options.push("COMMENT");
            if integer {
                options.extend(["AUTO_INCREMENT", "UNSIGNED"]);
            }
        }
        Dialect::Sqlite => {
            if tokens
                .iter()
                .any(|token| is_word(text, Some(token), &["INTEGER"]))
                && is_word(text, tokens.last(), &["KEY"])
            {
                options.push("AUTOINCREMENT");
            }
        }
        Dialect::Postgres | Dialect::Redis => {}
    }
    options.retain(|option| {
        let word = option.split_whitespace().next().unwrap_or(option);
        !tokens[1..]
            .iter()
            .any(|token| is_word(text, Some(token), &[word]))
            && !(*option == "NULL"
                && tokens[1..]
                    .iter()
                    .any(|token| is_word(text, Some(token), &["NOT"])))
    });
    Position::Keywords(options)
}

fn query(tokens: &[Token], text: &str, dialect: Dialect, fallback: &Position) -> Option<Position> {
    let select = tokens
        .iter()
        .position(|token| is_word(text, Some(token), &["SELECT"]))?;
    let tokens = &tokens[select..];
    let last = tokens.last();
    if is_word(text, last, &["UNION"]) {
        return Some(words(&["ALL", "SELECT"]));
    }
    if is_word(text, last, &["INTERSECT", "EXCEPT"])
        || (is_word(text, last, &["ALL"])
            && tokens.len() >= 2
            && is_word(
                text,
                tokens.get(tokens.len() - 2),
                &["UNION", "INTERSECT", "EXCEPT"],
            ))
    {
        return Some(words(&["SELECT"]));
    }
    if is_word(text, last, &["GROUP", "ORDER"]) {
        return Some(words(&["BY"]));
    }
    if is_word(text, last, &["NULLS"]) && dialect != Dialect::MySql {
        return Some(words(&["FIRST", "LAST"]));
    }
    if is_word(text, last, &["IS"]) {
        return Some(words(&["NULL", "NOT NULL", "TRUE", "FALSE"]));
    }
    if is_word(text, last, &["IS", "NOT"])
        && tokens
            .iter()
            .rev()
            .skip(1)
            .take(1)
            .any(|token| is_word(text, Some(token), &["IS"]))
    {
        return Some(words(&["NULL", "TRUE", "FALSE"]));
    }
    if is_word(text, last, &["LEFT", "RIGHT", "FULL", "INNER", "CROSS"]) {
        return Some(words(&["JOIN"]));
    }
    if is_word(text, last, &["NOT"]) && !matches!(fallback, Position::NewName) {
        return Some(words(if dialect == Dialect::Postgres {
            &["IN", "LIKE", "ILIKE", "BETWEEN"]
        } else {
            &["IN", "LIKE", "BETWEEN"]
        }));
    }
    if !matches!(fallback, Position::AfterOperand)
        && !is_word(text, last, &["ASC", "DESC", "FIRST", "LAST"])
    {
        return None;
    }
    let clause = tokens.iter().rposition(|token| {
        is_word(
            text,
            Some(token),
            &[
                "SELECT", "FROM", "JOIN", "WHERE", "ON", "GROUP", "HAVING", "ORDER", "LIMIT",
                "OFFSET",
            ],
        )
    })?;
    let token = &tokens[clause];
    let mut result = Vec::new();
    if is_word(text, Some(token), &["SELECT", "WHERE", "ON", "HAVING"]) {
        result.extend(operators(dialect));
    }
    if is_word(text, Some(token), &["SELECT"]) {
        result.extend(["AS", "FROM"]);
    }
    if is_word(text, Some(token), &["JOIN"])
        && (clause == 0 || !is_word(text, tokens.get(clause - 1), &["CROSS", "NATURAL"]))
    {
        return Some(words(&["ON", "USING"]));
    }
    if is_word(text, Some(token), &["FROM", "JOIN", "ON"]) {
        result.extend([
            "JOIN",
            "INNER JOIN",
            "LEFT JOIN",
            "RIGHT JOIN",
            "CROSS JOIN",
            "WHERE",
        ]);
        if dialect != Dialect::MySql {
            result.push("FULL JOIN");
        } else {
            result.push("STRAIGHT_JOIN");
        }
    }
    if is_word(
        text,
        Some(token),
        &["SELECT", "FROM", "JOIN", "ON", "WHERE"],
    ) {
        result.push("GROUP BY");
    }
    if is_word(text, Some(token), &["GROUP"]) {
        result.push("HAVING");
    }
    if !is_word(text, Some(token), &["ORDER", "LIMIT", "OFFSET"]) {
        result.push("ORDER BY");
    }
    if is_word(text, Some(token), &["ORDER"]) {
        if !is_word(text, last, &["ASC", "DESC", "FIRST", "LAST"]) {
            result.extend(["ASC", "DESC"]);
        }
        if dialect != Dialect::MySql && !is_word(text, last, &["FIRST", "LAST"]) {
            result.extend(["NULLS FIRST", "NULLS LAST"]);
        }
    }
    if !is_word(text, Some(token), &["LIMIT", "OFFSET"]) {
        result.push("LIMIT");
    }
    if is_word(text, Some(token), &["LIMIT"]) {
        result.push("OFFSET");
    }
    if !is_word(text, Some(token), &["OFFSET"]) {
        result.extend(["UNION", "UNION ALL", "INTERSECT", "EXCEPT"]);
    }
    Some(Position::Keywords(result))
}
