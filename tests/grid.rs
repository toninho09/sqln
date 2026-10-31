// Tests of the grid data without a screen: row search, column widths, and large values.

use std::time::{Duration, Instant};

use sqln::exec::{ExecEvent, Flow, ResultSink, StatementKey};
use sqln::grid::{Budget, ResultTable, Row, RowSearch, Value};
use sqln::ui::wrap::WrappedText;

fn text(value: &str) -> Value {
    Value::Text(value.into())
}

fn row(values: &[&str]) -> Row {
    values.iter().map(|value| text(value)).collect()
}

fn table(rows: &[&[&str]]) -> ResultTable {
    let columns = rows[0].iter().enumerate().map(|(index, _)| format!("c{index}")).collect();
    let mut table = ResultTable::new(columns);
    table.append(rows.iter().map(|values| row(values)).collect(), 0);
    table
}

fn later() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

fn search(table: &ResultTable, query: &str, origin: (usize, usize)) -> (RowSearch, Option<(usize, usize)>) {
    let mut search = RowSearch::new(origin);
    search.push_str(query);
    let first = search.scan(table, later());
    (search, first)
}

#[test]
fn the_search_ignores_case_and_reads_each_column() {
    let table = table(&[&["alpha", "BETA"], &["gamma", "Ação"], &["delta", "beta"]]);
    let (found, first) = search(&table, "beta", (0, 0));
    assert_eq!(first, Some((0, 1)));
    assert_eq!(found.total(), 2);
    assert!(found.is_match(2, 1));
    assert!(!found.is_match(1, 1));

    let (found, first) = search(&table, "AÇÃO", (0, 0));
    assert_eq!(first, Some((1, 1)));
    assert_eq!(found.total(), 1);
}

#[test]
fn the_search_reads_every_value_kind() {
    let mut table = ResultTable::new(vec!["a".into(), "b".into(), "c".into()]);
    let row: Row = Box::new([
        Value::Null,
        Value::Number("12345".into()),
        Value::Binary(Box::new([0xFF, 0xAB])),
    ]);
    table.append(vec![row], 0);
    assert_eq!(search(&table, "null", (0, 0)).1, Some((0, 0)));
    assert_eq!(search(&table, "234", (0, 0)).1, Some((0, 1)));
    assert_eq!(search(&table, "0xffab", (0, 0)).1, Some((0, 2)));
}

#[test]
fn the_search_finds_a_value_past_the_visible_part() {
    let long = format!("{}needle", "x".repeat(10_000));
    let table = table(&[&[long.as_str()]]);
    assert_eq!(search(&table, "NEEDLE", (0, 0)).0.total(), 1);
}

#[test]
fn an_empty_query_does_no_work_and_a_miss_reports_no_position() {
    let table = table(&[&["a"], &["b"]]);
    let (found, first) = search(&table, "", (0, 0));
    assert_eq!(first, None);
    assert!(!found.has_work(&table));

    let (mut found, first) = search(&table, "zzz", (0, 0));
    assert_eq!(first, None);
    assert_eq!((found.position(), found.total()), (0, 0));
    assert!(!found.is_partial(&table));
    assert_eq!(found.advance(true), None);
    assert_eq!(found.advance(false), None);
}

#[test]
fn the_first_result_is_at_or_after_the_cursor() {
    let table = table(&[&["a", "x"], &["a", "x"], &["x", "a"]]);
    assert_eq!(search(&table, "a", (1, 0)).1, Some((1, 0)));
    assert_eq!(search(&table, "a", (1, 1)).1, Some((2, 1)));
}

#[test]
fn without_a_result_after_the_cursor_the_search_uses_the_first_one() {
    let table = table(&[&["a"], &["b"], &["b"]]);
    assert_eq!(search(&table, "a", (2, 0)).1, Some((0, 0)));
}

#[test]
fn the_results_wrap_in_both_directions() {
    let table = table(&[&["a"], &["b"], &["a"]]);
    let (mut found, _) = search(&table, "a", (0, 0));
    assert_eq!(found.position(), 1);
    assert_eq!(found.advance(true), Some((2, 0)));
    assert_eq!(found.position(), 2);
    assert!(found.is_current(2, 0));
    assert_eq!(found.advance(true), Some((0, 0)));
    assert_eq!(found.advance(false), Some((2, 0)));
}

#[test]
fn a_new_query_starts_again() {
    let table = table(&[&["ab"], &["ac"]]);
    let (mut found, _) = search(&table, "ab", (0, 0));
    assert_eq!(found.total(), 1);
    found.pop();
    assert_eq!(found.query(), "a");
    assert_eq!(found.total(), 0);
    assert!(found.has_work(&table));
    found.scan(&table, later());
    assert_eq!(found.total(), 2);
}

#[test]
fn an_expired_deadline_reads_one_row_and_the_work_continues() {
    let table = table(&[&["a"], &["a"], &["a"]]);
    let mut found = RowSearch::new((0, 0));
    found.push_str("a");
    found.scan(&table, Instant::now());
    assert_eq!(found.total(), 1);
    assert!(found.has_work(&table));
    assert!(found.is_partial(&table));
    found.scan(&table, later());
    assert_eq!(found.total(), 3);
    assert!(!found.is_partial(&table));
}

#[test]
fn rows_that_arrive_later_enter_the_search() {
    let mut table = table(&[&["a"]]);
    let (mut found, _) = search(&table, "a", (0, 0));
    assert!(!found.has_work(&table));
    table.append(vec![row(&["a"])], 0);
    assert!(found.has_work(&table));
    found.scan(&table, later());
    assert_eq!(found.total(), 2);
}

#[test]
fn the_list_of_results_has_a_limit() {
    let columns = (0..10).map(|index| format!("c{index}")).collect();
    let mut table = ResultTable::new(columns);
    let rows: Vec<Row> = (0..100_100).map(|_| row(&["a"; 10])).collect();
    table.append(rows, 0);
    let (found, _) = search(&table, "a", (0, 0));
    assert_eq!(found.total(), 1_000_000);
    assert!(!found.has_work(&table));
    assert!(found.is_partial(&table));
}

#[test]
fn a_manual_width_stays_when_new_rows_arrive() {
    let mut table = ResultTable::new(vec!["a".into()]);
    assert_eq!(table.width(0), 4);
    table.resize(0, 2);
    assert_eq!(table.width(0), 6);
    table.append(vec![row(&["a long value of the column"])], 0);
    assert_eq!(table.width(0), 6);
    table.reset_width(0);
    assert_eq!(table.width(0), 26);
}

#[test]
fn the_width_has_limits() {
    let mut table = ResultTable::new(vec!["a".into()]);
    table.resize(0, -100);
    assert_eq!(table.width(0), 4);
    table.resize(0, 1000);
    assert_eq!(table.width(0), 200);
    table.resize(7, 1);
    table.reset_width(7);
}

#[test]
fn the_automatic_width_keeps_its_limit() {
    let table = table(&[&[&"x".repeat(100)]]);
    assert_eq!(table.width(0), 40);
}

#[test]
fn a_preview_is_the_start_of_the_display_text() {
    let values = [
        Value::Null,
        text("plain"),
        text("a\tb\nc"),
        text("日本語のテキスト"),
        Value::Number("12345".into()),
        Value::Binary(Box::new([0xFF, 0x00, 0xAB, 0x10])),
        Value::Binary(Box::new(*b"text")),
    ];
    for value in values {
        let display = value.display();
        for width in [0, 1, 2, 3, 5, 8, 40] {
            let preview = value.preview(width);
            if matches!(value, Value::Null) {
                continue;
            }
            assert!(display.starts_with(&preview), "{display:?} {preview:?}");
        }
        assert_eq!(value.preview(100), display);
    }
}

#[test]
fn a_preview_of_a_large_value_reads_only_its_start() {
    let value = text(&"x".repeat(50_000_000));
    let started = Instant::now();
    for _ in 0..1000 {
        assert_eq!(value.preview(40).len(), 40);
    }
    assert!(started.elapsed() < Duration::from_millis(500), "{:?}", started.elapsed());
}

#[test]
fn a_sink_that_nobody_finishes_returns_its_rows_to_the_budget() {
    let (events, _inbox) = tokio::sync::mpsc::channel::<ExecEvent>(8);
    let budget = std::sync::Arc::new(Budget::new(1 << 20));
    let key = StatementKey { console: 1, execution: 1, index: 0 };
    let mut sink = ResultSink::new(key, events, budget.clone(), 1000);
    sink.push_blocking(row(&["a value in the batch"]));
    assert!(budget.used() > 0);
    drop(sink);
    assert_eq!(budget.used(), 0);
}

fn rows_of(text: &str, width: usize) -> Vec<String> {
    let mut wrapped = WrappedText::new(text.to_owned());
    wrapped.set_width(width);
    let mut rows = Vec::new();
    while let Some(row) = wrapped.row(rows.len()) {
        rows.push(row.to_owned());
    }
    rows
}

#[test]
fn a_row_breaks_after_a_space_and_keeps_the_text() {
    assert_eq!(rows_of("one two three", 8), ["one two ", "three"]);
    assert_eq!(rows_of("one two", 7), ["one two"]);
    assert_eq!(rows_of("one two", 3), ["one", " ", "two"]);
}

#[test]
fn a_long_word_breaks_at_the_width() {
    assert_eq!(rows_of("abcdefgh", 3), ["abc", "def", "gh"]);
}

#[test]
fn line_breaks_start_new_rows_and_keep_empty_rows() {
    assert_eq!(rows_of("a\n\nb\n", 10), ["a", "", "b", ""]);
    assert_eq!(rows_of("", 10), [""]);
}

#[test]
fn wide_characters_stay_inside_the_width() {
    assert_eq!(rows_of("日本語テ", 5), ["日本", "語テ"]);
    assert_eq!(rows_of("日本", 1), ["日", "本"], "a wide character needs its own row");
    assert_eq!(rows_of("ação", 2), ["aç", "ão"]);
}

#[test]
fn every_row_joins_back_to_the_text() {
    let text = "SELECT 'ação' FROM t;\n\tdois  espaços  日本語 and a_very_long_word_without_spaces\n";
    for width in 1..40 {
        let joined: String = rows_of(text, width).join("\n");
        let plain: String = joined.chars().filter(|c| *c != '\n').collect();
        let expected: String = text.chars().filter(|c| *c != '\n').collect();
        assert_eq!(plain, expected, "{width}");
    }
}

#[test]
fn a_new_width_reads_the_rows_again_and_the_last_row_limits_the_view() {
    let mut wrapped = WrappedText::new("abcdefgh".to_owned());
    wrapped.set_width(2);
    assert_eq!(wrapped.row(3), Some("gh"));
    assert_eq!(wrapped.row(4), None);
    assert_eq!(wrapped.last_row_at(50), 3);
    wrapped.set_width(4);
    assert_eq!(wrapped.row(1), Some("efgh"));
    assert_eq!(wrapped.last_row_at(50), 1);
    assert_eq!(wrapped.text(), "abcdefgh");
}

#[test]
fn a_row_of_a_large_text_reads_only_that_row() {
    let mut wrapped = WrappedText::new("x".repeat(100_000_000));
    wrapped.set_width(80);
    let started = Instant::now();
    for index in 0..30 {
        assert_eq!(wrapped.row(index).unwrap().len(), 80);
    }
    assert!(started.elapsed() < Duration::from_millis(100), "{:?}", started.elapsed());
}

#[test]
fn a_sink_drops_the_rows_after_the_stop_request() {
    let (events, _inbox) = tokio::sync::mpsc::channel::<ExecEvent>(8);
    let budget = std::sync::Arc::new(Budget::new(1 << 20));
    let key = StatementKey { console: 1, execution: 1, index: 0 };
    let mut sink = ResultSink::new(key, events, budget.clone(), 1000);
    sink.set_columns_blocking(vec!["a".to_owned()]);
    assert_eq!(sink.push_blocking(row(&["kept"])), Flow::Continue);
    let used = budget.used();

    sink.stop_flag().store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(sink.push_blocking(row(&["dropped"])), Flow::Discard);
    assert_eq!(budget.used(), used);
    assert_eq!(sink.finish_blocking().0, 1);
}
