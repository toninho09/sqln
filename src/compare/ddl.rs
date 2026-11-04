//! The script that makes the target schema equal to the source schema. Both models
//! have the same dialect. A removal stays a comment unless the user asks for it.

use std::collections::BTreeSet;

use super::schema::{
    Change, Column, Constraint, ConstraintKind, Difference, Index, Object, SchemaModel, Table,
    after_parenthesis, quoted_list, target_text,
};
use crate::sql::{Dialect, quote_identifier};

/// The prefix of the new table of a SQLite rebuild.
const REBUILD_PREFIX: &str = "sqln_new_";

/// Returns the statements of the script, one per line or block. Without differences, the
/// script is empty.
pub fn script(
    source: &SchemaModel,
    target: &SchemaModel,
    differences: &[Difference],
    removals: bool,
) -> String {
    if differences.is_empty() {
        return String::new();
    }
    let mut writer = Writer {
        dialect: target.dialect,
        schema: target.schema.clone(),
        removals,
        lines: Vec::new(),
    };
    match target.dialect {
        Dialect::Sqlite => sqlite_script(&mut writer, source, target, differences),
        _ => server_script(&mut writer, differences),
    }
    writer.lines.join("\n")
}

struct Writer {
    dialect: Dialect,
    schema: String,
    removals: bool,
    lines: Vec<String>,
}

impl Writer {
    fn name(&self, name: &str) -> String {
        quote_identifier(name, self.dialect)
    }

    /// Returns the table with the schema of the target.
    fn table(&self, table: &str) -> String {
        format!("{}.{}", self.name(&self.schema), self.name(table))
    }

    fn statement(&mut self, sql: String) {
        self.lines.push(format!("{sql};"));
    }

    /// Writes a removal: a statement when the user asked for removals, else a comment.
    fn removal(&mut self, sql: String) {
        if self.removals {
            self.statement(sql);
        } else {
            let text = format!("{sql};");
            self.lines
                .extend(text.lines().map(|line| format!("-- {line}")));
        }
    }

    fn note(&mut self, text: String) {
        self.lines.push(format!("-- SQLN: {text}"));
    }
}

/// The differences of one kind of object, with the data of each side.
fn of_kind<'a>(
    differences: &'a [Difference],
    pick: impl Fn(&Object) -> bool + 'a,
) -> impl Iterator<Item = &'a Difference> + 'a {
    differences
        .iter()
        .filter(move |difference| pick(difference.object()))
}

fn is_foreign(object: &Object) -> bool {
    matches!(object, Object::Constraint(constraint) if constraint.kind == ConstraintKind::Foreign)
}

fn is_key_or_index(object: &Object) -> bool {
    matches!(object, Object::Index(_))
        || matches!(object, Object::Constraint(constraint) if constraint.kind != ConstraintKind::Foreign)
}

/// The tables that the script creates, from their source data.
fn created(differences: &[Difference]) -> Vec<&Table> {
    differences
        .iter()
        .filter_map(
            |difference| match (&difference.change, &difference.source) {
                (Change::Create, Some(Object::Table(table))) => Some(table),
                _ => None,
            },
        )
        .collect()
}

/// PostgreSQL and MySQL: drops first, then the new tables, the columns, the keys, and the
/// foreign keys last, so a foreign key always finds its target.
fn server_script(writer: &mut Writer, differences: &[Difference]) {
    let postgres = writer.dialect == Dialect::Postgres;
    if postgres {
        writer.statement("BEGIN".to_owned());
        let schema = writer.name(&writer.schema);
        writer.statement(format!("SET LOCAL search_path TO {schema}"));
    }
    for difference in of_kind(differences, is_foreign) {
        drop_object(writer, difference);
    }
    for difference in of_kind(differences, is_key_or_index) {
        drop_object(writer, difference);
    }
    for difference in of_kind(differences, |object| {
        matches!(object, Object::PrimaryKey { .. })
    }) {
        drop_primary_key(writer, difference);
    }
    for difference in differences {
        if let (Change::Drop, Some(Object::Column(column))) =
            (difference.change, &difference.target)
        {
            let sql = format!(
                "ALTER TABLE {} DROP COLUMN {}",
                writer.table(&difference.table),
                writer.name(&column.name)
            );
            writer.removal(sql);
        }
    }
    for difference in differences {
        if let (Change::Drop, Some(Object::Table(table))) = (difference.change, &difference.target)
        {
            let sql = format!("DROP TABLE {}", writer.table(&table.name));
            writer.removal(sql);
        }
    }
    let created = created(differences);
    for table in &created {
        create_table(writer, table);
    }
    for difference in differences {
        match (difference.change, &difference.source, &difference.target) {
            (Change::Create, Some(Object::Column(column)), _) => {
                let sql = format!(
                    "ALTER TABLE {} ADD COLUMN {}",
                    writer.table(&difference.table),
                    column_text(column, writer.dialect)
                );
                writer.statement(sql);
            }
            (Change::Alter, Some(Object::Column(from)), Some(Object::Column(to))) => {
                alter_column(writer, &difference.table, from, to);
            }
            _ => {}
        }
    }
    for difference in of_kind(differences, |object| {
        matches!(object, Object::PrimaryKey { .. })
    }) {
        add_primary_key(writer, difference);
    }
    for difference in of_kind(differences, is_key_or_index) {
        if difference.change != Change::Drop {
            add_object(writer, &difference.table, difference.object());
        }
    }
    for table in &created {
        for index in &table.indexes {
            add_object(writer, &table.name, &Object::Index(index.clone()));
        }
    }
    for difference in of_kind(differences, is_foreign) {
        if difference.change != Change::Drop {
            add_object(writer, &difference.table, difference.object());
        }
    }
    for table in &created {
        for constraint in &table.constraints {
            if constraint.kind == ConstraintKind::Foreign {
                add_object(writer, &table.name, &Object::Constraint(constraint.clone()));
            }
        }
    }
    if postgres {
        writer.statement("COMMIT".to_owned());
    }
}

/// Removes a key or an index of the target. A change removes it as a step, and an object
/// only in the target is a removal.
fn drop_object(writer: &mut Writer, difference: &Difference) {
    let Some(object) = &difference.target else {
        return;
    };
    let table = writer.table(&difference.table);
    let sql = match object {
        Object::Index(index) => match writer.dialect {
            Dialect::MySql => format!("DROP INDEX {} ON {table}", writer.name(&index.name)),
            _ => format!("DROP INDEX {}", writer.table(&index.name)),
        },
        Object::Constraint(constraint) => {
            let name = writer.name(&constraint.name);
            match (writer.dialect, constraint.kind) {
                (Dialect::MySql, ConstraintKind::Foreign) => {
                    format!("ALTER TABLE {table} DROP FOREIGN KEY {name}")
                }
                (Dialect::MySql, ConstraintKind::Check) => {
                    format!("ALTER TABLE {table} DROP CHECK {name}")
                }
                _ => format!("ALTER TABLE {table} DROP CONSTRAINT {name}"),
            }
        }
        _ => return,
    };
    if difference.change == Change::Drop {
        writer.removal(sql);
    } else {
        writer.statement(sql);
    }
}

/// Removes the primary key of the target. MySQL changes a key in one statement, so only
/// a key without a source key leaves here.
fn drop_primary_key(writer: &mut Writer, difference: &Difference) {
    let table = writer.table(&difference.table);
    let sql = match (writer.dialect, &difference.target, difference.change) {
        (Dialect::MySql, Some(_), Change::Drop) => format!("ALTER TABLE {table} DROP PRIMARY KEY"),
        (
            Dialect::Postgres,
            Some(Object::PrimaryKey {
                name: Some(name), ..
            }),
            _,
        ) => {
            format!("ALTER TABLE {table} DROP CONSTRAINT {}", writer.name(name))
        }
        _ => return,
    };
    if difference.change == Change::Drop {
        writer.removal(sql);
    } else {
        writer.statement(sql);
    }
}

fn add_primary_key(writer: &mut Writer, difference: &Difference) {
    let Some(Object::PrimaryKey { name, columns }) = &difference.source else {
        return;
    };
    let table = writer.table(&difference.table);
    let list = quoted_list(columns, writer.dialect);
    let sql = match (writer.dialect, difference.change) {
        (Dialect::MySql, Change::Alter) => {
            format!("ALTER TABLE {table} DROP PRIMARY KEY, ADD PRIMARY KEY ({list})")
        }
        (Dialect::Postgres, _) if name.is_some() => format!(
            "ALTER TABLE {table} ADD CONSTRAINT {} PRIMARY KEY ({list})",
            writer.name(name.as_deref().unwrap_or_default())
        ),
        _ => format!("ALTER TABLE {table} ADD PRIMARY KEY ({list})"),
    };
    writer.statement(sql);
}

/// Adds a key or an index of the source.
fn add_object(writer: &mut Writer, table: &str, object: &Object) {
    let sql = match object {
        Object::Index(index) => index_text(writer, table, index),
        Object::Constraint(constraint) => format!(
            "ALTER TABLE {} ADD CONSTRAINT {} {}",
            writer.table(table),
            writer.name(&constraint.name),
            constraint_body(writer, constraint)
        ),
        _ => return,
    };
    writer.statement(sql);
}

/// Returns the text of a constraint after its name. A foreign key of MySQL names the
/// schema of the target, because the text of the read has no schema.
fn constraint_body(writer: &Writer, constraint: &Constraint) -> String {
    if writer.dialect != Dialect::MySql || constraint.kind != ConstraintKind::Foreign {
        return constraint.definition.clone();
    }
    let Some(target) = &constraint.target else {
        return constraint.definition.clone();
    };
    let actions = constraint
        .definition
        .rfind(" ON UPDATE ")
        .map_or("", |at| &constraint.definition[at..]);
    format!(
        "FOREIGN KEY ({}) REFERENCES {} ({}){actions}",
        quoted_list(&constraint.columns, writer.dialect),
        target_text(target, Some(&writer.schema), writer.dialect),
        quoted_list(&constraint.target_columns, writer.dialect)
    )
}

fn index_text(writer: &Writer, table: &str, index: &Index) -> String {
    let unique = if index.unique { "UNIQUE " } else { "" };
    let name = writer.name(&index.name);
    let table = writer.table(table);
    match writer.dialect {
        Dialect::Postgres => format!(
            "CREATE {unique}INDEX {name} ON {table} USING {}",
            index.definition
        ),
        Dialect::MySql => {
            let (kind, list) = match index.definition.split_once(' ') {
                Some((kind, list)) if !index.definition.starts_with('(') => {
                    (format!("{kind} "), list)
                }
                _ => (String::new(), index.definition.as_str()),
            };
            format!("CREATE {unique}{kind}INDEX {name} ON {table} {list}")
        }
        _ => String::new(),
    }
}

/// Creates a table of the source with its columns, its primary key, and its unique keys
/// and checks. The indexes and the foreign keys come after all tables.
fn create_table(writer: &mut Writer, table: &Table) {
    let mut parts: Vec<String> = table
        .columns
        .iter()
        .map(|column| column_text(column, writer.dialect))
        .collect();
    if !table.primary.is_empty() {
        let list = quoted_list(&table.primary, writer.dialect);
        parts.push(match &table.primary_name {
            Some(name) if writer.dialect == Dialect::Postgres => {
                format!("CONSTRAINT {} PRIMARY KEY ({list})", writer.name(name))
            }
            _ => format!("PRIMARY KEY ({list})"),
        });
    }
    for constraint in &table.constraints {
        if constraint.kind != ConstraintKind::Foreign {
            parts.push(format!(
                "CONSTRAINT {} {}",
                writer.name(&constraint.name),
                constraint.definition
            ));
        }
    }
    let body = parts
        .iter()
        .map(|part| format!("    {part}"))
        .collect::<Vec<_>>()
        .join(",\n");
    let sql = format!("CREATE TABLE {} (\n{body}\n)", writer.table(&table.name));
    writer.statement(sql);
}

/// Writes the definition of a column in the dialect.
pub fn column_text(column: &Column, dialect: Dialect) -> String {
    let name = quote_identifier(&column.name, dialect);
    match dialect {
        Dialect::Postgres => {
            let serial = column.auto.as_deref() == Some("serial");
            if let Some(ty) = serial.then(|| serial_type(&column.ty)).flatten() {
                return format!("{name} {ty} NOT NULL");
            }
            let mut text = format!("{name} {}", column.ty);
            if let Some(clause) = column.generated.as_ref().or(column.auto.as_ref()) {
                text.push_str(&format!(" {clause}"));
            }
            if let Some(default) = &column.default {
                text.push_str(&format!(" DEFAULT {default}"));
            }
            if !column.nullable {
                text.push_str(" NOT NULL");
            }
            text
        }
        Dialect::MySql => {
            let mut text = format!("{name} {}", column.ty);
            for clause in [&column.charset, &column.generated].into_iter().flatten() {
                text.push_str(&format!(" {clause}"));
            }
            text.push_str(if column.nullable {
                " NULL"
            } else {
                " NOT NULL"
            });
            if let Some(default) = &column.default {
                text.push_str(&format!(" DEFAULT {default}"));
            }
            for clause in [&column.auto, &column.on_update].into_iter().flatten() {
                text.push_str(&format!(" {clause}"));
            }
            text
        }
        _ => {
            let mut text = format!("{name} {}", column.ty);
            if let Some(default) = &column.default {
                text.push_str(&format!(" DEFAULT {default}"));
            }
            if !column.nullable {
                text.push_str(" NOT NULL");
            }
            text.trim_end().to_owned()
        }
    }
}

/// Returns the `serial` form of an integer type.
fn serial_type(ty: &str) -> Option<&'static str> {
    match ty {
        "integer" => Some("serial"),
        "bigint" => Some("bigserial"),
        "smallint" => Some("smallserial"),
        _ => None,
    }
}

/// Changes a column of the target to the column of the source.
fn alter_column(writer: &mut Writer, table: &str, from: &Column, to: &Column) {
    let qualified = writer.table(table);
    if writer.dialect == Dialect::MySql {
        let sql = format!(
            "ALTER TABLE {qualified} MODIFY COLUMN {}",
            column_text(from, writer.dialect)
        );
        writer.statement(sql);
        return;
    }
    let name = writer.name(&from.name);
    let column = format!("ALTER TABLE {qualified} ALTER COLUMN {name}");
    if from.auto != to.auto || from.generated != to.generated {
        writer.note(format!(
            "change the generated value of {qualified}.{name} by hand: {}",
            from.generated
                .as_ref()
                .or(from.auto.as_ref())
                .map_or("none", String::as_str)
        ));
    }
    let retyped = from.ty != to.ty;
    // An old default can refuse the new type, so it leaves first.
    if retyped && to.default.is_some() {
        writer.statement(format!("{column} DROP DEFAULT"));
    }
    if retyped {
        writer.statement(format!(
            "{column} TYPE {} USING {name}::{}",
            from.ty, from.ty
        ));
    }
    if from.default != to.default || (retyped && to.default.is_some()) {
        match &from.default {
            Some(default) => writer.statement(format!("{column} SET DEFAULT {default}")),
            None if !retyped => writer.statement(format!("{column} DROP DEFAULT")),
            None => {}
        }
    }
    if from.nullable != to.nullable {
        let rule = if from.nullable { "DROP" } else { "SET" };
        writer.statement(format!("{column} {rule} NOT NULL"));
    }
}

/// SQLite: `ALTER TABLE` adds a simple column, and a rebuild makes each other change. The
/// rebuild needs the foreign keys off, and SQLite ignores that pragma in a transaction.
fn sqlite_script(
    writer: &mut Writer,
    source: &SchemaModel,
    target: &SchemaModel,
    differences: &[Difference],
) {
    let rebuilt = rebuilt_tables(differences, writer.removals);
    if !rebuilt.is_empty() {
        writer.statement("PRAGMA foreign_keys = OFF".to_owned());
    }
    writer.statement("BEGIN".to_owned());
    let kept = || {
        differences
            .iter()
            .filter(|difference| !rebuilt.contains(difference.table.as_str()))
    };
    // An index of a column leaves before its column.
    for difference in kept() {
        if let (Change::Drop | Change::Alter, Some(Object::Index(index))) =
            (difference.change, difference.target.as_ref())
        {
            let sql = format!("DROP INDEX {}", writer.table(&index.name));
            if difference.change == Change::Drop {
                writer.removal(sql);
            } else {
                writer.statement(sql);
            }
        }
    }
    for difference in kept() {
        match (difference.change, difference.target.as_ref()) {
            (Change::Drop, Some(Object::Column(column))) => {
                let sql = format!(
                    "ALTER TABLE {} DROP COLUMN {}",
                    writer.table(&difference.table),
                    writer.name(&column.name)
                );
                writer.removal(sql);
            }
            (Change::Drop, Some(Object::Constraint(constraint))) => writer.note(format!(
                "the target keeps {} of {}. Turn on removals to rebuild the table without it",
                constraint.definition,
                writer.table(&difference.table)
            )),
            (Change::Drop, Some(Object::PrimaryKey { .. })) => writer.note(format!(
                "the target keeps the primary key of {}. Turn on removals to rebuild the table without it",
                writer.table(&difference.table)
            )),
            (Change::Drop, Some(Object::Table(table))) => {
                let sql = format!("DROP TABLE {}", writer.table(&table.name));
                writer.removal(sql);
            }
            _ => {}
        }
    }
    let created = created(differences);
    for table in &created {
        let body = table
            .sql
            .as_deref()
            .map(|sql| after_parenthesis(sql, Dialect::Sqlite))
            .unwrap_or_default();
        let sql = format!("CREATE TABLE {} {body}", writer.table(&table.name));
        writer.statement(sql);
    }
    for difference in differences {
        if let (Change::Create, Some(Object::Column(column))) =
            (difference.change, &difference.source)
            && !rebuilt.contains(difference.table.as_str())
        {
            let sql = format!(
                "ALTER TABLE {} ADD COLUMN {}",
                writer.table(&difference.table),
                column_text(column, Dialect::Sqlite)
            );
            writer.statement(sql);
        }
    }
    for name in &rebuilt {
        if let (Some(from), Some(to)) = (source.tables.get(*name), target.tables.get(*name)) {
            rebuild(writer, from, to);
        }
    }
    for difference in differences {
        if let (Change::Create | Change::Alter, Some(Object::Index(index))) =
            (difference.change, &difference.source)
            && !rebuilt.contains(difference.table.as_str())
        {
            let sql = sqlite_index(writer, &difference.table, index);
            writer.statement(sql);
        }
    }
    for table in &created {
        for index in &table.indexes {
            let sql = sqlite_index(writer, &table.name, index);
            writer.statement(sql);
        }
    }
    if !rebuilt.is_empty() {
        writer.statement("PRAGMA foreign_key_check".to_owned());
    }
    writer.statement("COMMIT".to_owned());
    if !rebuilt.is_empty() {
        writer.statement("PRAGMA foreign_keys = ON".to_owned());
    }
}

/// Returns the tables that need a rebuild: a changed column, a key, a column that
/// `ALTER TABLE` cannot add, and with removals, a key only in the target.
fn rebuilt_tables(differences: &[Difference], removals: bool) -> BTreeSet<&str> {
    differences
        .iter()
        .filter(
            |difference| match (difference.change, difference.object()) {
                (_, Object::Table(_) | Object::Index(_)) => false,
                (Change::Create, Object::Column(column)) => !simple_column(column),
                (Change::Drop, Object::Column(_)) => false,
                (Change::Drop, Object::Constraint(_) | Object::PrimaryKey { .. }) => removals,
                _ => true,
            },
        )
        .map(|difference| difference.table.as_str())
        .collect()
}

/// Returns true for a column that `ALTER TABLE ADD COLUMN` of SQLite accepts: not
/// generated, with a constant default, and with a default when it refuses `NULL`. A new
/// column of the primary key also changes the key, and that change rebuilds the table.
fn simple_column(column: &Column) -> bool {
    let constant = column.default.as_deref().is_none_or(|default| {
        !default.starts_with('(') && !default.to_uppercase().starts_with("CURRENT_")
    });
    let has_value = column.nullable
        || column
            .default
            .as_deref()
            .is_some_and(|default| !default.eq_ignore_ascii_case("NULL"));
    column.generated.is_none() && constant && has_value
}

/// Rebuilds a table of the target with the text of the source table: a new table, the
/// copy of the common columns, the removal of the old table, the new name, and the
/// indexes. Without removals, the columns and the indexes of the target stay.
fn rebuild(writer: &mut Writer, from: &Table, to: &Table) {
    let Some(sql) = &from.sql else {
        writer.note(format!(
            "no text of the source table {}, so it cannot rebuild it",
            from.name
        ));
        return;
    };
    let temporary = format!("{REBUILD_PREFIX}{}", from.name);
    let new = writer.table(&temporary);
    let old = writer.table(&to.name);
    writer.statement(format!(
        "CREATE TABLE {new} {}",
        after_parenthesis(sql, Dialect::Sqlite)
    ));
    let kept: Vec<&Column> = if writer.removals {
        Vec::new()
    } else {
        to.columns
            .iter()
            .filter(|column| from.column(&column.name).is_none())
            .collect()
    };
    for column in &kept {
        let column = Column {
            nullable: column.nullable || column.default.is_none(),
            ..(*column).clone()
        };
        let sql = format!(
            "ALTER TABLE {new} ADD COLUMN {}",
            column_text(&column, Dialect::Sqlite)
        );
        writer.statement(sql);
    }
    let copied: Vec<String> = from
        .columns
        .iter()
        .filter(|column| column.generated.is_none())
        .filter(|column| to.column(&column.name).is_some())
        .chain(kept.iter().copied())
        .map(|column| column.name.clone())
        .collect();
    if !copied.is_empty() {
        let list = quoted_list(&copied, Dialect::Sqlite);
        writer.statement(format!(
            "INSERT INTO {new} ({list}) SELECT {list} FROM {old}"
        ));
    }
    writer.statement(format!("DROP TABLE {old}"));
    let name = writer.name(&to.name);
    writer.statement(format!("ALTER TABLE {new} RENAME TO {name}"));
    for index in &from.indexes {
        let sql = sqlite_index(writer, &to.name, index);
        writer.statement(sql);
    }
    if !writer.removals {
        for index in &to.indexes {
            if !from.indexes.iter().any(|other| other.name == index.name) {
                let sql = sqlite_index(writer, &to.name, index);
                writer.statement(sql);
            }
        }
        let lost: Vec<&str> = to
            .constraints
            .iter()
            .filter(|constraint| !from.constraints.contains(constraint))
            .map(|constraint| constraint.definition.as_str())
            .collect();
        if !lost.is_empty() {
            writer.note(format!(
                "the rebuild of {old} uses the source text, so it leaves out: {}",
                lost.join("; ")
            ));
        }
    }
}

fn sqlite_index(writer: &Writer, table: &str, index: &Index) -> String {
    let unique = if index.unique { "UNIQUE " } else { "" };
    format!(
        "CREATE {unique}INDEX {} ON {} {}",
        writer.table(&index.name),
        writer.name(table),
        index.definition
    )
}
