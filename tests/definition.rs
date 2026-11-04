// The DDL of an object: the queries of each database and the text of the reader.

use std::collections::BTreeMap;

use sqln::catalog::{Object, ObjectKind, RelationKind, RelationRef};
use sqln::compare::ddl::create_text;
use sqln::compare::schema::{
    Column, Constraint, ConstraintKind, Index, SchemaModel, Table, Target as KeyTarget,
};
use sqln::definition::{Target, definition_sql, definition_text};
use sqln::grid::{Row, Value};
use sqln::sql::Dialect;
use sqln::ui::text;

fn object(name: &str, kind: ObjectKind, detail: &str) -> Target {
    Target::object("public", &Object::new(name.into(), kind, detail.into()))
}

fn table(name: &str) -> Target {
    let relation = RelationRef {
        schema: "public".into(),
        relation: name.into(),
    };
    Target::relation(&relation, RelationKind::Table)
}

fn row(cells: &[Option<&str>]) -> Row {
    cells
        .iter()
        .map(|cell| cell.map_or(Value::Null, Value::Text))
        .collect()
}

fn one(sql: Option<Vec<String>>) -> String {
    let sql = sql.expect("a query");
    assert_eq!(sql.len(), 1, "{sql:?}");
    sql.into_iter().next().unwrap()
}

#[test]
fn postgres_reads_each_object_with_the_function_of_the_database() {
    let sql = one(definition_sql(
        Dialect::Postgres,
        &object("total", ObjectKind::Function, "integer, text"),
    ));
    assert!(sql.contains("pg_get_functiondef(p.oid)"), "{sql}");
    assert!(sql.contains("p.proname = E'total'"), "{sql}");
    assert!(
        sql.contains("pg_get_function_identity_arguments(p.oid) = E'integer, text'"),
        "{sql}"
    );
    let sql = one(definition_sql(
        Dialect::Postgres,
        &object("audit", ObjectKind::Trigger, "orders"),
    ));
    assert!(sql.contains("pg_get_triggerdef(t.oid, true)"), "{sql}");
    assert!(
        sql.contains("c.relname = E'orders' AND t.tgname = E'audit'"),
        "{sql}"
    );
    let view = RelationRef {
        schema: "public".into(),
        relation: "v".into(),
    };
    let sql = one(definition_sql(
        Dialect::Postgres,
        &Target::relation(&view, RelationKind::View),
    ));
    assert!(sql.contains("pg_get_viewdef(c.oid, true)"), "{sql}");
    assert!(sql.contains("CREATE MATERIALIZED VIEW"), "{sql}");
    // A table reads the columns, the keys, and the indexes of the compare.
    let sql = definition_sql(Dialect::Postgres, &table("orders")).unwrap();
    assert_eq!(sql.len(), 3);
    assert!(
        sql.iter().all(|sql| sql.contains("c.relname = E'orders'")),
        "{sql:?}"
    );
}

#[test]
fn a_name_with_a_quote_stays_inside_its_literal() {
    let sql = one(definition_sql(
        Dialect::Postgres,
        &object("it's", ObjectKind::Sequence, ""),
    ));
    assert!(sql.contains("c.relname = E'it''s'"), "{sql}");
    let sql = one(definition_sql(
        Dialect::MySql,
        &object("a`b", ObjectKind::Procedure, ""),
    ));
    assert_eq!(sql, "SHOW CREATE PROCEDURE `public`.`a``b`");
}

#[test]
fn each_database_reads_only_the_objects_that_it_has() {
    assert_eq!(
        one(definition_sql(Dialect::MySql, &table("orders"))),
        "SHOW CREATE TABLE `public`.`orders`"
    );
    assert!(definition_sql(Dialect::MySql, &object("s", ObjectKind::Sequence, "")).is_none());
    assert!(definition_sql(Dialect::Sqlite, &object("f", ObjectKind::Function, "")).is_none());
    assert!(definition_sql(Dialect::Redis, &table("key")).is_none());
    let sql = one(definition_sql(
        Dialect::Sqlite,
        &object("audit", ObjectKind::Trigger, "orders"),
    ));
    assert!(sql.contains("\"public\".sqlite_schema"), "{sql}");
    assert!(sql.contains("name = 'audit' AND type = 'trigger'"), "{sql}");
}

#[test]
fn the_header_names_the_kind_the_object_and_the_connection() {
    let target = object("total", ObjectKind::Function, "integer");
    let rows = vec![vec![row(&[Some(
        "CREATE OR REPLACE FUNCTION public.total(integer)\n RETURNS integer\n AS $$ SELECT 1 $$\n",
    )])]];
    let ddl = definition_text(Dialect::Postgres, &target, "shop", &rows).unwrap();
    assert_eq!(
        ddl,
        "-- Function public.total(integer) · shop\n\
         CREATE OR REPLACE FUNCTION public.total(integer)\n RETURNS integer\n AS $$ SELECT 1 $$;\n"
    );
}

#[test]
fn a_statement_keeps_one_semicolon() {
    let view = RelationRef {
        schema: "public".into(),
        relation: "v".into(),
    };
    let target = Target::relation(&view, RelationKind::View);
    let rows = vec![vec![row(&[Some(
        "CREATE OR REPLACE VIEW public.v AS\n SELECT 1;  \n",
    )])]];
    let ddl = definition_text(Dialect::Postgres, &target, "shop", &rows).unwrap();
    assert!(ddl.ends_with("SELECT 1;\n"), "{ddl}");
    assert!(!ddl.contains(text::DEFINITION_WRITTEN), "{ddl}");
}

#[test]
fn mysql_takes_the_text_from_the_column_of_its_statement() {
    let rows = vec![vec![row(&[
        Some("orders"),
        Some("CREATE TABLE `orders` (`id` int)"),
    ])]];
    let ddl = definition_text(Dialect::MySql, &table("orders"), "shop", &rows).unwrap();
    assert!(
        ddl.ends_with("CREATE TABLE `orders` (`id` int);\n"),
        "{ddl}"
    );
    let target = object("close", ObjectKind::Procedure, "");
    let rows = vec![vec![row(&[
        Some("close"),
        Some("STRICT_TRANS_TABLES"),
        Some("CREATE PROCEDURE `close`() BEGIN SELECT 1; END"),
        Some("utf8mb4"),
    ])]];
    let ddl = definition_text(Dialect::MySql, &target, "shop", &rows).unwrap();
    assert!(ddl.ends_with("BEGIN SELECT 1; END;\n"), "{ddl}");
}

#[test]
fn mysql_without_the_privilege_gives_the_reason() {
    let target = object("close", ObjectKind::Procedure, "");
    let rows = vec![vec![row(&[Some("close"), Some(""), None])]];
    assert_eq!(
        definition_text(Dialect::MySql, &target, "shop", &rows),
        Err(text::DEFINITION_PRIVILEGE)
    );
}

#[test]
fn an_empty_read_says_that_the_object_is_missing() {
    for (dialect, target) in [
        (Dialect::Postgres, object("f", ObjectKind::Function, "")),
        (Dialect::Postgres, object("s", ObjectKind::Sequence, "")),
        (Dialect::Postgres, object("e", ObjectKind::Enum, "")),
        (Dialect::Postgres, table("t")),
        (Dialect::MySql, table("t")),
        (Dialect::Sqlite, table("t")),
    ] {
        assert_eq!(
            definition_text(
                dialect,
                &target,
                "shop",
                &[Vec::new(), Vec::new(), Vec::new()]
            ),
            Err(text::DEFINITION_MISSING),
            "{target:?}"
        );
    }
}

#[test]
fn sqlite_gives_the_table_and_then_its_indexes() {
    let rows = vec![vec![
        row(&[Some("CREATE TABLE orders (id INTEGER)")]),
        row(&[Some("CREATE INDEX orders_id ON orders (id)")]),
    ]];
    let ddl = definition_text(Dialect::Sqlite, &table("orders"), "local", &rows).unwrap();
    assert_eq!(
        ddl,
        "-- Table public.orders · local\n\
         CREATE TABLE orders (id INTEGER);\n\nCREATE INDEX orders_id ON orders (id);\n"
    );
}

#[test]
fn postgres_writes_a_sequence_with_its_owner() {
    let rows = vec![vec![row(&[
        Some("bigint"),
        Some("1"),
        Some("1"),
        Some("9223372036854775807"),
        Some("1"),
        Some("1"),
        Some("no"),
        Some("public.orders.id"),
    ])]];
    let target = object("orders_id_seq", ObjectKind::Sequence, "");
    let ddl = definition_text(Dialect::Postgres, &target, "shop", &rows).unwrap();
    assert_eq!(
        ddl,
        format!(
            "-- Sequence public.orders_id_seq · shop\n{}\n\
             CREATE SEQUENCE \"public\".\"orders_id_seq\"\n    AS bigint\n    INCREMENT BY 1\n    \
             MINVALUE 1\n    MAXVALUE 9223372036854775807\n    START WITH 1\n    CACHE 1\n    \
             NO CYCLE;\n\
             ALTER SEQUENCE \"public\".\"orders_id_seq\" OWNED BY public.orders.id;\n",
            text::DEFINITION_WRITTEN
        )
    );
}

#[test]
fn postgres_writes_an_enum_and_a_domain() {
    let rows = vec![vec![row(&[Some("new")]), row(&[Some("it's done")])]];
    let target = object("status", ObjectKind::Enum, "");
    let ddl = definition_text(Dialect::Postgres, &target, "shop", &rows).unwrap();
    assert!(
        ddl.ends_with(
            "CREATE TYPE \"public\".\"status\" AS ENUM (\n    E'new',\n    E'it''s done'\n);\n"
        ),
        "{ddl}"
    );
    let rows = vec![vec![
        row(&[
            Some("integer"),
            Some("0"),
            Some("yes"),
            Some("positive"),
            Some("CHECK (VALUE > 0)"),
        ]),
        row(&[
            Some("integer"),
            Some("0"),
            Some("yes"),
            Some("small"),
            Some("CHECK (VALUE < 10)"),
        ]),
    ]];
    let target = object("amount", ObjectKind::Domain, "");
    let ddl = definition_text(Dialect::Postgres, &target, "shop", &rows).unwrap();
    assert!(ddl.contains(text::DEFINITION_WRITTEN), "{ddl}");
    assert!(
        ddl.ends_with(
            "CREATE DOMAIN \"public\".\"amount\" AS integer\n    DEFAULT 0\n    NOT NULL\n    \
             CONSTRAINT \"positive\" CHECK (VALUE > 0)\n    \
             CONSTRAINT \"small\" CHECK (VALUE < 10);\n"
        ),
        "{ddl}"
    );
}

#[test]
fn the_text_of_a_table_has_its_keys_its_indexes_and_its_foreign_keys() {
    let column = |name: &str, ty: &str| Column {
        name: name.into(),
        ty: ty.into(),
        nullable: name != "id",
        ..Column::default()
    };
    let orders = Table {
        name: "orders".into(),
        columns: vec![column("id", "integer"), column("customer_id", "integer")],
        primary: vec!["id".into()],
        primary_name: Some("orders_pkey".into()),
        constraints: vec![Constraint {
            name: "orders_customer_fk".into(),
            kind: ConstraintKind::Foreign,
            columns: vec!["customer_id".into()],
            target: Some(KeyTarget {
                schema: None,
                table: "customers".into(),
            }),
            target_columns: vec!["id".into()],
            definition: "FOREIGN KEY (customer_id) REFERENCES customers(id)".into(),
        }],
        indexes: vec![Index {
            name: "orders_customer".into(),
            unique: false,
            parts: vec!["customer_id".into()],
            definition: "btree (customer_id)".into(),
        }],
        sql: None,
    };
    let model = SchemaModel {
        dialect: Dialect::Postgres,
        schema: "public".into(),
        tables: BTreeMap::from([("orders".to_owned(), orders)]),
    };
    assert_eq!(
        create_text(&model, "orders").unwrap(),
        "CREATE TABLE \"public\".\"orders\" (\n    \"id\" integer NOT NULL,\n    \
         \"customer_id\" integer,\n    CONSTRAINT \"orders_pkey\" PRIMARY KEY (\"id\")\n);\n\
         CREATE INDEX \"orders_customer\" ON \"public\".\"orders\" USING btree (customer_id);\n\
         ALTER TABLE \"public\".\"orders\" ADD CONSTRAINT \"orders_customer_fk\" \
         FOREIGN KEY (customer_id) REFERENCES customers(id);"
    );
    assert!(create_text(&model, "missing").is_none());
}
