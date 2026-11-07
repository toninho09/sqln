use sqln::grid::{ResultTable, Value};

#[test]
fn selected_tsv_keeps_quotes_spaces_binary_and_missing_values() {
    let mut table = ResultTable::new(vec!["a".into(), "b".into(), "c".into()]);
    table.append(
        vec![
            vec![
                Value::Text("skip"),
                Value::Text("a\tb\r\n\" "),
                Value::binary(&[0xff, 0x00]),
            ]
            .into_iter()
            .collect(),
            vec![Value::Null, Value::Text("")].into_iter().collect(),
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
        let value = Value::binary(&bytes);
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
    let value = Value::binary(&[0, 255, 17]);
    let Value::Binary(bytes) = value else {
        panic!("not binary")
    };
    assert_eq!(&bytes[..], &[0, 255, 17]);
    let row: sqln::grid::Row = [value].into_iter().collect();
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

/// The scan of a field reads eight bytes at a time, so it must find a separator at every
/// place of a text, including the bytes after the blocks of eight.
#[test]
fn the_scan_of_a_field_finds_a_separator_at_every_place() {
    for length in 0..=40 {
        for at in 0..length {
            for separator in ['\t', '\n', '\r', '"'] {
                let mut text: Vec<char> = (0..length).map(|_| 'a').collect();
                text[at] = separator;
                let text: String = text.into_iter().collect();
                let expected = format!("\"{}\"", text.replace('"', "\"\""));
                assert_eq!(
                    sqln::grid::delimited_text(&text, b'\t', ""),
                    expected,
                    "length {length}, place {at}, separator {separator:?}"
                );
            }
        }
    }
    let plain = "a text with no separator at all, and a comma";
    assert_eq!(sqln::grid::delimited_text(plain, b'\t', ""), plain);
    let semicolon = "a;b";
    assert_eq!(sqln::grid::delimited_text(semicolon, b';', ""), "\"a;b\"");
    assert_eq!(sqln::grid::delimited_text(semicolon, b'\t', ""), semicolon);
}

/// The digits of each byte come from a table of pairs, which must keep the order of the
/// high and the low digit.
#[test]
fn hexadecimal_keeps_the_high_digit_before_the_low_one() {
    let bytes: Vec<u8> = (0..=255).collect();
    let text = sqln::grid::hex(&bytes);
    let expected: String = bytes.iter().map(|byte| format!("{byte:02X}")).collect();
    assert_eq!(text, format!("0x{expected}"));
    assert_eq!(sqln::grid::hex(&[]), "0x");
    for length in 1..=20 {
        let bytes: Vec<u8> = (0..length as u8).collect();
        let text = sqln::grid::hex(&bytes);
        let expected: String = bytes.iter().map(|byte| format!("{byte:02X}")).collect();
        assert_eq!(text, format!("0x{expected}"), "length {length}");
    }
}
