// The queries of a structure tab. The tests of text need no database, and the data tests
// use real SQLite. `tests/servers.rs` runs the queries of PostgreSQL and MySQL.

use rusqlite::Connection;
use rusqlite::types::ValueRef;
use sqln::catalog::RelationRef;
use sqln::sql::Dialect;
use sqln::structure::{Scope, Section, Structure, structure_sql};

const SETUP: &str = "
    CREATE TABLE pairs (x INT, y INT, PRIMARY KEY (x, y));
    CREATE TABLE users (
        id INTEGER PRIMARY KEY,
        email TEXT NOT NULL UNIQUE,
        active BOOLEAN DEFAULT 1
    );
    CREATE TABLE orders (
        id INTEGER PRIMARY KEY,
        user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
        owner INTEGER REFERENCES users,
        total REAL,
        a INT,
        b INT,
        FOREIGN KEY (a, b) REFERENCES pairs (x, y)
    );
    CREATE INDEX orders_pair ON orders (b, a);
    CREATE INDEX orders_double ON orders (total * 2);
    CREATE VIEW paid AS SELECT id FROM orders;
";

fn relation(schema: &str, table: &str) -> RelationRef {
    RelationRef {
        schema: schema.to_owned(),
        relation: table.to_owned(),
    }
}

fn database() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(SETUP).unwrap();
    connection
}

/// Runs a query and returns its header and its rows as text.
fn read(connection: &Connection, sql: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let mut statement = connection.prepare(sql).unwrap();
    let header: Vec<String> = statement
        .column_names()
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let count = header.len();
    let rows = statement
        .query_map([], |row| {
            (0..count)
                .map(|index| {
                    Ok(match row.get_ref(index)? {
                        ValueRef::Null => "NULL".to_owned(),
                        ValueRef::Integer(number) => number.to_string(),
                        ValueRef::Text(text) => String::from_utf8_lossy(text).into_owned(),
                        other => panic!("not text: {other:?}"),
                    })
                })
                .collect()
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    (header, rows)
}

fn sqlite_grid(
    connection: &Connection,
    scope: Scope,
    target: &RelationRef,
    section: Section,
) -> (Vec<String>, Vec<Vec<String>>) {
    let sql = structure_sql(Dialect::Sqlite, scope, target, section).unwrap();
    read(connection, &sql)
}

#[test]
fn the_columns_of_a_table_show_the_type_the_null_rule_the_default_and_the_key() {
    let connection = database();
    let target = relation("main", "users");
    let (header, rows) = sqlite_grid(&connection, Scope::Relation, &target, Section::Columns);
    assert_eq!(header, ["name", "type", "nullable", "default", "key"]);
    assert_eq!(
        rows,
        [
            ["id", "INTEGER", "yes", "NULL", "PK"],
            ["email", "TEXT", "no", "NULL", "NULL"],
            ["active", "BOOLEAN", "yes", "1", "NULL"],
        ]
    );
}

#[test]
fn each_column_of_a_composite_primary_key_is_marked() {
    let connection = database();
    let target = relation("main", "pairs");
    let (_, rows) = sqlite_grid(&connection, Scope::Relation, &target, Section::Columns);
    assert_eq!(rows[0][4], "PK");
    assert_eq!(rows[1][4], "PK");
}

#[test]
fn an_index_keeps_the_order_of_its_columns_and_names_an_expression() {
    let connection = database();
    let target = relation("main", "orders");
    let (header, rows) = sqlite_grid(&connection, Scope::Relation, &target, Section::Indexes);
    assert_eq!(header, ["name", "columns", "unique"]);
    assert_eq!(
        rows,
        [
            ["orders_double", "expression", "no"],
            ["orders_pair", "b, a", "no"],
        ]
    );
}

#[test]
fn an_index_shows_that_it_is_unique() {
    let connection = database();
    let target = relation("main", "users");
    let (_, rows) = sqlite_grid(&connection, Scope::Relation, &target, Section::Indexes);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][1..], ["email", "yes"]);
}

#[test]
fn a_foreign_key_shows_its_columns_its_target_and_its_actions() {
    let connection = database();
    let target = relation("main", "orders");
    let (header, rows) = sqlite_grid(&connection, Scope::Relation, &target, Section::ForeignKeys);
    assert_eq!(
        header,
        ["name", "columns", "references", "on_update", "on_delete"]
    );
    let cells: Vec<&[String]> = rows.iter().map(|row| &row[1..]).collect();
    assert_eq!(
        cells,
        [
            ["a, b", "pairs (x, y)", "NO ACTION", "NO ACTION"],
            ["owner", "users", "NO ACTION", "NO ACTION"],
            ["user_id", "users (id)", "NO ACTION", "CASCADE"],
        ]
    );
}

#[test]
fn a_view_has_columns_and_no_keys() {
    let connection = database();
    let target = relation("main", "paid");
    let columns = sqlite_grid(&connection, Scope::Relation, &target, Section::Columns);
    assert_eq!(columns.1[0][0], "id");
    let indexes = sqlite_grid(&connection, Scope::Relation, &target, Section::Indexes);
    assert!(indexes.1.is_empty());
    let keys = sqlite_grid(&connection, Scope::Relation, &target, Section::ForeignKeys);
    assert!(keys.1.is_empty());
}

#[test]
fn a_schema_lists_each_column_of_its_tables_and_views() {
    let connection = database();
    let target = relation("main", "");
    let (header, rows) = sqlite_grid(&connection, Scope::Schema, &target, Section::Columns);
    assert_eq!(
        header,
        ["schema", "relation", "name", "type", "nullable", "default"]
    );
    let places: Vec<(&str, &str)> = rows
        .iter()
        .map(|row| (row[1].as_str(), row[2].as_str()))
        .collect();
    assert_eq!(places[0], ("orders", "id"));
    assert!(places.contains(&("paid", "id")));
    assert!(places.contains(&("users", "active")));
    assert!(!rows.iter().any(|row| row[1].starts_with("sqlite_")));
}

#[test]
fn a_schema_reads_only_itself_and_a_connection_reads_each_schema() {
    let connection = database();
    connection
        .execute_batch("ATTACH ':memory:' AS other; CREATE TABLE other.extra (id INT);")
        .unwrap();
    connection
        .execute_batch("CREATE TEMP TABLE scratch (id INT);")
        .unwrap();

    let schema = relation("other", "");
    let (_, rows) = sqlite_grid(&connection, Scope::Schema, &schema, Section::Columns);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][..3], ["other", "extra", "id"]);

    let all = RelationRef::default();
    let (_, rows) = sqlite_grid(&connection, Scope::Connection, &all, Section::Columns);
    assert!(rows.iter().any(|row| row[0] == "main"));
    assert!(rows.iter().any(|row| row[0] == "other"));
    assert!(!rows.iter().any(|row| row[1] == "scratch"));
}

#[test]
fn a_hostile_name_stays_a_name() {
    let connection = database();
    let name = "x'); DROP TABLE users; --";
    connection
        .execute_batch(&format!("CREATE TABLE \"{name}\" (id INT);"))
        .unwrap();
    let target = relation("main", name);
    for section in Section::ALL {
        sqlite_grid(&connection, Scope::Relation, &target, section);
    }
    let (_, rows) = sqlite_grid(&connection, Scope::Relation, &target, Section::Columns);
    assert_eq!(rows[0][0], "id");
    read(&connection, "SELECT * FROM users");
}

#[test]
fn postgres_writes_a_name_in_a_literal_that_ignores_the_session_settings() {
    let target = relation("pu'b", "t\\");
    let sql = structure_sql(
        Dialect::Postgres,
        Scope::Relation,
        &target,
        Section::Columns,
    )
    .unwrap();
    assert!(sql.contains(r"n.nspname = E'pu''b'"), "{sql}");
    assert!(sql.contains(r"c.relname = E't\\'"), "{sql}");
}

#[test]
fn mysql_doubles_a_backslash_in_a_name() {
    let target = relation("app", "t\\'x");
    let sql = structure_sql(Dialect::MySql, Scope::Relation, &target, Section::Indexes).unwrap();
    assert!(sql.contains(r"TABLE_NAME = 't\\''x'"), "{sql}");
}

#[test]
fn a_connection_hides_the_schemas_that_the_navigator_hides() {
    let all = RelationRef::default();
    let postgres =
        structure_sql(Dialect::Postgres, Scope::Connection, &all, Section::Columns).unwrap();
    assert!(postgres.contains("n.nspname !~ '^pg_'"), "{postgres}");
    assert!(postgres.contains("'information_schema'"), "{postgres}");
    let mysql = structure_sql(Dialect::MySql, Scope::Connection, &all, Section::Columns).unwrap();
    assert!(mysql.contains("'performance_schema'"), "{mysql}");
}

#[test]
fn redis_has_no_structure() {
    let all = RelationRef::default();
    for scope in [Scope::Relation, Scope::Schema, Scope::Connection] {
        assert_eq!(
            structure_sql(Dialect::Redis, scope, &all, Section::Columns),
            None
        );
    }
}

#[test]
fn the_sections_turn_in_a_circle() {
    assert_eq!(Section::Columns.step(true), Section::Indexes);
    assert_eq!(Section::ForeignKeys.step(true), Section::Columns);
    assert_eq!(Section::Columns.step(false), Section::ForeignKeys);
}

#[test]
fn only_a_relation_has_sections() {
    let name = "name".to_owned();
    assert!(Structure::new(Scope::Relation, name.clone()).has_sections());
    assert!(!Structure::new(Scope::Schema, name.clone()).has_sections());
    assert!(!Structure::new(Scope::Connection, name).has_sections());
}
