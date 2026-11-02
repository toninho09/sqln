//! Suggestions from the text, the cursor, the catalog, and the dialect.
//! This module reads only the cache, so it never queries the database.

mod context;
pub mod keywords;
#[cfg(test)]
mod tests;

use std::ops::Range;

use self::context::{Analysis, Cte, JoinOn, Position, Reference, analyze};
use self::keywords::is_reserved;
use crate::catalog::{Catalog, Columns, Relation, RelationKind, RelationRef, Schema};
use crate::sql::{Dialect, Statement, quote_with, split};

const MAX_ITEMS: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SuggestionKind {
    /// The condition of a foreign key. It leads the list.
    Join,
    Column,
    Alias,
    Cte,
    Table,
    View,
    Schema,
    Keyword,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub label: String,
    pub insert: String,
    pub kind: SuggestionKind,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Completion {
    pub items: Vec<Suggestion>,
    /// Byte range that the accepted suggestion replaces.
    pub replace: Range<usize>,
    /// Relations in use with columns that the cache does not hold yet.
    pub missing_columns: Vec<RelationRef>,
}

pub fn suggest(
    sql: &str,
    cursor: usize,
    dialect: Dialect,
    catalog: Option<&Catalog>,
) -> Completion {
    suggest_in(sql, &split(sql, dialect), cursor, dialect, catalog)
}

/// Works as `suggest`, for a caller that has the statements of the text.
pub fn suggest_in(
    sql: &str,
    statements: &[Statement],
    cursor: usize,
    dialect: Dialect,
    catalog: Option<&Catalog>,
) -> Completion {
    if dialect == Dialect::Redis {
        return suggest_command(sql, cursor);
    }
    let Some(analysis) = analyze(sql, statements, cursor, dialect) else {
        return Completion {
            replace: cursor..cursor,
            ..Completion::default()
        };
    };
    let typed = sql[analysis.replace.clone()].to_owned();
    let mut builder = Builder::new(&analysis.prefix, analysis.quote, dialect);
    builder.fill(&analysis, catalog);
    builder.finish(analysis.replace, &typed)
}

/// Suggests a command at the start of a line. The arguments have no suggestions.
fn suggest_command(sql: &str, cursor: usize) -> Completion {
    let line = sql[..cursor].rsplit('\n').next().unwrap_or("");
    let typed = line.trim_start();
    if !typed
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Completion {
            replace: cursor..cursor,
            ..Completion::default()
        };
    }
    let mut builder = Builder::new(typed, None, Dialect::Redis);
    keywords::starters(Dialect::Redis).for_each(|word| builder.add_keyword(word));
    builder.finish(cursor - typed.len()..cursor, typed)
}

struct Ranked {
    rank: u8,
    suggestion: Suggestion,
}

struct Builder {
    needle: String,
    uppercase_keywords: bool,
    quote: Option<char>,
    dialect: Dialect,
    items: Vec<Ranked>,
    missing_columns: Vec<RelationRef>,
}

impl Builder {
    fn new(prefix: &str, quote: Option<char>, dialect: Dialect) -> Self {
        let typed_lowercase =
            prefix.chars().any(char::is_lowercase) && !prefix.chars().any(char::is_uppercase);
        Self {
            needle: prefix.to_lowercase(),
            uppercase_keywords: !typed_lowercase,
            quote,
            dialect,
            items: Vec::new(),
            missing_columns: Vec::new(),
        }
    }

    fn fill(&mut self, analysis: &Analysis, catalog: Option<&Catalog>) {
        if !analysis.qualifiers.is_empty() {
            self.add_qualified(analysis, catalog);
            return;
        }
        match &analysis.position {
            Position::StatementStart => {
                keywords::starters(self.dialect).for_each(|word| self.add_keyword(word));
            }
            Position::Relation => {
                for cte in &analysis.ctes {
                    self.add_name(&cte.name, SuggestionKind::Cte, None);
                }
                if let Some(catalog) = catalog {
                    self.add_relations(catalog);
                }
            }
            Position::Expression => {
                if let Some((join, catalog)) = analysis.join_on.as_ref().zip(catalog) {
                    self.add_join_conditions(join, &analysis.ctes, catalog);
                }
                self.add_scope(analysis, catalog);
                keywords::all(self.dialect).for_each(|word| self.add_keyword(word));
            }
            Position::AfterOperand => {
                keywords::all(self.dialect).for_each(|word| self.add_keyword(word));
            }
            Position::ColumnList(target) => {
                if let Some(catalog) = catalog {
                    self.add_columns_of(target, catalog, None);
                }
            }
            Position::NewName => {}
        }
    }

    /// Returns 0 for a match at the start and 1 for a match in the middle.
    fn rank(&self, name: &str, allow_middle: bool) -> Option<u8> {
        let name = name.to_lowercase();
        if name.starts_with(&self.needle) {
            Some(0)
        } else {
            (allow_middle && name.contains(&self.needle)).then_some(1)
        }
    }

    fn push(&mut self, rank: u8, suggestion: Suggestion) {
        self.items.push(Ranked { rank, suggestion });
    }

    fn add_keyword(&mut self, word: &str) {
        let Some(rank) = self.rank(word, false) else {
            return;
        };
        let text = if self.uppercase_keywords {
            word.to_owned()
        } else {
            word.to_lowercase()
        };
        self.push(
            rank,
            Suggestion {
                label: text.clone(),
                insert: text,
                kind: SuggestionKind::Keyword,
                detail: None,
            },
        );
    }

    fn add_name(&mut self, name: &str, kind: SuggestionKind, detail: Option<&str>) {
        let Some(rank) = self.rank(name, true) else {
            return;
        };
        self.push(
            rank,
            Suggestion {
                label: name.to_owned(),
                insert: quote_name(name, self.dialect, self.quote),
                kind,
                detail: detail.map(str::to_owned),
            },
        );
    }

    fn add_relations(&mut self, catalog: &Catalog) {
        for schema in &catalog.schemas {
            self.add_name(&schema.name, SuggestionKind::Schema, None);
            for relation in &schema.relations {
                if catalog.is_current(schema) {
                    self.add_name(&relation.name, relation_kind(relation), Some(&schema.name));
                } else {
                    self.add_other_schema_relation(schema, relation);
                }
            }
        }
    }

    /// Suggests the qualified name of a relation out of the current schema.
    fn add_other_schema_relation(&mut self, schema: &Schema, relation: &Relation) {
        let label = format!("{}.{}", schema.name, relation.name);
        let Some(rank) = self
            .rank(&relation.name, true)
            .or_else(|| self.rank(&label, false))
        else {
            return;
        };
        let insert = format!(
            "{}.{}",
            quote_name(&schema.name, self.dialect, self.quote),
            quote_name(&relation.name, self.dialect, self.quote),
        );
        self.push(
            rank,
            Suggestion {
                label,
                insert,
                kind: relation_kind(relation),
                detail: None,
            },
        );
    }

    fn add_scope(&mut self, analysis: &Analysis, catalog: Option<&Catalog>) {
        for reference in &analysis.scope {
            let cte = cte_of(reference, &analysis.ctes);
            if let Some(alias) = &reference.alias {
                self.add_name(alias, SuggestionKind::Alias, reference.name.as_deref());
            } else if let Some(name) = &reference.name {
                let kind = cte.map_or_else(
                    || reference_kind(reference, catalog),
                    |_| SuggestionKind::Cte,
                );
                self.add_name(name, kind, reference.schema.as_deref());
            }
            let detail = reference.alias.as_deref().or(reference.name.as_deref());
            if let Some(cte) = cte {
                self.add_cte_columns(cte, detail);
            } else if let Some(catalog) = catalog {
                self.add_columns_of(reference, catalog, detail);
            }
        }
    }

    /// Suggests the conditions of the foreign keys between the new relation and the
    /// relations before it, in both directions.
    fn add_join_conditions(&mut self, join: &JoinOn, ctes: &[Cte], catalog: &Catalog) {
        if cte_of(&join.joined, ctes).is_some() {
            return;
        }
        for other in &join.others {
            if cte_of(other, ctes).is_none() {
                self.add_keys_between(&join.joined, other, catalog);
                self.add_keys_between(other, &join.joined, catalog);
            }
        }
    }

    /// Suggests the condition of each key that goes from `source` to `target`.
    fn add_keys_between(&mut self, source: &Reference, target: &Reference, catalog: &Catalog) {
        let found =
            |item: &Reference| catalog.relation(item.schema.as_deref(), item.name.as_deref()?);
        let (Some((_, relation)), Some((target_schema, target_relation))) =
            (found(source), found(target))
        else {
            return;
        };
        let keys = relation.foreign_keys.iter().filter(|key| {
            key.target.schema == target_schema.name && key.target.relation == target_relation.name
        });
        for key in keys {
            let pairs = key.columns.iter().zip(&key.target_columns);
            let parts: Vec<String> = pairs
                .map(|(column, target_column)| {
                    format!(
                        "{} = {}",
                        self.qualified_column(source, column),
                        self.qualified_column(target, target_column),
                    )
                })
                .collect();
            let condition = parts.join(" AND ");
            if let Some(rank) = self.rank(&condition, true) {
                let suggestion = Suggestion {
                    label: condition.clone(),
                    insert: condition,
                    kind: SuggestionKind::Join,
                    detail: None,
                };
                self.push(rank, suggestion);
            }
        }
    }

    /// Writes a column with the alias of its relation, or with the name of the relation.
    fn qualified_column(&self, reference: &Reference, column: &str) -> String {
        let owner = reference.alias.as_deref().or(reference.name.as_deref());
        format!(
            "{}.{}",
            quote_name(owner.unwrap_or(""), self.dialect, None),
            quote_name(column, self.dialect, None),
        )
    }

    fn add_cte_columns(&mut self, cte: &Cte, detail: Option<&str>) {
        for column in &cte.columns {
            self.add_name(column, SuggestionKind::Column, detail.or(Some(&cte.name)));
        }
    }

    fn add_columns_of(&mut self, reference: &Reference, catalog: &Catalog, detail: Option<&str>) {
        let Some(name) = &reference.name else { return };
        if let Some((schema, relation)) = catalog.relation(reference.schema.as_deref(), name) {
            self.add_columns(schema, relation, detail);
        }
    }

    fn add_columns(&mut self, schema: &Schema, relation: &Relation, detail: Option<&str>) {
        let detail = detail.or(Some(&relation.name));
        match &relation.columns {
            Columns::Loaded(columns) => {
                for column in columns {
                    self.add_name(column, SuggestionKind::Column, detail);
                }
            }
            Columns::NotLoaded => {
                let missing = RelationRef {
                    schema: schema.name.clone(),
                    relation: relation.name.clone(),
                };
                if !self.missing_columns.contains(&missing) {
                    self.missing_columns.push(missing);
                }
            }
        }
    }

    fn add_qualified(&mut self, analysis: &Analysis, catalog: Option<&Catalog>) {
        let qualifiers = &analysis.qualifiers;
        let last = &qualifiers[qualifiers.len() - 1];
        let outer = qualifiers
            .len()
            .checked_sub(2)
            .map(|index| qualifiers[index].as_str());
        let in_scope = analysis.scope.iter().find(|reference| {
            let matches = |name: &Option<String>| {
                name.as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(last))
            };
            matches(&reference.alias) || (reference.alias.is_none() && matches(&reference.name))
        });
        let cte = in_scope.and_then(|reference| cte_of(reference, &analysis.ctes));
        if let Some(cte) = cte.filter(|_| outer.is_none()) {
            self.add_cte_columns(cte, None);
            return;
        }
        let Some(catalog) = catalog else { return };

        if outer.is_none() {
            if let Some(reference) = in_scope {
                self.add_columns_of(reference, catalog, None);
                return;
            }
            if analysis.position == Position::Relation {
                self.add_schema_relations(catalog, last);
                return;
            }
        }
        if let Some((schema, relation)) = catalog.relation(outer, last) {
            self.add_columns(schema, relation, None);
            return;
        }
        self.add_schema_relations(catalog, last);
    }

    fn add_schema_relations(&mut self, catalog: &Catalog, name: &str) {
        let Some(schema) = catalog.schema(name) else {
            return;
        };
        for relation in &schema.relations {
            self.add_name(&relation.name, relation_kind(relation), Some(&schema.name));
        }
    }

    fn finish(self, replace: Range<usize>, typed: &str) -> Completion {
        let mut keys: Vec<_> = self
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                (
                    item.rank,
                    item.suggestion.kind,
                    item.suggestion.label.to_lowercase(),
                    item.suggestion.detail.as_deref(),
                    index,
                )
            })
            .collect();
        let mut remaining = keys.as_mut_slice();
        let mut selected = Vec::with_capacity(MAX_ITEMS);
        let mut batch_size = MAX_ITEMS;
        let mut previous: Option<&Suggestion> = None;
        while !remaining.is_empty() && selected.len() < MAX_ITEMS {
            let count = remaining.len().min(batch_size);
            if count < remaining.len() {
                remaining.select_nth_unstable(count);
            }
            let (batch, rest) = remaining.split_at_mut(count);
            batch.sort_unstable();
            for key in batch {
                let suggestion = &self.items[key.4].suggestion;
                let duplicate = previous.is_some_and(|last| {
                    last.kind == suggestion.kind && last.insert == suggestion.insert
                });
                previous = Some(suggestion);
                if !duplicate {
                    selected.push(suggestion.clone());
                    if selected.len() == MAX_ITEMS {
                        break;
                    }
                }
            }
            remaining = rest;
            batch_size = batch_size.saturating_mul(2);
        }
        let mut items = selected;
        if let [only] = items.as_slice()
            && only.insert == typed
        {
            items.clear();
        }
        Completion {
            items,
            replace,
            missing_columns: self.missing_columns,
        }
    }
}

/// Returns the name of `WITH` that a reference reads. A schema excludes it.
fn cte_of<'a>(reference: &Reference, ctes: &'a [Cte]) -> Option<&'a Cte> {
    let name = reference
        .name
        .as_deref()
        .filter(|_| reference.schema.is_none())?;
    ctes.iter().find(|cte| cte.name.eq_ignore_ascii_case(name))
}

fn relation_kind(relation: &Relation) -> SuggestionKind {
    match relation.kind {
        RelationKind::Table => SuggestionKind::Table,
        RelationKind::View => SuggestionKind::View,
    }
}

fn reference_kind(reference: &Reference, catalog: Option<&Catalog>) -> SuggestionKind {
    reference
        .name
        .as_deref()
        .and_then(|name| catalog?.relation(reference.schema.as_deref(), name))
        .map_or(SuggestionKind::Table, |(_, relation)| {
            relation_kind(relation)
        })
}

fn needs_quotes(name: &str, dialect: Dialect) -> bool {
    let mut chars = name.chars();
    let starts_well = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_');
    let continues_well = chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    let case_ok = dialect != Dialect::Postgres || !name.chars().any(|c| c.is_ascii_uppercase());
    !(starts_well && continues_well && case_ok) || is_reserved(name)
}

/// Writes a name with quotes when the text or the dialect needs them.
pub fn quote_name(name: &str, dialect: Dialect, typed_quote: Option<char>) -> String {
    let open = match (typed_quote, dialect) {
        (Some(open), _) => open,
        (None, _) if !needs_quotes(name, dialect) => return name.to_owned(),
        (None, Dialect::MySql) => '`',
        (None, _) => '"',
    };
    if open == '[' {
        return format!("[{name}]");
    }
    quote_with(name, open)
}
