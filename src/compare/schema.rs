//! The model of the tables of one schema, the readers of the rows of `compare::sql`, and
//! the differences between two models.

use std::collections::{BTreeMap, BTreeSet};

use crate::grid::Row;
use crate::sql::{Dialect, TokenKind, quote_identifier, tokenize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaModel {
    pub dialect: Dialect,
    pub schema: String,
    /// The tables in name order.
    pub tables: BTreeMap<String, Table>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
    /// The columns of the primary key, in key order.
    pub primary: Vec<String>,
    /// The name of the primary key. Only PostgreSQL gives one.
    pub primary_name: Option<String>,
    pub constraints: Vec<Constraint>,
    pub indexes: Vec<Index>,
    /// The `CREATE TABLE` text of SQLite.
    pub sql: Option<String>,
}

impl Table {
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|column| column.name == name)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub ty: String,
    pub nullable: bool,
    /// The default as SQL text.
    pub default: Option<String>,
    /// The value that the database makes: an identity, `serial`, or `AUTO_INCREMENT`.
    pub auto: Option<String>,
    /// The generated clause. SQLite gives only `VIRTUAL` or `STORED`.
    pub generated: Option<String>,
    /// The `ON UPDATE` clause of MySQL.
    pub on_update: Option<String>,
    /// The character set and the collation of MySQL. The DDL keeps them, and they do not
    /// count as a difference.
    pub charset: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConstraintKind {
    Unique,
    Foreign,
    Check,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Constraint {
    /// SQLite gives a number for a foreign key, and a generated name for a unique key.
    pub name: String,
    pub kind: ConstraintKind,
    pub columns: Vec<String>,
    /// The table that a foreign key reads.
    pub target: Option<Target>,
    pub target_columns: Vec<String>,
    /// The text after the name in the dialect, such as `UNIQUE (a)` or `CHECK (a > 0)`.
    pub definition: String,
}

/// The table that a foreign key reads. A table of the same schema has no schema.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Target {
    pub schema: Option<String>,
    pub table: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Index {
    pub name: String,
    pub unique: bool,
    /// Each key part: a column name or an expression.
    pub parts: Vec<String>,
    /// The text after the table in the dialect. PostgreSQL gives the text after `USING`,
    /// MySQL the kind and the parts, and SQLite the text from the first parenthesis.
    pub definition: String,
}

/// Builds the model of a schema from the rows of the queries of `compare_sql`.
pub fn build(dialect: Dialect, schema: &str, results: &[Vec<Row>]) -> SchemaModel {
    let mut model = SchemaModel {
        dialect,
        schema: schema.to_owned(),
        tables: BTreeMap::new(),
    };
    let rows = |index: usize| results.get(index).map_or(&[][..], Vec::as_slice);
    read_columns(&mut model, rows(0));
    read_constraints(&mut model, dialect, rows(1));
    let texts = (dialect == Dialect::Sqlite).then(|| sqlite_texts(rows(3)));
    read_indexes(&mut model, dialect, rows(2), texts.as_ref());
    if let Some(texts) = &texts {
        for table in model.tables.values_mut() {
            table.sql = texts.get(&("table", table.name.clone())).cloned();
        }
    }
    resolve_targets(&mut model);
    model
}

fn read_columns(model: &mut SchemaModel, rows: &[Row]) {
    let mut places: BTreeMap<String, Vec<(u32, String)>> = BTreeMap::new();
    for row in rows {
        let (Some(relation), Some(name)) = (row.text(0), row.text(1)) else {
            continue;
        };
        let column = Column {
            name: name.clone(),
            ty: row.text(2).unwrap_or_default(),
            nullable: row.text(3).is_none_or(|nullable| nullable == "yes"),
            default: row.text(4),
            auto: row.text(5),
            generated: row.text(6),
            on_update: row.text(8),
            charset: row.text(9),
        };
        if let Some(place) = row.text(7).and_then(|place| place.parse().ok()) {
            places
                .entry(relation.clone())
                .or_default()
                .push((place, name));
        }
        model
            .tables
            .entry(relation.clone())
            .or_insert_with(|| Table {
                name: relation,
                ..Table::default()
            })
            .columns
            .push(column);
    }
    for (relation, mut places) in places {
        places.sort();
        if let Some(table) = model.tables.get_mut(&relation) {
            table.primary = places.into_iter().map(|(_, name)| name).collect();
        }
    }
}

/// One group of rows of the constraint query, or of the index query.
struct Group<'a> {
    relation: String,
    name: String,
    rows: Vec<&'a Row>,
}

/// Groups the rows by table and name. The query sorts them, so a group is a run of rows.
fn groups(rows: &[Row]) -> Vec<Group<'_>> {
    let mut groups: Vec<Group> = Vec::new();
    for row in rows {
        let (Some(relation), Some(name)) = (row.text(0), row.text(1)) else {
            continue;
        };
        match groups.last_mut() {
            Some(last) if last.relation == relation && last.name == name => last.rows.push(row),
            _ => groups.push(Group {
                relation,
                name,
                rows: vec![row],
            }),
        }
    }
    groups
}

fn read_constraints(model: &mut SchemaModel, dialect: Dialect, rows: &[Row]) {
    for group in groups(rows) {
        let Some(table) = model.tables.get_mut(&group.relation) else {
            continue;
        };
        let first = group.rows[0];
        let definition = first.text(3).unwrap_or_default();
        let kind = match first.text(2).as_deref() {
            Some("p") => {
                table.primary_name = Some(group.name);
                continue;
            }
            Some("u") => ConstraintKind::Unique,
            Some("f") => ConstraintKind::Foreign,
            Some("c") => ConstraintKind::Check,
            _ => continue,
        };
        let columns: Vec<String> = group.rows.iter().filter_map(|row| row.text(5)).collect();
        let target_columns = group.rows.iter().filter_map(|row| row.text(8)).collect();
        let target = first.text(7).map(|target| Target {
            schema: first.text(6).filter(|schema| *schema != model.schema),
            table: target,
        });
        let mut constraint = Constraint {
            name: group.name,
            kind,
            columns,
            target,
            target_columns,
            definition,
        };
        if dialect != Dialect::Postgres {
            constraint.definition = constraint_text(&constraint, dialect);
        }
        table.constraints.push(constraint);
    }
}

/// Writes the text of a constraint of MySQL or SQLite. The query gives the actions of a
/// foreign key and the clause of a check.
fn constraint_text(constraint: &Constraint, dialect: Dialect) -> String {
    let list = |names: &[String]| quoted_list(names, dialect);
    match constraint.kind {
        ConstraintKind::Unique => format!("UNIQUE ({})", list(&constraint.columns)),
        ConstraintKind::Check => check_text(&constraint.definition),
        ConstraintKind::Foreign => {
            let target = constraint
                .target
                .as_ref()
                .map(|target| target_text(target, None, dialect))
                .unwrap_or_default();
            let columns = if constraint.target_columns.is_empty() {
                String::new()
            } else {
                format!(" ({})", list(&constraint.target_columns))
            };
            format!(
                "FOREIGN KEY ({}) REFERENCES {target}{columns} {}",
                list(&constraint.columns),
                constraint.definition
            )
        }
    }
}

/// Writes a check with its parentheses. MySQL gives the clause with them.
fn check_text(clause: &str) -> String {
    if clause.starts_with('(') && clause.ends_with(')') {
        format!("CHECK {clause}")
    } else {
        format!("CHECK ({clause})")
    }
}

/// Writes the table of a foreign key. A table of the same schema takes `schema`, which
/// is `None` in the text that the compare reads.
pub fn target_text(target: &Target, schema: Option<&str>, dialect: Dialect) -> String {
    match target.schema.as_deref().or(schema) {
        Some(schema) => format!(
            "{}.{}",
            quote_identifier(schema, dialect),
            quote_identifier(&target.table, dialect)
        ),
        None => quote_identifier(&target.table, dialect),
    }
}

pub fn quoted_list(names: &[String], dialect: Dialect) -> String {
    names
        .iter()
        .map(|name| quote_identifier(name, dialect))
        .collect::<Vec<_>>()
        .join(", ")
}

fn read_indexes(
    model: &mut SchemaModel,
    dialect: Dialect,
    rows: &[Row],
    texts: Option<&BTreeMap<(&'static str, String), String>>,
) {
    for group in groups(rows) {
        let Some(table) = model.tables.get_mut(&group.relation) else {
            continue;
        };
        let first = group.rows[0];
        let unique = first.text(2).as_deref() == Some("yes");
        let (parts, written): (Vec<String>, Vec<String>) = group
            .rows
            .iter()
            .map(|row| index_part(row, dialect))
            .unzip();
        let definition = match dialect {
            Dialect::Postgres => first
                .text(5)
                .map(|text| postgres_index_text(&text))
                .unwrap_or_default(),
            Dialect::MySql => {
                let kind = first.text(5).unwrap_or_default();
                let list = written.join(", ");
                match kind.as_str() {
                    "FULLTEXT" | "SPATIAL" => format!("{kind} ({list})"),
                    _ => format!("({list})"),
                }
            }
            Dialect::Sqlite => texts
                .and_then(|texts| texts.get(&("index", group.name.clone())))
                .map(|text| after_parenthesis(text, dialect).to_owned())
                .unwrap_or_default(),
            Dialect::Redis => String::new(),
        };
        table.indexes.push(Index {
            name: group.name,
            unique,
            parts,
            definition,
        });
    }
}

/// Returns one key part: the name or the expression that a compare between two dialects
/// reads, and the text of the part in the dialect. MySQL gives a column with its prefix
/// and its order, or an expression. SQLite gives `NULL` for an expression, and its text
/// holds the expression.
fn index_part(row: &Row, dialect: Dialect) -> (String, String) {
    match dialect {
        Dialect::MySql => {
            let (part, mut written) = match (row.text(4), row.text(8)) {
                (Some(column), _) => {
                    let written = quote_identifier(&column, dialect);
                    (column, written)
                }
                (None, Some(expression)) => (expression.clone(), format!("({expression})")),
                (None, None) => (String::new(), String::new()),
            };
            if let Some(length) = row.text(6) {
                written.push_str(&format!("({length})"));
            }
            if row.text(7).as_deref() == Some("yes") {
                written.push_str(" DESC");
            }
            (part, written)
        }
        Dialect::Postgres => {
            let part = row.text(4).unwrap_or_default();
            (unquoted(&part), part)
        }
        _ => {
            let part = row.text(4).unwrap_or_else(|| "<expression>".to_owned());
            (part.clone(), part)
        }
    }
}

/// Removes the quotes of a name that PostgreSQL writes, so that a part compares with the
/// name of another database.
fn unquoted(part: &str) -> String {
    match part
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        Some(inner) if !inner.replace("\"\"", "").contains('"') => inner.replace("\"\"", "\""),
        _ => part.to_owned(),
    }
}

/// Returns the text of `pg_get_indexdef` after its table: the method, the parts, and the
/// other clauses.
fn postgres_index_text(text: &str) -> String {
    let start = text.find(" ON ").map_or(0, |at| at + 4);
    match text[start..].find(" USING ") {
        Some(at) => text[start + at + 7..].to_owned(),
        None => text.to_owned(),
    }
}

/// Returns the text from the first parenthesis outside a name or a comment.
pub fn after_parenthesis(text: &str, dialect: Dialect) -> &str {
    tokenize(text, dialect)
        .iter()
        .find(|token| {
            token.kind == TokenKind::Punctuation && &text[token.start()..token.end()] == "("
        })
        .map_or("", |token| &text[token.start()..])
}

/// Returns the text of each table and index of SQLite, by kind and name.
fn sqlite_texts(rows: &[Row]) -> BTreeMap<(&'static str, String), String> {
    let mut texts = BTreeMap::new();
    for row in rows {
        let (Some(kind), Some(name), Some(sql)) = (row.text(0), row.text(1), row.text(3)) else {
            continue;
        };
        let kind = match kind.as_str() {
            "table" => "table",
            "index" => "index",
            _ => continue,
        };
        texts.insert((kind, name), sql);
    }
    texts
}

/// Gives a SQLite key without target columns the primary key of its target, and then
/// writes its text again.
fn resolve_targets(model: &mut SchemaModel) {
    let primaries: BTreeMap<String, Vec<String>> = model
        .tables
        .values()
        .map(|table| (table.name.clone(), table.primary.clone()))
        .collect();
    for table in model.tables.values_mut() {
        for constraint in &mut table.constraints {
            let Some(target) = &constraint.target else {
                continue;
            };
            if constraint.target_columns.is_empty()
                && target.schema.is_none()
                && let Some(primary) = primaries.get(&target.table)
            {
                constraint.target_columns = primary.clone();
            }
        }
    }
}

/// What a difference does to the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Create,
    Drop,
    Alter,
}

/// An object of a difference, with its data on one side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Object {
    Table(Table),
    Column(Column),
    PrimaryKey {
        name: Option<String>,
        columns: Vec<String>,
    },
    Constraint(Constraint),
    Index(Index),
}

impl Object {
    pub fn name(&self) -> &str {
        match self {
            Self::Table(table) => &table.name,
            Self::Column(column) => &column.name,
            Self::PrimaryKey { name, .. } => name.as_deref().unwrap_or(""),
            Self::Constraint(constraint) => &constraint.name,
            Self::Index(index) => &index.name,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Difference {
    pub change: Change,
    pub table: String,
    pub source: Option<Object>,
    pub target: Option<Object>,
}

impl Difference {
    fn new(change: Change, table: &str, source: Option<Object>, target: Option<Object>) -> Self {
        Self {
            change,
            table: table.to_owned(),
            source,
            target,
        }
    }

    /// Returns the object of the difference: the source object, or the target object of
    /// a removal.
    pub fn object(&self) -> &Object {
        self.source
            .as_ref()
            .or(self.target.as_ref())
            .expect("a difference has an object")
    }
}

/// Returns the differences that make the target equal to the source, in table order.
/// Between two dialects, the type and the default of a column do not count, the checks
/// stay out, and the keys and the indexes compare by their columns.
pub fn diff(source: &SchemaModel, target: &SchemaModel) -> Vec<Difference> {
    let rule = Rule {
        same_dialect: source.dialect == target.dialect,
        // A key of SQLite has no name of its own.
        by_definition: source.dialect != target.dialect || source.dialect == Dialect::Sqlite,
    };
    let names: BTreeSet<&String> = source.tables.keys().chain(target.tables.keys()).collect();
    let mut differences = Vec::new();
    for name in names {
        match (source.tables.get(name), target.tables.get(name)) {
            (Some(table), None) => differences.push(Difference::new(
                Change::Create,
                name,
                Some(Object::Table(table.clone())),
                None,
            )),
            (None, Some(table)) => differences.push(Difference::new(
                Change::Drop,
                name,
                None,
                Some(Object::Table(table.clone())),
            )),
            (Some(from), Some(to)) => diff_table(from, to, rule, &mut differences),
            (None, None) => {}
        }
    }
    differences
}

/// How a compare matches and compares the objects of two tables.
#[derive(Clone, Copy)]
struct Rule {
    same_dialect: bool,
    /// True when the keys compare by their columns, not by their names.
    by_definition: bool,
}

fn diff_table(from: &Table, to: &Table, rule: Rule, differences: &mut Vec<Difference>) {
    let same_dialect = rule.same_dialect;
    let table = &from.name;
    for column in &from.columns {
        match to.column(&column.name) {
            None => differences.push(Difference::new(
                Change::Create,
                table,
                Some(Object::Column(column.clone())),
                None,
            )),
            Some(other) if column_differs(column, other, same_dialect) => {
                differences.push(Difference::new(
                    Change::Alter,
                    table,
                    Some(Object::Column(column.clone())),
                    Some(Object::Column(other.clone())),
                ));
            }
            Some(_) => {}
        }
    }
    for column in &to.columns {
        if from.column(&column.name).is_none() {
            differences.push(Difference::new(
                Change::Drop,
                table,
                None,
                Some(Object::Column(column.clone())),
            ));
        }
    }
    if from.primary != to.primary {
        let key = |table: &Table| {
            (!table.primary.is_empty()).then(|| Object::PrimaryKey {
                name: table.primary_name.clone(),
                columns: table.primary.clone(),
            })
        };
        let change = match (from.primary.is_empty(), to.primary.is_empty()) {
            (false, true) => Change::Create,
            (true, false) => Change::Drop,
            _ => Change::Alter,
        };
        differences.push(Difference::new(change, table, key(from), key(to)));
    }
    let sources = keyed(from, rule);
    let targets = keyed(to, rule);
    for (identity, object) in &sources {
        match targets.iter().find(|(other, _)| other == identity) {
            None => differences.push(Difference::new(
                Change::Create,
                table,
                Some(object.clone()),
                None,
            )),
            Some((_, other)) if same_dialect && object_text(object) != object_text(other) => {
                differences.push(Difference::new(
                    Change::Alter,
                    table,
                    Some(object.clone()),
                    Some(other.clone()),
                ));
            }
            Some(_) => {}
        }
    }
    for (identity, object) in &targets {
        if !sources.iter().any(|(other, _)| other == identity) {
            differences.push(Difference::new(
                Change::Drop,
                table,
                None,
                Some(object.clone()),
            ));
        }
    }
}

/// Returns true when two columns of the same name differ. Between two dialects only the
/// `NULL` rule counts.
fn column_differs(from: &Column, to: &Column, same_dialect: bool) -> bool {
    if from.nullable != to.nullable {
        return true;
    }
    same_dialect
        && (type_text(&from.ty) != type_text(&to.ty)
            || from.default != to.default
            || from.auto != to.auto
            || from.generated != to.generated
            || from.on_update != to.on_update)
}

/// Writes a type in lowercase with single spaces.
fn type_text(ty: &str) -> String {
    ty.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The identity of a key or an index in a compare.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Identity {
    Name(ConstraintKind, String),
    IndexName(String),
    /// A unique key or an index, by its columns.
    Columns(bool, Vec<String>),
    /// A foreign key, by its columns and its target.
    Foreign(Vec<String>, Option<Target>, Vec<String>),
}

/// Returns the keys and the indexes of a table with their identities. Between two
/// dialects, a unique key compares with a unique index.
fn keyed(table: &Table, rule: Rule) -> Vec<(Identity, Object)> {
    let Rule {
        same_dialect,
        by_definition,
    } = rule;
    let mut keyed = Vec::new();
    for constraint in &table.constraints {
        let identity = match constraint.kind {
            ConstraintKind::Check if !same_dialect => continue,
            ConstraintKind::Foreign if by_definition => Identity::Foreign(
                constraint.columns.clone(),
                constraint.target.clone(),
                constraint.target_columns.clone(),
            ),
            ConstraintKind::Unique if by_definition => {
                Identity::Columns(true, constraint.columns.clone())
            }
            kind => Identity::Name(kind, constraint.name.clone()),
        };
        keyed.push((identity, Object::Constraint(constraint.clone())));
    }
    for index in &table.indexes {
        let identity = if same_dialect {
            Identity::IndexName(index.name.clone())
        } else {
            Identity::Columns(index.unique, index.parts.clone())
        };
        keyed.push((identity, Object::Index(index.clone())));
    }
    keyed
}

/// Returns the text that decides a change of a key or an index of the same identity.
fn object_text(object: &Object) -> String {
    match object {
        Object::Constraint(constraint) => constraint.definition.clone(),
        Object::Index(index) => format!("{} {}", index.unique, index.definition),
        _ => String::new(),
    }
}

/// Returns the short text of an object for the grid.
pub fn summary(object: &Object) -> String {
    match object {
        Object::Table(table) => crate::ui::text::compare_columns(table.columns.len()),
        Object::Column(column) => {
            let mut text = column.ty.clone();
            if let Some(auto) = &column.auto {
                text.push(' ');
                text.push_str(auto);
            }
            if let Some(generated) = &column.generated {
                text.push(' ');
                text.push_str(generated);
            }
            if !column.nullable {
                text.push_str(" NOT NULL");
            }
            if let Some(default) = &column.default {
                text.push_str(" DEFAULT ");
                text.push_str(default);
            }
            if let Some(on_update) = &column.on_update {
                text.push(' ');
                text.push_str(on_update);
            }
            text
        }
        Object::PrimaryKey { columns, .. } => format!("({})", columns.join(", ")),
        Object::Constraint(constraint) => constraint.definition.clone(),
        Object::Index(index) => {
            let unique = if index.unique { "UNIQUE " } else { "" };
            if index.definition.is_empty() {
                format!("{unique}({})", index.parts.join(", "))
            } else {
                format!("{unique}{}", index.definition)
            }
        }
    }
}
