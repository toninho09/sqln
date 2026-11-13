//! Local completion for the Kafka SELECT subset.

use super::{Builder, Completion, keywords};
use crate::catalog::{Catalog, Columns, Relation, RelationRef};
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
    let Some(typed) = Typed::read(input, true) else {
        return empty();
    };
    let all = command::tokenize_mode(&sql[start..end.max(cursor)], true).ok();
    let topic = all
        .as_deref()
        .and_then(from_topic)
        .or_else(|| from_topic(&typed.tokens));
    if let Some(completion) = suggest_field(&typed, cursor, topic, catalog) {
        return completion;
    }
    let (tokens, typed) = (&typed.tokens, typed.word);
    let mut builder = Builder::new(typed, None, Dialect::Kafka);
    add_values(&mut builder, tokens, topic, catalog);
    let last = tokens.last();
    let last_is = |word| last.is_some_and(|token| token.is_word(word));
    let from = from_index(tokens);
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

/// Suggests the words after the topic of a line command: field paths, the values of a
/// compared field, and the clause words. The filter of a topic tab uses this form.
pub(super) fn suggest_line(line: &str, cursor: usize, catalog: Option<&Catalog>) -> Completion {
    let Some(typed) = Typed::read(line, false) else {
        return Completion {
            replace: cursor..cursor,
            ..Completion::default()
        };
    };
    let topic = match typed.tokens.as_slice() {
        [command, topic, ..]
            if (command.is_word("CONSUME") || command.is_word("TAIL"))
                && matches!(topic.kind, TokenKind::Word | TokenKind::Text) =>
        {
            Some(topic.text.as_str())
        }
        _ => None,
    };
    if let Some(completion) = suggest_field(&typed, cursor, topic, catalog) {
        return completion;
    }
    let mut builder = Builder::new(typed.word, None, Dialect::Kafka);
    add_values(&mut builder, &typed.tokens, topic, catalog);
    let plain = typed
        .word
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    if plain {
        for word in keywords::KAFKA_CLAUSES {
            builder.add_keyword(word);
        }
    }
    builder.finish(cursor - typed.word.len()..cursor, typed.word)
}

/// The text before the cursor: the tokens before the typed word, and the typed word.
struct Typed<'a> {
    tokens: Vec<Token>,
    /// The text that a suggestion replaces. A value can start with an open quote.
    word: &'a str,
    /// The typed text without the open quote.
    prefix: &'a str,
    /// True when the word starts with a quote that the text does not close.
    quoted: bool,
}

impl<'a> Typed<'a> {
    /// Reads the text before the cursor. A `'` that the text does not close starts the
    /// typed word, as in `value.state = 'AC`.
    fn read(input: &'a str, select: bool) -> Option<Self> {
        let mut tokens = match command::tokenize_mode(input, select) {
            Ok(tokens) => tokens,
            Err(error) => {
                let at = error
                    .offset
                    .filter(|&at| input.as_bytes().get(at) == Some(&b'\''))?;
                // A closing quote at the end must complete the text.
                command::tokenize_mode(&format!("{input}'"), select).ok()?;
                return Some(Self {
                    tokens: command::tokenize_mode(&input[..at], select).ok()?,
                    word: &input[at..],
                    prefix: &input[at + 1..],
                    quoted: true,
                });
            }
        };
        let word = match tokens.last() {
            Some(last) if last.kind == TokenKind::Word && !input.ends_with(char::is_whitespace) => {
                let start = last.start;
                tokens.pop();
                &input[start..]
            }
            _ => "",
        };
        Some(Self {
            tokens,
            word,
            prefix: word,
            quoted: false,
        })
    }
}

/// Suggests the names of a field path, or only the values after an open quote. It
/// returns `None` when the typed word needs the other words.
fn suggest_field(
    typed: &Typed<'_>,
    cursor: usize,
    topic: Option<&str>,
    catalog: Option<&Catalog>,
) -> Option<Completion> {
    if typed.quoted {
        let mut builder = Builder::new(typed.prefix, None, Dialect::Kafka);
        add_values(&mut builder, &typed.tokens, topic, catalog);
        return Some(builder.finish(cursor - typed.word.len()..cursor, typed.word));
    }
    if !typed.word.contains('.') {
        return None;
    }
    let items = open_items(&typed.tokens);
    suggest_path(typed.word, cursor, topic, &items, catalog)
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

/// Returns the items of the `ANY` and `EVERY` that are open at the end of the tokens,
/// from the outer one: the name of the item and its list field.
fn open_items(tokens: &[Token]) -> Vec<(&str, &str)> {
    let mut open = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.is_word("END") {
            open.pop();
        } else if let [quantifier, name, word_in, list, satisfies, ..] = &tokens[index..]
            && (quantifier.is_word("ANY") || quantifier.is_word("EVERY"))
            && name.kind == TokenKind::Word
            && word_in.is_word("IN")
            && list.kind == TokenKind::Word
            && satisfies.is_word("SATISFIES")
        {
            open.push((name.text.as_str(), list.text.as_str()));
        }
    }
    open
}

/// Writes a field word with the list field of each open item in place of its name, as
/// `value.lines[*].sku` for `i.sku` in `ANY i IN value.lines SATISFIES`. It returns
/// `None` when the root is not `key`, `value`, or an open item.
fn resolve(word: &str, items: &[(&str, &str)]) -> Option<String> {
    let root = &word[..word.find(['.', '[']).unwrap_or(word.len())];
    if ["key", "value"]
        .iter()
        .any(|name| root.eq_ignore_ascii_case(name))
    {
        return Some(word.to_owned());
    }
    let at = items
        .iter()
        .rposition(|(name, _)| name.eq_ignore_ascii_case(root))?;
    let list = resolve(items[at].1, &items[..at])?;
    Some(format!("{list}[*]{}", &word[root.len()..]))
}

/// Returns the topic and its columns. A topic without loaded columns asks for them.
fn loaded<'a>(
    topic: Option<&str>,
    catalog: Option<&'a Catalog>,
    missing: &mut Vec<RelationRef>,
) -> Option<(&'a Relation, &'a [String])> {
    let (_, relation) = catalog?.relation(Some(TOPICS), topic?)?;
    match &relation.columns {
        Columns::Loaded(columns) => Some((relation, columns)),
        Columns::NotLoaded => {
            let wanted = RelationRef {
                schema: TOPICS.to_owned(),
                relation: relation.name.clone(),
            };
            if !missing.contains(&wanted) {
                missing.push(wanted);
            }
            None
        }
    }
}

/// Returns the known columns of a topic below a path, with their index: the column
/// path has the same root and `extra` more steps.
fn columns_below<'a>(
    columns: &'a [String],
    root: &'a str,
    path: &'a [Segment],
    extra: usize,
) -> impl Iterator<Item = (usize, Vec<Segment>)> + 'a {
    columns
        .iter()
        .enumerate()
        .filter_map(move |(index, column)| {
            let (column_root, steps) = parse_path(column)?;
            (column_root.eq_ignore_ascii_case(root)
                && steps.len() == path.len() + extra
                && path
                    .iter()
                    .zip(&steps)
                    .all(|(typed, known)| same_step(typed, known)))
            .then_some((index, steps))
        })
}

/// Suggests the next name of a field path, such as `customer` after `value.cu`. The names
/// and their types come from the columns of the topic in the catalog. A topic without
/// loaded columns asks for them. It returns `None` when the word is not a path of `key`,
/// `value`, or an open item.
fn suggest_path(
    word: &str,
    cursor: usize,
    topic: Option<&str>,
    items: &[(&str, &str)],
    catalog: Option<&Catalog>,
) -> Option<Completion> {
    let (parent, typed) = word.rsplit_once('.')?;
    let parent = resolve(parent, items)?;
    let (root, path) = parse_path(&parent)?;
    let replace = cursor - typed.len()..cursor;
    let mut builder = Builder::new(typed, None, Dialect::Kafka);
    if typed.contains(['"', '[', ']']) {
        return Some(builder.finish(replace, typed));
    }
    if let Some((relation, columns)) = loaded(topic, catalog, &mut builder.missing_columns) {
        for (index, steps) in columns_below(columns, root, &path, 1) {
            if let Some(Segment::Name(name)) = steps.last() {
                let detail = relation.notes.get(index).map(|note| note.detail.as_str());
                builder.add_path_name(name, detail);
            }
        }
    }
    Some(builder.finish(replace, typed))
}

/// Adds the values of the field that the tokens compare at their end, as in
/// `value.state =`, `value.state !=`, or `value.state IN ('A',`.
fn add_values(
    builder: &mut Builder,
    tokens: &[Token],
    topic: Option<&str>,
    catalog: Option<&Catalog>,
) {
    let Some(field) = compared_field(tokens) else {
        return;
    };
    let items = open_items(tokens);
    let Some(word) = resolve(&field.text, &items) else {
        return;
    };
    let Some((root, path)) = parse_path(&word) else {
        return;
    };
    let Some((relation, columns)) = loaded(topic, catalog, &mut builder.missing_columns) else {
        return;
    };
    for (index, _) in columns_below(columns, root, &path, 0) {
        for value in relation
            .notes
            .get(index)
            .map_or(&[][..], |note| &note.values)
        {
            builder.add_value(value);
        }
    }
}

/// Returns the field before `=`, `!=`, `IN (`, or `NOT IN (` at the end of the tokens. In
/// a list, the items before the cursor stay out, as in `IN ('A', 'B',`.
fn compared_field(tokens: &[Token]) -> Option<&Token> {
    let mut end = tokens.len();
    if tokens.last()?.is_operator(",") {
        while end >= 2
            && tokens[end - 1].is_operator(",")
            && matches!(tokens[end - 2].kind, TokenKind::Text | TokenKind::Word)
        {
            end -= 2;
        }
        if !tokens.get(end.checked_sub(1)?)?.is_operator("(") {
            return None;
        }
    }
    let field = match &tokens[..end] {
        [.., field, not, word_in, open]
            if open.is_operator("(") && word_in.is_word("IN") && not.is_word("NOT") =>
        {
            field
        }
        [.., field, word_in, open] if open.is_operator("(") && word_in.is_word("IN") => field,
        [.., field, operator] if operator.is_operator("=") || operator.is_operator("!=") => field,
        _ => return None,
    };
    (field.kind == TokenKind::Word).then_some(field)
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
