//! Schema compare: the reads of SQLite, the differences, and the script. A SQLite round
//! trip runs the script and compares again, so the target must end equal to the source.

use rusqlite::Connection;
use rusqlite::types::ValueRef;

use sqln::compare::ddl::script;
use sqln::compare::schema::{
    Change, Column, Constraint, ConstraintKind, Difference, Index, Object, SchemaModel, Table,
    Target, build, diff,
};
use sqln::compare::sql::compare_sql;
use sqln::grid::{Binary, Row, Value};
use sqln::sql::Dialect;

/// Runs the queries of the compare on a SQLite file and builds its model.
fn read(connection: &Connection) -> SchemaModel {
    let statements = compare_sql(Dialect::Sqlite, "main").expect("SQLite has queries");
    let results: Vec<Vec<Row>> = statements
        .iter()
        .map(|sql| {
            let mut statement = connection.prepare(sql).expect("valid query");
            let columns = statement.column_count();
            let mut rows = statement.query([]).expect("query runs");
            let mut read = Vec::new();
            while let Some(row) = rows.next().expect("row") {
                let values: Vec<ValueRef> = (0..columns)
                    .map(|index| row.get_ref(index).expect("value"))
                    .collect();
                let texts: Vec<String> = values
                    .iter()
                    .map(|value| match value {
                        ValueRef::Integer(number) => number.to_string(),
                        ValueRef::Real(number) => number.to_string(),
                        ValueRef::Text(text) => String::from_utf8_lossy(text).into_owned(),
                        _ => String::new(),
                    })
                    .collect();
                read.push(
                    values
                        .iter()
                        .zip(&texts)
                        .map(|(value, text)| match value {
                            ValueRef::Null => Value::Null,
                            ValueRef::Integer(_) | ValueRef::Real(_) => Value::Number(text),
                            ValueRef::Text(_) => Value::Text(text),
                            ValueRef::Blob(bytes) => Value::Binary(Binary::new(bytes)),
                        })
                        .collect(),
                );
            }
            read
        })
        .collect();
    build(Dialect::Sqlite, "main", &results)
}

fn open(sql: &str) -> Connection {
    let connection = Connection::open_in_memory().expect("database");
    connection.execute_batch(sql).expect("schema");
    connection
}

/// Compares two SQLite schemas, runs the script on the target, and returns the
/// differences before the run, the script, and the differences after the run.
fn round_trip(
    source: &str,
    target: &str,
    removals: bool,
) -> (Vec<Difference>, String, Vec<Difference>, Connection) {
    let source = open(source);
    let target = open(target);
    let (from, to) = (read(&source), read(&target));
    let differences = diff(&from, &to);
    let text = script(&from, &to, &differences, removals);
    target.execute_batch(&text).unwrap_or_else(|error| {
        panic!("the script runs: {error}\n{text}");
    });
    let after = diff(&from, &read(&target));
    (differences, text, after, target)
}

#[test]
fn equal_schemas_have_no_differences_and_no_script() {
    let schema = "CREATE TABLE a (id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT 'x');
                  CREATE INDEX a_name ON a (name);";
    let (from, to) = (read(&open(schema)), read(&open(schema)));
    let differences = diff(&from, &to);
    assert!(differences.is_empty(), "{differences:?}");
    assert!(script(&from, &to, &differences, true).is_empty());
}

#[test]
fn the_sqlite_model_reads_columns_keys_and_indexes() {
    let model = read(&open(
        "CREATE TABLE parent (id INTEGER PRIMARY KEY, code TEXT UNIQUE);
         CREATE TABLE child (id INTEGER, parent INTEGER REFERENCES parent ON DELETE CASCADE,
                             note TEXT DEFAULT 'none' NOT NULL, PRIMARY KEY (id));
         CREATE INDEX child_note ON child (note DESC) WHERE note <> '';",
    ));
    let child = &model.tables["child"];
    assert_eq!(child.primary, ["id"]);
    let note = child.column("note").expect("column");
    assert_eq!(note.default.as_deref(), Some("'none'"));
    assert!(!note.nullable);
    let key = &child.constraints[0];
    assert_eq!(key.kind, ConstraintKind::Foreign);
    // A key without target columns reads the primary key of its target.
    assert_eq!(key.target_columns, ["id"]);
    assert!(
        key.definition.contains("ON DELETE CASCADE"),
        "{}",
        key.definition
    );
    assert_eq!(child.indexes[0].definition, "(note DESC) WHERE note <> ''");
    let parent = &model.tables["parent"];
    assert_eq!(parent.constraints[0].kind, ConstraintKind::Unique);
    assert!(
        parent
            .sql
            .as_deref()
            .is_some_and(|sql| sql.starts_with("CREATE TABLE"))
    );
}

#[test]
fn a_new_table_with_its_index_comes_to_the_target() {
    let (differences, text, after, _) = round_trip(
        "CREATE TABLE a (id INTEGER PRIMARY KEY, name TEXT);
         CREATE INDEX a_name ON a (name);",
        "",
        false,
    );
    assert_eq!(differences.len(), 1);
    assert_eq!(differences[0].change, Change::Create);
    assert!(text.contains("CREATE TABLE \"main\".\"a\" (id INTEGER PRIMARY KEY, name TEXT);"));
    assert!(text.contains("CREATE INDEX \"main\".\"a_name\" ON \"a\" (name);"));
    assert!(after.is_empty(), "{after:?}");
}

#[test]
fn a_simple_column_uses_alter_table_and_keeps_the_rows() {
    let (_, text, after, target) = round_trip(
        "CREATE TABLE a (id INTEGER PRIMARY KEY, name TEXT, size INTEGER DEFAULT 0);",
        "CREATE TABLE a (id INTEGER PRIMARY KEY, name TEXT);
         INSERT INTO a VALUES (1, 'one');",
        false,
    );
    assert!(text.contains("ALTER TABLE \"main\".\"a\" ADD COLUMN \"size\" INTEGER DEFAULT 0;"));
    assert!(!text.contains("sqln_new_"), "{text}");
    assert!(after.is_empty(), "{after:?}");
    let size: i64 = target
        .query_row("SELECT size FROM a WHERE id = 1", [], |row| row.get(0))
        .expect("row");
    assert_eq!(size, 0);
}

#[test]
fn a_changed_column_rebuilds_the_table_and_keeps_the_rows_and_indexes() {
    let (differences, text, after, target) = round_trip(
        "CREATE TABLE a (id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT '');
         CREATE INDEX a_name ON a (name);",
        "CREATE TABLE a (id INTEGER PRIMARY KEY, name TEXT);
         CREATE INDEX a_name ON a (name);
         INSERT INTO a VALUES (1, 'one'), (2, 'two');",
        false,
    );
    assert_eq!(differences.len(), 1, "{differences:?}");
    assert!(
        text.starts_with("PRAGMA foreign_keys = OFF;\nBEGIN;"),
        "{text}"
    );
    assert!(text.contains("ALTER TABLE \"main\".\"sqln_new_a\" RENAME TO \"a\";"));
    assert!(
        text.ends_with("COMMIT;\nPRAGMA foreign_keys = ON;"),
        "{text}"
    );
    assert!(after.is_empty(), "{after:?}");
    let count: i64 = target
        .query_row("SELECT count(*) FROM a", [], |row| row.get(0))
        .expect("count");
    assert_eq!(count, 2);
}

#[test]
fn a_new_foreign_key_rebuilds_the_table_of_the_key() {
    let (_, text, after, _) = round_trip(
        "CREATE TABLE p (id INTEGER PRIMARY KEY);
         CREATE TABLE c (id INTEGER PRIMARY KEY, p INTEGER REFERENCES p (id));",
        "CREATE TABLE p (id INTEGER PRIMARY KEY);
         CREATE TABLE c (id INTEGER PRIMARY KEY, p INTEGER);
         INSERT INTO p VALUES (1); INSERT INTO c VALUES (1, 1);",
        false,
    );
    assert!(text.contains("sqln_new_c"), "{text}");
    assert!(!text.contains("sqln_new_p"), "{text}");
    assert!(after.is_empty(), "{after:?}");
}

#[test]
fn removals_stay_comments_by_default() {
    let (differences, text, after, target) = round_trip(
        "CREATE TABLE a (id INTEGER PRIMARY KEY);",
        "CREATE TABLE a (id INTEGER PRIMARY KEY, extra TEXT);
         CREATE INDEX a_extra ON a (extra);
         CREATE TABLE old (id INTEGER);",
        false,
    );
    assert_eq!(differences.len(), 3, "{differences:?}");
    assert!(
        text.contains("-- DROP INDEX \"main\".\"a_extra\";"),
        "{text}"
    );
    assert!(text.contains("-- ALTER TABLE \"main\".\"a\" DROP COLUMN \"extra\";"));
    assert!(text.contains("-- DROP TABLE \"main\".\"old\";"));
    assert_eq!(after, differences);
    let tables: i64 = target
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type = 'table'",
            [],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(tables, 2);
}

#[test]
fn removals_on_request_make_the_target_equal() {
    let (_, text, after, _) = round_trip(
        "CREATE TABLE a (id INTEGER PRIMARY KEY);",
        "CREATE TABLE a (id INTEGER PRIMARY KEY, extra TEXT);
         CREATE INDEX a_extra ON a (extra);
         CREATE TABLE old (id INTEGER);",
        true,
    );
    assert!(!text.contains("-- "), "{text}");
    assert!(after.is_empty(), "{after:?}");
}

#[test]
fn a_rebuild_without_removals_keeps_the_columns_of_the_target() {
    let (_, text, after, target) = round_trip(
        "CREATE TABLE a (id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT '');",
        "CREATE TABLE a (id INTEGER PRIMARY KEY, name TEXT, extra TEXT);
         INSERT INTO a VALUES (1, 'one', 'kept');",
        false,
    );
    assert!(text.contains("ADD COLUMN \"extra\" TEXT;"), "{text}");
    // Only the removal of the kept column stays.
    assert_eq!(after.len(), 1, "{after:?}");
    assert_eq!(after[0].change, Change::Drop);
    let extra: String = target
        .query_row("SELECT extra FROM a", [], |row| row.get(0))
        .expect("row");
    assert_eq!(extra, "kept");
}

fn column(name: &str, ty: &str, nullable: bool) -> Column {
    Column {
        name: name.to_owned(),
        ty: ty.to_owned(),
        nullable,
        ..Column::default()
    }
}

fn model(dialect: Dialect, schema: &str, tables: Vec<Table>) -> SchemaModel {
    SchemaModel {
        dialect,
        schema: schema.to_owned(),
        tables: tables
            .into_iter()
            .map(|table| (table.name.clone(), table))
            .collect(),
    }
}

fn table(name: &str, columns: Vec<Column>, primary: &[&str]) -> Table {
    Table {
        name: name.to_owned(),
        columns,
        primary: primary.iter().map(|name| (*name).to_owned()).collect(),
        ..Table::default()
    }
}

#[test]
fn between_dialects_only_the_null_rule_of_a_column_counts() {
    let from = model(
        Dialect::Postgres,
        "public",
        vec![table(
            "a",
            vec![column("id", "integer", false), column("name", "text", true)],
            &["id"],
        )],
    );
    let to = model(
        Dialect::MySql,
        "app",
        vec![table(
            "a",
            vec![
                column("id", "int", false),
                column("name", "varchar(20)", false),
            ],
            &["id"],
        )],
    );
    let differences = diff(&from, &to);
    assert_eq!(differences.len(), 1, "{differences:?}");
    assert_eq!(differences[0].object().name(), "name");
}

#[test]
fn between_dialects_a_unique_key_matches_a_unique_index_and_names_do_not_count() {
    let mut from = table("a", vec![column("code", "text", true)], &[]);
    from.constraints.push(Constraint {
        name: "a_code_key".to_owned(),
        kind: ConstraintKind::Unique,
        columns: vec!["code".to_owned()],
        target: None,
        target_columns: Vec::new(),
        definition: "UNIQUE (code)".to_owned(),
    });
    let mut to = table("a", vec![column("code", "varchar(9)", true)], &[]);
    to.indexes.push(Index {
        name: "code".to_owned(),
        unique: true,
        parts: vec!["code".to_owned()],
        definition: "(`code`)".to_owned(),
    });
    let differences = diff(
        &model(Dialect::Postgres, "public", vec![from]),
        &model(Dialect::MySql, "app", vec![to]),
    );
    assert!(differences.is_empty(), "{differences:?}");
}

fn foreign(name: &str, column: &str, target: &str) -> Constraint {
    Constraint {
        name: name.to_owned(),
        kind: ConstraintKind::Foreign,
        columns: vec![column.to_owned()],
        target: Some(Target {
            schema: None,
            table: target.to_owned(),
        }),
        target_columns: vec!["id".to_owned()],
        definition: format!("FOREIGN KEY ({column}) REFERENCES {target}(id)"),
    }
}

#[test]
fn the_postgres_script_creates_tables_before_their_foreign_keys() {
    let mut serial = column("id", "integer", false);
    serial.auto = Some("serial".to_owned());
    let mut child = table(
        "child",
        vec![serial.clone(), column("parent", "integer", true)],
        &["id"],
    );
    child.primary_name = Some("child_pkey".to_owned());
    child
        .constraints
        .push(foreign("child_parent_fkey", "parent", "parent"));
    child.indexes.push(Index {
        name: "child_parent".to_owned(),
        unique: false,
        parts: vec!["parent".to_owned()],
        definition: "btree (parent)".to_owned(),
    });
    let parent = table("parent", vec![serial], &["id"]);
    let from = model(Dialect::Postgres, "app", vec![child, parent]);
    let to = model(Dialect::Postgres, "app", Vec::new());
    let differences = diff(&from, &to);
    let text = script(&from, &to, &differences, false);
    assert_eq!(
        text,
        "BEGIN;\n\
         SET LOCAL search_path TO \"app\";\n\
         CREATE TABLE \"app\".\"child\" (\n    \"id\" serial NOT NULL,\n    \"parent\" integer,\n    \
         CONSTRAINT \"child_pkey\" PRIMARY KEY (\"id\")\n);\n\
         CREATE TABLE \"app\".\"parent\" (\n    \"id\" serial NOT NULL,\n    PRIMARY KEY (\"id\")\n);\n\
         CREATE INDEX \"child_parent\" ON \"app\".\"child\" USING btree (parent);\n\
         ALTER TABLE \"app\".\"child\" ADD CONSTRAINT \"child_parent_fkey\" \
         FOREIGN KEY (parent) REFERENCES parent(id);\n\
         COMMIT;"
    );
}

#[test]
fn the_postgres_script_changes_a_column_in_steps() {
    let mut from_column = column("size", "bigint", false);
    from_column.default = Some("1".to_owned());
    let mut to_column = column("size", "integer", true);
    to_column.default = Some("0".to_owned());
    let from = model(
        Dialect::Postgres,
        "public",
        vec![table("a", vec![from_column], &[])],
    );
    let to = model(
        Dialect::Postgres,
        "public",
        vec![table("a", vec![to_column], &[])],
    );
    let differences = diff(&from, &to);
    let text = script(&from, &to, &differences, false);
    let column = "ALTER TABLE \"public\".\"a\" ALTER COLUMN \"size\"";
    assert_eq!(
        text,
        format!(
            "BEGIN;\nSET LOCAL search_path TO \"public\";\n\
             {column} DROP DEFAULT;\n\
             {column} TYPE bigint USING \"size\"::bigint;\n\
             {column} SET DEFAULT 1;\n\
             {column} SET NOT NULL;\n\
             COMMIT;"
        )
    );
}

#[test]
fn a_changed_key_drops_first_and_a_key_only_in_the_target_stays_a_comment() {
    let mut from = table("a", vec![column("p", "integer", true)], &[]);
    from.constraints.push(foreign("a_p_fkey", "p", "p2"));
    let mut to = table("a", vec![column("p", "integer", true)], &[]);
    to.constraints.push(foreign("a_p_fkey", "p", "p1"));
    to.constraints.push(foreign("a_old_fkey", "p", "p1"));
    let from = model(Dialect::Postgres, "s", vec![from]);
    let to = model(Dialect::Postgres, "s", vec![to]);
    let differences = diff(&from, &to);
    let text = script(&from, &to, &differences, false);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[2..5],
        [
            "ALTER TABLE \"s\".\"a\" DROP CONSTRAINT \"a_p_fkey\";",
            "-- ALTER TABLE \"s\".\"a\" DROP CONSTRAINT \"a_old_fkey\";",
            "ALTER TABLE \"s\".\"a\" ADD CONSTRAINT \"a_p_fkey\" FOREIGN KEY (p) REFERENCES p2(id);",
        ]
    );
}

#[test]
fn the_mysql_script_writes_whole_columns_and_names_the_schema_of_a_key_target() {
    let mut name = column("name", "varchar(20)", false);
    name.charset = Some("CHARACTER SET utf8mb4 COLLATE utf8mb4_bin".to_owned());
    name.default = Some("'x'".to_owned());
    let mut key = foreign("a_ibfk_1", "p", "p");
    key.definition =
        "FOREIGN KEY (`p`) REFERENCES `p` (`id`) ON UPDATE RESTRICT ON DELETE CASCADE".to_owned();
    let mut from = table(
        "a",
        vec![column("id", "int", false), column("p", "int", true), name],
        &["id", "p"],
    );
    from.constraints.push(key);
    from.indexes.push(Index {
        name: "a_text".to_owned(),
        unique: false,
        parts: vec!["name".to_owned()],
        definition: "FULLTEXT (`name`)".to_owned(),
    });
    let to = table(
        "a",
        vec![
            column("id", "int", false),
            column("p", "int", true),
            column("name", "varchar(10)", true),
        ],
        &["id"],
    );
    let from = model(Dialect::MySql, "app", vec![from]);
    let to = model(Dialect::MySql, "app", vec![to]);
    let differences = diff(&from, &to);
    let text = script(&from, &to, &differences, false);
    assert_eq!(
        text,
        "ALTER TABLE `app`.`a` MODIFY COLUMN `name` varchar(20) \
         CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL DEFAULT 'x';\n\
         ALTER TABLE `app`.`a` DROP PRIMARY KEY, ADD PRIMARY KEY (`id`, `p`);\n\
         CREATE FULLTEXT INDEX `a_text` ON `app`.`a` (`name`);\n\
         ALTER TABLE `app`.`a` ADD CONSTRAINT `a_ibfk_1` FOREIGN KEY (`p`) \
         REFERENCES `app`.`p` (`id`) ON UPDATE RESTRICT ON DELETE CASCADE;"
    );
}

#[test]
fn an_object_of_a_removal_comes_from_the_target() {
    let from = model(Dialect::MySql, "app", Vec::new());
    let to = model(
        Dialect::MySql,
        "app",
        vec![table("gone", vec![column("id", "int", false)], &[])],
    );
    let differences = diff(&from, &to);
    assert!(matches!(differences[0].object(), Object::Table(table) if table.name == "gone"));
    assert_eq!(
        script(&from, &to, &differences, false),
        "-- DROP TABLE `app`.`gone`;"
    );
}
