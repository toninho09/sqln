// The ER diagram: its queries on real SQLite, its model, its layout, and its Mermaid text.
// `tests/servers.rs` runs the queries of PostgreSQL and MySQL.

mod common;

mod compare_schema;
mod diagram_tab;

use std::collections::BTreeSet;

use rusqlite::Connection;
use rusqlite::types::ValueRef;
use sqln::catalog::RelationRef;
use sqln::diagram::layout::{ARM_DOWN, ARM_LEFT, ARM_RIGHT, ARM_UP, Direction, Layout, LinePiece};
use sqln::diagram::mermaid::to_mermaid;
use sqln::diagram::sql::{diagram_sql, parse_column, parse_key};
use sqln::diagram::{ColumnRow, Focus, KeyRow, Model};
use sqln::grid::{Row, Value};
use sqln::sql::Dialect;

const SETUP: &str = "
    CREATE TABLE pairs (x INT, y INT, PRIMARY KEY (x, y));
    CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT NOT NULL, boss INT REFERENCES users);
    CREATE TABLE orders (
        id INTEGER PRIMARY KEY,
        user_id INTEGER NOT NULL REFERENCES users (id),
        owner INTEGER REFERENCES Users,
        a INT,
        b INT,
        note,
        FOREIGN KEY (a, b) REFERENCES pairs (x, y)
    );
    CREATE TABLE loose (v TEXT);
    CREATE VIEW paid AS SELECT id FROM orders;
";

fn relation(schema: &str, table: &str) -> RelationRef {
    RelationRef {
        schema: schema.to_owned(),
        relation: table.to_owned(),
    }
}

/// Runs a query on SQLite and returns its rows as grid rows.
fn rows(connection: &Connection, sql: &str) -> Vec<Row> {
    let mut statement = connection.prepare(sql).unwrap();
    let count = statement.column_count();
    statement
        .query_map([], |row| {
            let texts: Vec<Option<String>> = (0..count)
                .map(|index| match row.get_ref(index).unwrap() {
                    ValueRef::Null => None,
                    ValueRef::Integer(number) => Some(number.to_string()),
                    ValueRef::Text(text) => Some(String::from_utf8_lossy(text).into_owned()),
                    other => panic!("not text: {other:?}"),
                })
                .collect();
            Ok(texts
                .iter()
                .map(|text| text.as_deref().map_or(Value::Null, Value::Text))
                .collect::<Row>())
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn sqlite_model(setup: &str, focus: &Focus) -> Model {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(setup).unwrap();
    let [columns, keys] = diagram_sql(Dialect::Sqlite, "main").unwrap();
    let columns = rows(&connection, &columns)
        .iter()
        .map(|row| parse_column(row).unwrap())
        .collect();
    let keys = rows(&connection, &keys)
        .iter()
        .map(|row| parse_key(row).unwrap())
        .collect();
    Model::build(columns, keys, focus)
}

fn names(model: &Model) -> Vec<String> {
    model
        .entities
        .iter()
        .map(|entity| entity.relation.relation.clone())
        .collect()
}

/// Describes each link as `child(columns) -> parent(columns)`.
fn links(model: &Model) -> Vec<String> {
    model
        .links
        .iter()
        .map(|link| {
            let child = &model.entities[link.child];
            let parent = &model.entities[link.parent];
            let from: Vec<&str> = link
                .child_columns
                .iter()
                .map(|&column| child.columns[column].name.as_str())
                .collect();
            let to: Vec<&str> = link
                .parent_columns
                .iter()
                .map(|column| column.map_or("?", |column| parent.columns[column].name.as_str()))
                .collect();
            format!(
                "{}({}) -> {}({})",
                child.relation.relation,
                from.join(","),
                parent.relation.relation,
                to.join(",")
            )
        })
        .collect()
}

#[test]
fn redis_has_no_diagram() {
    assert_eq!(diagram_sql(Dialect::Redis, "0"), None);
}

#[test]
fn a_hostile_schema_name_stays_inside_its_literal() {
    for dialect in [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite] {
        let [columns, keys] = diagram_sql(dialect, "x' OR '1'='1").unwrap();
        for sql in [columns, keys] {
            assert!(!sql.contains("= 'x' OR"), "{dialect:?}: {sql}");
        }
    }
}

#[test]
fn sqlite_reads_the_tables_their_columns_and_their_keys() {
    let model = sqlite_model(SETUP, &Focus::Schema("main".to_owned()));
    // The view stays out, and the order is the order of the names.
    assert_eq!(names(&model), ["loose", "orders", "pairs", "users"]);
    assert_eq!(
        links(&model),
        [
            "orders(a,b) -> pairs(x,y)",
            "orders(owner) -> users(id)",
            "orders(user_id) -> users(id)",
            "users(boss) -> users(id)",
        ]
    );
    let orders = &model.entities[1];
    let note = orders.columns.iter().find(|column| column.name == "note");
    assert_eq!(note.map(|column| column.ty.as_str()), Some(""));
    let user_id = &orders.columns[1];
    assert!(user_id.foreign && !user_id.nullable && user_id.primary.is_none());
    let pairs = &model.entities[2];
    assert_eq!(pairs.columns[1].primary, Some(2));
    assert!(pairs.columns[0].referenced);
}

#[test]
fn a_table_focus_keeps_the_table_and_its_neighbors() {
    let focus = Focus::Table(relation("main", "pairs"));
    let model = sqlite_model(SETUP, &focus);
    assert_eq!(names(&model), ["orders", "pairs"]);
    assert_eq!(links(&model), ["orders(a,b) -> pairs(x,y)"]);
    // The column of a key to a table out of the diagram is not a key here.
    let owner = model.entities[0].columns.iter().find(|c| c.name == "owner");
    assert!(!owner.unwrap().foreign);

    let gone = sqlite_model(SETUP, &Focus::Table(relation("main", "missing")));
    assert!(gone.entities.is_empty());
}

fn column(table: &str, name: &str, primary: Option<u32>) -> ColumnRow {
    ColumnRow {
        relation: relation("app", table),
        name: name.to_owned(),
        ty: "integer".to_owned(),
        nullable: primary.is_none(),
        primary,
    }
}

fn key(table: &str, name: &str, column: &str, target: RelationRef, to: &str) -> KeyRow {
    KeyRow {
        relation: relation("app", table),
        key: name.to_owned(),
        column: column.to_owned(),
        target,
        target_column: Some(to.to_owned()),
    }
}

#[test]
fn a_table_of_another_schema_becomes_a_stub_on_both_sides() {
    let columns = vec![
        column("orders", "id", Some(1)),
        column("orders", "user_id", None),
    ];
    let keys = vec![
        key(
            "orders",
            "fk_user",
            "user_id",
            relation("other", "users"),
            "id",
        ),
        KeyRow {
            relation: relation("billing", "invoices"),
            key: "fk_order".to_owned(),
            column: "order_id".to_owned(),
            target: relation("app", "orders"),
            target_column: Some("id".to_owned()),
        },
    ];
    let model = Model::build(columns, keys, &Focus::Schema("app".to_owned()));
    let stubs: Vec<(String, bool)> = model
        .entities
        .iter()
        .map(|entity| (entity.relation.schema.clone(), entity.stub))
        .collect();
    assert_eq!(
        stubs,
        [
            ("app".to_owned(), false),
            ("billing".to_owned(), true),
            ("other".to_owned(), true),
        ]
    );
    assert_eq!(
        links(&model),
        ["orders(user_id) -> users(?)", "invoices() -> orders(id)"]
    );
}

#[test]
fn the_same_key_name_in_two_tables_gives_two_links() {
    let columns = vec![
        column("a", "id", Some(1)),
        column("b", "a_id", None),
        column("c", "a_id", None),
    ];
    let target = relation("app", "a");
    let keys = vec![
        key("b", "fk", "a_id", target.clone(), "id"),
        key("c", "fk", "a_id", target, "id"),
    ];
    let model = Model::build(columns, keys, &Focus::Schema("app".to_owned()));
    assert_eq!(links(&model), ["b(a_id) -> a(id)", "c(a_id) -> a(id)"]);
}

#[test]
fn mermaid_writes_entities_attributes_and_cardinality() {
    let model = sqlite_model(SETUP, &Focus::Table(relation("main", "pairs")));
    assert_eq!(
        to_mermaid(&model, false),
        "erDiagram\n    \
         e1[\"orders\"] {\n        \
         INTEGER id PK\n        \
         INTEGER user_id\n        \
         INTEGER owner\n        \
         INT a FK\n        \
         INT b FK\n        \
         any note\n    \
         }\n    \
         e2[\"pairs\"] {\n        \
         INT x PK\n        \
         INT y PK\n    \
         }\n    \
         e2 |o..o{ e1 : \"a, b\"\n"
    );
    assert_eq!(
        to_mermaid(&model, true),
        "erDiagram\n    \
         e1[\"orders\"] {\n        \
         INTEGER id PK\n        \
         INT a FK\n        \
         INT b FK\n    \
         }\n    \
         e2[\"pairs\"] {\n        \
         INT x PK\n        \
         INT y PK\n    \
         }\n    \
         e2 |o..o{ e1 : \"a, b\"\n"
    );
}

#[test]
fn mermaid_marks_a_required_and_an_identifying_key() {
    let mut columns = vec![
        column("parent", "id", Some(1)),
        column("child", "parent_id", Some(1)),
    ];
    columns[1].nullable = false;
    let keys = vec![key(
        "child",
        "fk",
        "parent_id",
        relation("app", "parent"),
        "id",
    )];
    let model = Model::build(columns, keys, &Focus::Schema("app".to_owned()));
    let text = to_mermaid(&model, false);
    assert!(text.contains("integer parent_id PK, FK\n"), "{text}");
    assert!(text.ends_with("e2 ||--o{ e1 : \"parent_id\"\n"), "{text}");
}

#[test]
fn mermaid_cleans_names_that_it_cannot_hold() {
    let columns = vec![ColumnRow {
        relation: relation("app", "a \"b\"\u{1b}[2J"),
        name: "1st \"col\"".to_owned(),
        ty: "character varying(10)".to_owned(),
        nullable: true,
        primary: None,
    }];
    let model = Model::build(columns, Vec::new(), &Focus::Schema("app".to_owned()));
    let text = to_mermaid(&model, false);
    assert!(!text.contains('\u{1b}'), "{text}");
    assert!(text.contains("e1[\"a 'b'�[2J\"]"), "{text}");
    assert!(
        text.contains("character_varying(10) c1st__col_ \"1st 'col'\""),
        "{text}"
    );
}

// Layout.

/// A schema where `t<n>` has a key to each table in `targets(n)`.
fn generated(count: usize, targets: impl Fn(usize) -> Vec<usize>) -> Model {
    let name = |index: usize| format!("t{index:03}");
    let mut columns = Vec::new();
    let mut keys = Vec::new();
    for index in 0..count {
        columns.push(column(&name(index), "id", Some(1)));
        columns.push(column(&name(index), "label", None));
        for (place, target) in targets(index).into_iter().enumerate() {
            let fk = format!("ref_{place}");
            columns.push(column(&name(index), &fk, None));
            keys.push(key(
                &name(index),
                &fk,
                &fk,
                relation("app", &name(target)),
                "id",
            ));
        }
    }
    Model::build(columns, keys, &Focus::Schema("app".to_owned()))
}

/// The rows where a link meets its boxes: the key column of the child and the read
/// column of the parent.
fn pins(model: &Model, layout: &Layout, link: usize) -> ((usize, u32), (usize, u32)) {
    let link = &model.links[link];
    let row = |entity: usize, column: Option<usize>| {
        let placed = &layout.boxes[entity];
        let place = column.and_then(|column| placed.rows.iter().position(|&r| r == column));
        (entity, placed.y + 1 + place.unwrap_or(0) as u32)
    };
    (
        row(link.child, link.child_columns.first().copied()),
        row(link.parent, link.parent_columns.first().copied().flatten()),
    )
}

/// Lists each line cell: the row, the column, the joined arms, and the pieces.
fn all_cells(layout: &Layout) -> Vec<(u32, u32, u8, Vec<LinePiece>)> {
    (0..layout.height)
        .flat_map(|y| {
            layout
                .cells(y)
                .map(move |(x, arms, pieces)| (y, x, arms, pieces.to_vec()))
        })
        .collect()
}

/// Checks the rules that make the picture readable.
fn check(model: &Model, layout: &Layout) {
    assert_eq!(layout.conflicts, 0);
    assert_eq!(layout.boxes.len(), model.entities.len());
    for (index, a) in layout.boxes.iter().enumerate() {
        for b in &layout.boxes[index + 1..] {
            let apart = a.x + a.width <= b.x
                || b.x + b.width <= a.x
                || a.y + a.height <= b.y
                || b.y + b.height <= a.y;
            assert!(apart, "boxes overlap: {a:?} {b:?}");
        }
    }
    for (y, x, arms, pieces) in all_cells(layout) {
        assert!(x < layout.width && y < layout.height);
        if let Some(entity) = layout.box_at(x, y) {
            let placed = &layout.boxes[entity];
            let on_side = x == placed.x || x == placed.x + placed.width - 1;
            let inside_rows = y > placed.y && y + 1 < placed.y + placed.height;
            assert!(
                on_side && inside_rows && arms & (ARM_UP | ARM_DOWN) == ARM_UP | ARM_DOWN,
                "line inside box {entity} at {x},{y}"
            );
        }
        if pieces.len() > 1 && arms != ARM_UP | ARM_DOWN | ARM_LEFT | ARM_RIGHT {
            // Lines may join only where they meet the same row of a box.
            let ends: Vec<BTreeSet<(usize, u32)>> = pieces
                .iter()
                .map(|piece| {
                    let (child, parent) = pins(model, layout, piece.link);
                    BTreeSet::from([child, parent])
                })
                .collect();
            for pair in ends.windows(2) {
                assert!(
                    !pair[0].is_disjoint(&pair[1]),
                    "two lines share {x},{y} with no common end"
                );
            }
        }
    }
    for arrow in &layout.arrows {
        assert_eq!(layout.box_at(arrow.x, arrow.y), None, "arrow inside a box");
    }
    for (index, link) in model.links.iter().enumerate() {
        let ((_, child_row), (_, parent_row)) = pins(model, layout, index);
        if link.child == link.parent && child_row == parent_row {
            continue;
        }
        let child = &layout.boxes[link.child];
        let port = [child.x, child.x + child.width - 1].into_iter().any(|x| {
            let cell = layout.cell(x, child_row);
            cell.is_some_and(|(_, pieces)| pieces.iter().any(|piece| piece.link == index))
        });
        assert!(port, "link {index} does not leave its child row");
        let parent = &layout.boxes[link.parent];
        let arrow = layout.arrows.iter().any(|arrow| {
            arrow.link == index
                && arrow.y == parent_row
                && (arrow.x + 1 == parent.x || arrow.x == parent.x + parent.width)
        });
        assert!(arrow, "link {index} has no arrow at its parent row");
    }
}

#[test]
fn a_chain_places_each_parent_on_the_left() {
    let model = generated(4, |index| if index > 0 { vec![index - 1] } else { vec![] });
    let layout = Layout::new(&model, false, 1);
    check(&model, &layout);
    let xs: Vec<u32> = layout.boxes.iter().map(|placed| placed.x).collect();
    assert!(xs.windows(2).all(|pair| pair[0] < pair[1]), "{xs:?}");
}

#[test]
fn cycles_self_keys_and_long_keys_keep_their_own_paths() {
    // t0 <- t1 <- t2 <- t3, t3 -> t0 (long), t0 -> t3 (cycle), t1 -> t1 (self).
    let model = generated(5, |index| match index {
        0 => vec![3],
        1 => vec![0, 1],
        2 => vec![1],
        3 => vec![2, 0],
        _ => vec![],
    });
    let layout = Layout::new(&model, false, 1);
    check(&model, &layout);
    // The table without keys goes under the layers.
    let loose = &layout.boxes[4];
    let bottom = layout.boxes[..4]
        .iter()
        .map(|b| b.y + b.height)
        .max()
        .unwrap();
    assert!(loose.y > bottom);
}

#[test]
fn many_keys_to_one_table_join_at_its_row() {
    let model = generated(12, |index| if index > 0 { vec![0] } else { vec![] });
    let layout = Layout::new(&model, false, 1);
    check(&model, &layout);
    assert_eq!(layout.arrows.len(), 11);
}

/// A large schema with keys in each direction, from a fixed pseudo-random sequence.
fn large(count: usize) -> Model {
    generated(count, |index| {
        let mut seed = (index as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let mut targets = Vec::new();
        for _ in 0..(seed % 3) {
            seed ^= seed >> 13;
            seed = seed.wrapping_mul(0xBF58_476D_1CE4_E5B9);
            targets.push((seed % count as u64) as usize);
        }
        targets
    })
}

#[test]
fn a_large_schema_keeps_the_rules() {
    let model = large(120);
    for keys_only in [false, true] {
        let layout = Layout::new(&model, keys_only, 2);
        check(&model, &layout);
    }
}

#[test]
fn the_layout_does_not_depend_on_the_order_of_the_rows() {
    let model = large(40);
    let mut columns = Vec::new();
    let mut keys = Vec::new();
    for entity in model.entities.iter().rev() {
        for column in &entity.columns {
            columns.push(ColumnRow {
                relation: entity.relation.clone(),
                name: column.name.clone(),
                ty: column.ty.clone(),
                nullable: column.nullable,
                primary: column.primary,
            });
        }
    }
    for link in model.links.iter().rev() {
        let child = &model.entities[link.child];
        let parent = &model.entities[link.parent];
        for (&from, to) in link.child_columns.iter().zip(&link.parent_columns) {
            keys.push(KeyRow {
                relation: child.relation.clone(),
                key: link.name.clone(),
                column: child.columns[from].name.clone(),
                target: parent.relation.clone(),
                target_column: to.map(|to| parent.columns[to].name.clone()),
            });
        }
    }
    let again = Model::build(columns, keys, &Focus::Schema("app".to_owned()));
    assert_eq!(again, model);
    assert_eq!(Layout::new(&again, false, 1), Layout::new(&model, false, 1));
}

#[test]
fn keys_only_keeps_each_key_row_and_hides_the_rest() {
    let model = generated(3, |index| if index > 0 { vec![0] } else { vec![] });
    let full = Layout::new(&model, false, 1);
    let keys = Layout::new(&model, true, 1);
    check(&model, &keys);
    // "label" leaves each box.
    for (full, keys) in full.boxes.iter().zip(&keys.boxes) {
        assert_eq!(keys.rows.len() + 1, full.rows.len());
    }
}

#[test]
fn nearest_follows_the_direction() {
    let model = generated(3, |index| if index > 0 { vec![0] } else { vec![] });
    let layout = Layout::new(&model, false, 1);
    // t000 on the left; t001 and t002 stacked on the right.
    assert_eq!(layout.nearest(0, Direction::Right), Some(1));
    assert_eq!(layout.nearest(1, Direction::Down), Some(2));
    assert_eq!(layout.nearest(2, Direction::Up), Some(1));
    assert_eq!(layout.nearest(2, Direction::Left), Some(0));
    assert_eq!(layout.nearest(0, Direction::Left), None);
}

/// Draws a layout as text, with the names of the boxes and of their columns.
fn picture(model: &Model, layout: &Layout) -> Vec<String> {
    let mut grid = vec![vec![' '; layout.width as usize]; layout.height as usize];
    for (entity, placed) in model.entities.iter().zip(&layout.boxes) {
        let (x, y, w, h) = (
            placed.x as usize,
            placed.y as usize,
            placed.width as usize,
            placed.height as usize,
        );
        grid[y][x..x + w].fill('─');
        grid[y + h - 1][x..x + w].fill('─');
        for row in &mut grid[y..y + h] {
            row[x] = '│';
            row[x + w - 1] = '│';
        }
        let title: Vec<char> = entity.relation.relation.chars().collect();
        for (i, c) in title.iter().enumerate().take(w.saturating_sub(4)) {
            grid[y][x + 2 + i] = *c;
        }
        for (place, &column) in placed.rows.iter().enumerate() {
            let name: Vec<char> = entity.columns[column].name.chars().collect();
            for (i, c) in name.iter().enumerate().take(w.saturating_sub(4)) {
                grid[y + 1 + place][x + 2 + i] = *c;
            }
        }
    }
    for (y, x, arms, _) in all_cells(layout) {
        grid[y as usize][x as usize] = sqln::diagram::layout::glyph(arms);
    }
    for arrow in &layout.arrows {
        grid[arrow.y as usize][arrow.x as usize] = if arrow.left { '◀' } else { '▶' };
    }
    grid.into_iter()
        .map(|row| row.into_iter().collect::<String>().trim_end().to_owned())
        .collect()
}

#[test]
fn a_schema_gives_this_picture() {
    let model = sqlite_model(SETUP, &Focus::Schema("main".to_owned()));
    let layout = Layout::new(&model, false, 1);
    assert_eq!(
        picture(&model, &layout),
        [
            "│─pairs────│",
            "│ x        │◀─────────┐        │─orders─────────────│",
            "│ y        │          │        │ id                 │",
            "│──────────│          │ ┌──────┤ user_id            │",
            "                      │ │ ┌────┤ owner              │",
            "│─users────────────│  └─┼─┼────┤ a                  │",
            "│ id               │◀───┴─┴─┐  │ b                  │",
            "│ email            │        │  │ note               │",
            "│ boss             ├────────┘  │────────────────────│",
            "│──────────────────│",
            "",
            "",
            "│─loose─────│",
            "│ v         │",
            "│───────────│",
        ]
    );
}

#[test]
fn right_skips_a_wider_box_below_in_the_same_layer() {
    use sqln::diagram::layout::Placed;

    let place = |x, y, width, height| Placed {
        x,
        y,
        width,
        height,
        ..Placed::default()
    };
    // A narrow box over a wide box in one layer, and a box in the next layer.
    let layout = Layout {
        boxes: vec![place(0, 0, 20, 5), place(0, 7, 25, 5), place(31, 0, 26, 7)],
        ..Layout::default()
    };
    assert_eq!(layout.nearest(0, Direction::Right), Some(2));
    assert_eq!(layout.nearest(0, Direction::Down), Some(1));
    assert_eq!(layout.nearest(1, Direction::Right), Some(2));
    assert_eq!(layout.nearest(2, Direction::Left), Some(0));
    assert_eq!(layout.nearest(1, Direction::Up), Some(0));
}
