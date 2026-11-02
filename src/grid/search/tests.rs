use super::*;
use crate::grid::Value;

fn table(values: Vec<Value>) -> ResultTable {
    let mut table = ResultTable::new(vec!["value".into()]);
    table.append(vec![values.into_iter().collect()], 0);
    table
}

#[test]
fn a_search_resumes_inside_a_multibyte_cell() {
    let table = table(vec![Value::Text("xx日本語AÇÃOyy")]);
    let mut search = RowSearch::new((0, 0));
    search.push_str("日本語ação");
    search.scan_step(&table, 4);
    assert_eq!(search.next_row, 0);
    assert_eq!(search.offset, 8);
    assert_eq!(search.total(), 0);
    while search.has_work(&table) {
        search.scan_step(&table, 2);
    }
    assert!(search.is_match(0, 0));
    assert_eq!(search.total(), 1);
}

#[test]
fn a_long_prefix_fallback_also_yields_between_steps() {
    let table = table(vec![Value::Text(&("a".repeat(5000) + "c"))]);
    let mut search = RowSearch::new((0, 0));
    search.push_str(&("a".repeat(5000) + "b"));
    search.scan_step(&table, 5000);
    assert_eq!(search.offset, 5000);
    search.scan_step(&table, CHARS_PER_STEP);
    assert_eq!(search.offset, 5000);
    assert!(search.has_work(&table));
    while search.has_work(&table) {
        search.scan_step(&table, CHARS_PER_STEP);
    }
    assert_eq!(search.total(), 0);
}

#[test]
fn a_query_change_clears_the_partial_match() {
    let table = table(vec![Value::Text("aaaaab")]);
    let mut search = RowSearch::new((0, 0));
    search.push_str("aaaab");
    search.scan_step(&table, 3);
    search.pop();
    assert_eq!(search.offset, 0);
    while search.has_work(&table) {
        search.scan_step(&table, 1);
    }
    assert_eq!(search.total(), 1);
}

#[test]
fn binary_search_streams_hexadecimal_and_its_prefix() {
    let table = table(vec![Value::binary(&[0xff, 0xab, 0x00])]);
    for query in ["0xff", "FfAb", "ab00", "xFFAB00"] {
        let mut search = RowSearch::new((0, 0));
        search.push_str(query);
        while search.has_work(&table) {
            search.scan_step(&table, 1);
        }
        assert_eq!(search.total(), 1, "{query}");
    }
}

#[test]
fn an_expired_deadline_does_not_finish_a_large_cell() {
    let table = table(vec![Value::Text(&("x".repeat(100_000) + "needle"))]);
    let mut search = RowSearch::new((0, 0));
    search.push_str("needle");
    search.scan(&table, Instant::now());
    assert_eq!(search.next_row, 0);
    assert_eq!(search.offset, CHARS_PER_STEP);
    assert!(search.has_work(&table));
}

#[test]
fn matching_resets_between_cells_and_handles_repeated_prefixes() {
    let table = table(vec![
        Value::Text("aaaaa"),
        Value::Text("b"),
        Value::Text("aaaaaaaaaaaaaaaaab"),
    ]);
    let mut search = RowSearch::new((0, 0));
    search.push_str("aaaaab");
    while search.has_work(&table) {
        search.scan_step(&table, 1);
    }
    assert_eq!(search.matches, vec![(0, 2)]);
}
