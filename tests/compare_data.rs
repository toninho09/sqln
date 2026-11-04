//! Data compare: the equality of values, the match of rows by key, the budget, and the
//! script of rows.

use sqln::compare::data::{DataCompare, RowChange, decimal, same, value_literal};
use sqln::grid::{Binary, Budget, Reservation, Row, Value};
use sqln::sql::Dialect;

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

fn row(values: &[Value]) -> Row {
    values.iter().copied().collect()
}

#[test]
fn a_number_has_one_decimal_form() {
    for (text, form) in [
        ("10.50", "10.5"),
        ("10.5", "10.5"),
        ("1.05e1", "10.5"),
        ("0010", "10"),
        ("1e3", "1000"),
        ("-0.000", "0"),
        ("-12.300", "-12.3"),
        ("+7", "7"),
        (".5", "0.5"),
        ("5.", "5"),
        (
            "123456789012345678901234567890",
            "123456789012345678901234567890",
        ),
        ("1E-3", "0.001"),
    ] {
        assert_eq!(decimal(text).as_deref(), Some(form), "{text}");
    }
    for text in ["NaN", "Infinity", "", ".", "1e", "1-2", "0x10", "1e999999"] {
        assert_eq!(decimal(text), None, "{text}");
    }
}

#[test]
fn values_compare_by_number_text_and_bytes() {
    assert!(same(Value::Null, Value::Null));
    assert!(!same(Value::Null, Value::Text("")));
    assert!(same(Value::Number("10.50"), Value::Number("10.5")));
    // Large integers stay exact: an f64 would make these two equal.
    assert!(!same(
        Value::Number("9007199254740993"),
        Value::Number("9007199254740992")
    ));
    assert!(same(Value::Number("5"), Value::Text("5")));
    assert!(!same(Value::Text("5.0"), Value::Text("5")));
    assert!(!same(Value::Text("t"), Value::Number("1")));
    assert!(same(Value::Binary(Binary::new(b"ab")), Value::Text("ab")));
}

/// A compare of `id`, `name`, and `price`, with `id` as the key.
fn compare() -> DataCompare {
    DataCompare::new(names(&["id", "name", "price"]), &names(&["id"])).expect("a key column")
}

#[test]
fn rows_match_by_key_and_an_equal_row_leaves_the_budget() {
    let budget = Budget::new(1 << 20);
    let mut data = compare();
    data.add_source(vec![
        row(&[Value::Number("1"), Value::Text("a"), Value::Number("1.0")]),
        row(&[Value::Number("2"), Value::Text("b"), Value::Number("2")]),
        row(&[Value::Number("3"), Value::Text("c"), Value::Null]),
    ]);
    // The target has its columns in another order and one more column.
    data.set_target_columns(&names(&["price", "extra", "name", "id"]), &[])
        .unwrap();
    let target = vec![
        row(&[
            Value::Number("1"),
            Value::Text("x"),
            Value::Text("a"),
            Value::Number("1"),
        ]),
        row(&[
            Value::Number("2"),
            Value::Null,
            Value::Text("B"),
            Value::Number("2.00"),
        ]),
        row(&[
            Value::Number("4"),
            Value::Null,
            Value::Text("d"),
            Value::Number("4"),
        ]),
    ];
    let mut reserved = 0;
    for item in &target {
        match budget.reserve(item) {
            Reservation::Accepted(cost) => reserved += cost,
            other => panic!("{other:?}"),
        }
    }
    let kept = data.add_target(target, &budget);
    // The equal row leaves at once, and the two other rows stay.
    assert_eq!(budget.used(), kept);
    assert!(kept < reserved);
    data.finish();
    assert_eq!(data.changes.len(), 3, "{:?}", data.changes);
    assert!(matches!(
        &data.changes[0],
        RowChange::Update { source: 1, columns, .. } if columns == &[1]
    ));
    assert!(matches!(&data.changes[1], RowChange::Delete(_)));
    assert_eq!(data.changes[2], RowChange::Insert(2));
}

#[test]
fn a_missing_key_column_and_a_missing_target_column_refuse() {
    assert!(DataCompare::new(names(&["a"]), &names(&["id"])).is_none());
    let mut data = compare();
    assert_eq!(
        data.set_target_columns(&names(&["id", "name"]), &[]),
        Err("price".to_owned())
    );
}

#[test]
fn the_script_writes_one_statement_per_row_in_one_transaction() {
    let budget = Budget::new(1 << 20);
    let mut data = compare();
    data.add_source(vec![
        row(&[Value::Number("1"), Value::Text("it's"), Value::Number("1")]),
        row(&[Value::Number("2"), Value::Text("b"), Value::Null]),
    ]);
    // The target makes `price` itself, so the script leaves it out.
    data.set_target_columns(&names(&["id", "name", "price"]), &names(&["price"]))
        .unwrap();
    data.add_target(
        vec![
            row(&[Value::Number("1"), Value::Text("a"), Value::Number("9")]),
            row(&[Value::Number("3"), Value::Text("c"), Value::Number("3")]),
        ],
        &budget,
    );
    data.finish();
    let table = "\"s\".\"t\"";
    assert_eq!(
        data.script(table, Dialect::Postgres, false, true),
        "BEGIN;\n\
         UPDATE \"s\".\"t\" SET \"name\" = E'it''s' WHERE \"id\" = 1;\n\
         -- DELETE FROM \"s\".\"t\" WHERE \"id\" = 3;\n\
         INSERT INTO \"s\".\"t\" (\"id\", \"name\") OVERRIDING SYSTEM VALUE VALUES (2, E'b');\n\
         COMMIT;"
    );
    let script = data.script(table, Dialect::MySql, true, false);
    assert!(
        script.contains("DELETE FROM \"s\".\"t\" WHERE `id` = 3;\n"),
        "{script}"
    );
    assert!(script.contains("VALUES (2, 'b');"), "{script}");
}

#[test]
fn a_value_becomes_a_literal_of_the_target() {
    assert_eq!(value_literal(Value::Null, Dialect::MySql), "NULL");
    assert_eq!(
        value_literal(Value::Number("10.50"), Dialect::Sqlite),
        "10.50"
    );
    assert_eq!(
        value_literal(Value::Number("NaN"), Dialect::Postgres),
        "E'NaN'"
    );
    assert_eq!(
        value_literal(Value::Text("a\\b"), Dialect::MySql),
        "_utf8mb4 0x615C62"
    );
    assert_eq!(
        value_literal(Value::Binary(Binary::new(&[0, 255])), Dialect::Postgres),
        "E'\\\\x00FF'"
    );
    assert_eq!(
        value_literal(Value::Binary(Binary::new(&[0, 255])), Dialect::Sqlite),
        "X'00FF'"
    );
}

/// Gives a data compare of `main.t` the rows of its two column reads, and returns the tab
/// at the step of the source rows.
fn data_tab(budget: &Budget) -> sqln::compare::tab::CompareTab {
    use sqln::compare::tab::{CompareTab, SideRead};
    use sqln::compare::{Kind, Side};
    use sqln::exec::{ExecEvent, ExecutionEnd, StatementKey};

    let side = |connection| Side {
        connection,
        schema: "main".to_owned(),
        table: Some("t".to_owned()),
    };
    let sides = [
        SideRead::new(10, side(1), Dialect::Sqlite, "a".to_owned()),
        SideRead::new(11, side(2), Dialect::Sqlite, "b".to_owned()),
    ];
    let mut tab = CompareTab::new(1, Kind::Data, sides, false);
    tab.begin(budget);
    for (console, execution) in [(10, 1), (11, 2)] {
        tab.started(console, execution);
        let cells = [
            Value::Text("t"),
            Value::Text("id"),
            Value::Text("INTEGER"),
            Value::Text("no"),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Number("1"),
            Value::Null,
            Value::Null,
        ];
        let rows = vec![row(&cells)];
        let bytes = rows.iter().map(sqln::grid::row_cost).sum();
        for item in &rows {
            budget.reserve(item);
        }
        let key = StatementKey {
            console,
            execution,
            index: 0,
        };
        tab.on_event(ExecEvent::Rows { key, rows, bytes }, budget);
        tab.on_event(
            ExecEvent::ExecutionFinished {
                console,
                execution,
                end: ExecutionEnd::Completed,
                message: None,
            },
            budget,
        );
    }
    tab
}

#[test]
fn a_data_compare_reads_the_source_rows_after_the_columns() {
    let budget = Budget::new(1 << 20);
    let mut tab = data_tab(&budget);
    assert!(tab.is_busy());
    let (side, statements) = tab.take_next().expect("the read of the source rows");
    assert_eq!(side, 0);
    assert_eq!(statements, ["SELECT * FROM \"main\".\"t\""]);
    assert!(tab.take_next().is_none(), "the next read comes once");
    // The rows of the column reads left the budget.
    assert_eq!(budget.used(), 0);
}

#[test]
fn a_canceled_password_of_a_later_step_ends_the_data_compare() {
    let budget = Budget::new(1 << 20);
    let mut tab = data_tab(&budget);
    tab.take_next();
    tab.canceled(10, &budget);
    assert!(!tab.is_busy());
    assert_eq!(
        tab.message.as_ref().map(|message| message.text.as_str()),
        Some(sqln::ui::text::TABLE_CANCELED)
    );
}
