//! The DDL of one object: the queries that read it, and the text that SQLN gives the
//! user. A database function or statement gives the text where the database has one.
//! SQLN writes the text of a PostgreSQL table, sequence, and type from the catalog.

use crate::catalog::{Object, ObjectKind, RelationKind, RelationRef};
use crate::compare::{ddl, schema, sql::postgres_table_sql};
use crate::grid::Row;
use crate::sql::literal::text_literal;
use crate::sql::{Dialect, quote_identifier};
use crate::structure::name_literal;
use crate::ui::text;

/// What a DDL describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Table,
    View,
    Object(ObjectKind),
}

/// The object of a DDL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub schema: String,
    pub name: String,
    pub kind: Kind,
    /// The arguments of a routine on PostgreSQL, or the table of a trigger.
    pub detail: String,
}

impl Target {
    pub fn relation(relation: &RelationRef, kind: RelationKind) -> Self {
        Self {
            schema: relation.schema.clone(),
            name: relation.relation.clone(),
            kind: match kind {
                RelationKind::Table => Kind::Table,
                RelationKind::View => Kind::View,
            },
            detail: String::new(),
        }
    }

    pub fn object(schema: &str, object: &Object) -> Self {
        Self {
            schema: schema.to_owned(),
            name: object.name.clone(),
            kind: Kind::Object(object.kind),
            detail: object.detail.clone(),
        }
    }

    /// Returns the name of the header: the name, with the arguments of a routine.
    fn label(&self) -> String {
        match self.kind {
            Kind::Object(kind) => Object::new(self.name.clone(), kind, self.detail.clone()).label,
            _ => self.name.clone(),
        }
    }

    fn literals(&self, dialect: Dialect) -> (String, String, String) {
        (
            name_literal(&self.schema, dialect),
            name_literal(&self.name, dialect),
            name_literal(&self.detail, dialect),
        )
    }

    /// Returns the name with its schema, with the quotes of the dialect.
    fn qualified(&self, dialect: Dialect) -> String {
        format!(
            "{}.{}",
            quote_identifier(&self.schema, dialect),
            quote_identifier(&self.name, dialect)
        )
    }
}

/// Returns true when SQLN writes the text of the object from the catalog.
fn is_written(dialect: Dialect, kind: Kind) -> bool {
    dialect == Dialect::Postgres
        && matches!(
            kind,
            Kind::Table
                | Kind::Object(ObjectKind::Sequence | ObjectKind::Enum | ObjectKind::Domain)
        )
}

/// Returns the queries that read the DDL of an object. A source or a kind without DDL
/// gives `None`.
pub fn definition_sql(dialect: Dialect, target: &Target) -> Option<Vec<String>> {
    let (schema, name, detail) = target.literals(dialect);
    let one = |sql: String| Some(vec![sql]);
    match (dialect, target.kind) {
        (Dialect::Postgres, Kind::Table) => Some(postgres_table_sql(&target.schema, &target.name)),
        (Dialect::Postgres, Kind::View) => one(format!(
            "SELECT CASE c.relkind WHEN 'm' THEN 'CREATE MATERIALIZED VIEW ' \
               ELSE 'CREATE OR REPLACE VIEW ' END \
             || pg_catalog.quote_ident(n.nspname) || '.' || pg_catalog.quote_ident(c.relname) \
             || ' AS' || pg_catalog.chr(10) || pg_catalog.pg_get_viewdef(c.oid, true) \
             FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = {schema} AND c.relname = {name} AND c.relkind IN ('v', 'm')"
        )),
        (Dialect::Postgres, Kind::Object(ObjectKind::Function | ObjectKind::Procedure)) => {
            one(format!(
                "SELECT pg_catalog.pg_get_functiondef(p.oid) \
                 FROM pg_catalog.pg_proc p \
                 JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
                 WHERE n.nspname = {schema} AND p.proname = {name} \
                 AND pg_catalog.pg_get_function_identity_arguments(p.oid) = {detail}"
            ))
        }
        (Dialect::Postgres, Kind::Object(ObjectKind::Trigger)) => one(format!(
            "SELECT pg_catalog.pg_get_triggerdef(t.oid, true) \
             FROM pg_catalog.pg_trigger t \
             JOIN pg_catalog.pg_class c ON c.oid = t.tgrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = {schema} AND c.relname = {detail} AND t.tgname = {name}"
        )),
        (Dialect::Postgres, Kind::Object(ObjectKind::Sequence)) => one(format!(
            "SELECT pg_catalog.format_type(q.seqtypid, NULL), q.seqincrement::text, \
             q.seqmin::text, q.seqmax::text, q.seqstart::text, q.seqcache::text, \
             CASE WHEN q.seqcycle THEN 'yes' ELSE 'no' END, \
             (SELECT pg_catalog.quote_ident(tn.nspname) || '.' \
               || pg_catalog.quote_ident(tc.relname) || '.' || pg_catalog.quote_ident(a.attname) \
              FROM pg_catalog.pg_depend d \
              JOIN pg_catalog.pg_class tc ON tc.oid = d.refobjid \
              JOIN pg_catalog.pg_namespace tn ON tn.oid = tc.relnamespace \
              JOIN pg_catalog.pg_attribute a \
                ON a.attrelid = d.refobjid AND a.attnum = d.refobjsubid \
              WHERE d.classid = 'pg_catalog.pg_class'::pg_catalog.regclass \
                AND d.objid = c.oid AND d.deptype = 'a' LIMIT 1) \
             FROM pg_catalog.pg_sequence q \
             JOIN pg_catalog.pg_class c ON c.oid = q.seqrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = {schema} AND c.relname = {name}"
        )),
        (Dialect::Postgres, Kind::Object(ObjectKind::Enum)) => one(format!(
            "SELECT e.enumlabel::text \
             FROM pg_catalog.pg_enum e \
             JOIN pg_catalog.pg_type t ON t.oid = e.enumtypid \
             JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace \
             WHERE n.nspname = {schema} AND t.typname = {name} \
             ORDER BY e.enumsortorder"
        )),
        // A domain gives one row per check, or one row without a check.
        (Dialect::Postgres, Kind::Object(ObjectKind::Domain)) => one(format!(
            "SELECT pg_catalog.format_type(t.typbasetype, t.typtypmod), t.typdefault, \
             CASE WHEN t.typnotnull THEN 'yes' ELSE 'no' END, k.conname::text, \
             pg_catalog.pg_get_constraintdef(k.oid) \
             FROM pg_catalog.pg_type t \
             JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace \
             LEFT JOIN pg_catalog.pg_constraint k ON k.contypid = t.oid AND k.contype = 'c' \
             WHERE n.nspname = {schema} AND t.typname = {name} AND t.typtype = 'd' \
             ORDER BY k.conname"
        )),
        (Dialect::MySql, kind) => {
            let statement = match kind {
                Kind::Table => "TABLE",
                Kind::View => "VIEW",
                Kind::Object(ObjectKind::Function) => "FUNCTION",
                Kind::Object(ObjectKind::Procedure) => "PROCEDURE",
                Kind::Object(ObjectKind::Trigger) => "TRIGGER",
                Kind::Object(_) => return None,
            };
            one(format!(
                "SHOW CREATE {statement} {}",
                target.qualified(dialect)
            ))
        }
        (Dialect::Sqlite, kind) => {
            let schema = quote_identifier(&target.schema, dialect);
            let filter = match kind {
                Kind::Table => {
                    format!("tbl_name = {name} AND type IN ('table', 'index')")
                }
                Kind::View => format!("name = {name} AND type = 'view'"),
                Kind::Object(ObjectKind::Trigger) => format!("name = {name} AND type = 'trigger'"),
                Kind::Object(_) => return None,
            };
            // The table comes first, then its indexes. An automatic index has no text.
            one(format!(
                "SELECT sql FROM {schema}.sqlite_schema \
                 WHERE {filter} AND sql IS NOT NULL \
                 ORDER BY type <> 'table', name"
            ))
        }
        _ => None,
    }
}

/// Writes the DDL from the rows of the queries of `definition_sql`. `connection` names the
/// connection in the header.
pub fn definition_text(
    dialect: Dialect,
    target: &Target,
    connection: &str,
    results: &[Vec<Row>],
) -> Result<String, &'static str> {
    let rows = |index: usize| results.get(index).map_or(&[][..], Vec::as_slice);
    let first = rows(0).first();
    let body = match (dialect, target.kind) {
        (Dialect::Postgres, Kind::Table) => {
            let model = schema::build(dialect, &target.schema, results);
            ddl::create_text(&model, &target.name).ok_or(text::DEFINITION_MISSING)?
        }
        (Dialect::Postgres, Kind::Object(ObjectKind::Sequence)) => {
            sequence_text(target, first.ok_or(text::DEFINITION_MISSING)?)
        }
        (Dialect::Postgres, Kind::Object(ObjectKind::Enum)) => {
            if rows(0).is_empty() {
                return Err(text::DEFINITION_MISSING);
            }
            enum_text(target, rows(0))
        }
        (Dialect::Postgres, Kind::Object(ObjectKind::Domain)) => {
            if rows(0).is_empty() {
                return Err(text::DEFINITION_MISSING);
            }
            domain_text(target, rows(0))
        }
        (Dialect::Postgres, _) => {
            let row = first.ok_or(text::DEFINITION_MISSING)?;
            statement(&row.text(0).ok_or(text::DEFINITION_MISSING)?)
        }
        (Dialect::MySql, kind) => {
            let row = first.ok_or(text::DEFINITION_MISSING)?;
            // The text has the second place in `SHOW CREATE TABLE` and `SHOW CREATE VIEW`,
            // and the third in the others: MySQL and MariaDB give the same columns.
            let column = if matches!(kind, Kind::Table | Kind::View) {
                1
            } else {
                2
            };
            statement(&row.text(column).ok_or(text::DEFINITION_PRIVILEGE)?)
        }
        (Dialect::Sqlite, _) => {
            let texts: Vec<String> = rows(0)
                .iter()
                .filter_map(|row| row.text(0))
                .map(|text| statement(&text))
                .collect();
            if texts.is_empty() {
                return Err(text::DEFINITION_MISSING);
            }
            texts.join("\n\n")
        }
        (Dialect::Redis, _) => return Err(text::DEFINITION_MISSING),
    };
    let mut header = format!(
        "-- {} {}.{} · {connection}\n",
        kind_name(target.kind),
        target.schema,
        target.label()
    );
    if is_written(dialect, target.kind) {
        header.push_str(text::DEFINITION_WRITTEN);
        header.push('\n');
    }
    Ok(format!("{header}{body}\n"))
}

/// Ends a statement of the database with one semicolon.
fn statement(text: &str) -> String {
    let text = text.trim_end();
    let text = text.strip_suffix(';').unwrap_or(text).trim_end();
    format!("{text};")
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Table => "Table",
        Kind::View => "View",
        Kind::Object(ObjectKind::Function) => "Function",
        Kind::Object(ObjectKind::Procedure) => "Procedure",
        Kind::Object(ObjectKind::Sequence) => "Sequence",
        Kind::Object(ObjectKind::Trigger) => "Trigger",
        Kind::Object(ObjectKind::Enum) => "Enum type",
        Kind::Object(ObjectKind::Domain) => "Domain",
    }
}

/// The row has the type, the increment, the minimum, the maximum, the start, the cache,
/// the cycle (`yes` or `no`), and the column that owns the sequence.
fn sequence_text(target: &Target, row: &Row) -> String {
    let cell = |column| row.text(column).unwrap_or_default();
    let name = target.qualified(Dialect::Postgres);
    let cycle = if cell(6) == "yes" {
        "CYCLE"
    } else {
        "NO CYCLE"
    };
    let mut text = format!(
        "CREATE SEQUENCE {name}\n    AS {}\n    INCREMENT BY {}\n    MINVALUE {}\n    \
         MAXVALUE {}\n    START WITH {}\n    CACHE {}\n    {cycle};",
        cell(0),
        cell(1),
        cell(2),
        cell(3),
        cell(4),
        cell(5)
    );
    if let Some(owner) = row.text(7) {
        text.push_str(&format!("\nALTER SEQUENCE {name} OWNED BY {owner};"));
    }
    text
}

fn enum_text(target: &Target, rows: &[Row]) -> String {
    let labels: Vec<String> = rows
        .iter()
        .map(|row| text_literal(&row.text(0).unwrap_or_default(), Dialect::Postgres))
        .collect();
    format!(
        "CREATE TYPE {} AS ENUM (\n    {}\n);",
        target.qualified(Dialect::Postgres),
        labels.join(",\n    ")
    )
}

/// Each row has the base type, the default, `NOT NULL` (`yes` or `no`), and the name and
/// the text of one check.
fn domain_text(target: &Target, rows: &[Row]) -> String {
    let first = &rows[0];
    let mut text = format!(
        "CREATE DOMAIN {} AS {}",
        target.qualified(Dialect::Postgres),
        first.text(0).unwrap_or_default()
    );
    if let Some(default) = first.text(1) {
        text.push_str(&format!("\n    DEFAULT {default}"));
    }
    if first.text(2).as_deref() == Some("yes") {
        text.push_str("\n    NOT NULL");
    }
    for row in rows {
        if let (Some(name), Some(check)) = (row.text(3), row.text(4)) {
            let name = quote_identifier(&name, Dialect::Postgres);
            text.push_str(&format!("\n    CONSTRAINT {name} {check}"));
        }
    }
    text.push(';');
    text
}
