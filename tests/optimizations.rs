use sqln::grid::{ResultTable, Value};

#[test]
fn selected_tsv_keeps_quotes_spaces_binary_and_missing_values() {
    let mut table = ResultTable::new(vec!["a".into(), "b".into(), "c".into()]);
    table.append(
        vec![
            vec![
                Value::Text("skip".into()),
                Value::Text("a\tb\r\n\" ".into()),
                Value::binary([0xff, 0x00]),
            ]
            .into_boxed_slice(),
            vec![Value::Null, Value::Text("".into())].into_boxed_slice(),
        ],
        0,
    );
    assert_eq!(table.tsv(0, 1, 1, 2), "\"a\tb\r\n\"\" \"\t0xFF00\n\"\"\t\n");
}

#[test]
fn binary_classification_uses_the_complete_value() {
    let mut invalid = vec![b'x'; 100_000];
    invalid.push(0xff);
    let mut control = vec![b'x'; 100_000];
    control.push(0);
    for bytes in [invalid, control] {
        let value = Value::binary(bytes);
        for _ in 0..10 {
            assert_eq!(value.preview(4), "0x7878");
        }
    }
    let value = Value::binary("日本語".as_bytes());
    assert_eq!(value.preview(4), "日本");
    assert_eq!(value.full_text(), "日本語");
}

#[test]
fn binary_classification_preserves_bytes_and_the_memory_budget() {
    let value = Value::binary(vec![0, 255, 17]);
    let Value::Binary(bytes) = &value else {
        panic!("not binary")
    };
    assert_eq!(&bytes[..], &[0, 255, 17]);
    let row = vec![value].into_boxed_slice();
    let cost = sqln::grid::row_cost(&row);
    let budget = sqln::grid::Budget::new(cost);
    assert_eq!(
        budget.reserve(&row),
        sqln::grid::Reservation::Accepted(cost)
    );
    assert_eq!(budget.used(), cost);
    budget.release(cost);
    assert_eq!(budget.used(), 0);
}
