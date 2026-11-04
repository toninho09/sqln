// Tests of the grid data without a screen: row search, column widths, and large values.

use std::time::{Duration, Instant};

use sqln::exec::{ExecEvent, Flow, ResultSink, StatementKey};
use sqln::grid::json::{FORMAT_LIMIT, pretty};
use sqln::grid::{Budget, GridView, Reservation, ResultTable, Row, RowSearch, RowWriter, Value};
use sqln::result::ResultTab;
use sqln::ui::wrap::WrappedText;

fn text(value: &str) -> Value<'_> {
    Value::Text(value)
}

fn row(values: &[&str]) -> Row {
    values.iter().map(|value| text(value)).collect()
}

fn table(rows: &[&[&str]]) -> ResultTable {
    let columns = rows[0]
        .iter()
        .enumerate()
        .map(|(index, _)| format!("c{index}"))
        .collect();
    let mut table = ResultTable::new(columns);
    table.append(rows.iter().map(|values| row(values)).collect(), 0);
    table
}

fn later() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

fn search(
    table: &ResultTable,
    query: &str,
    origin: (usize, usize),
) -> (RowSearch, Option<(usize, usize)>) {
    let mut search = RowSearch::new(origin);
    search.push_str(query);
    let first = search.scan(table, later());
    (search, first)
}

#[test]
fn row_selection_replaces_the_previous_range_and_includes_every_column() {
    let table = table(&[&["a", "b", "c"], &["d", "e", "f"]]);
    let mut view = GridView::default();
    view.select_all(&table);
    view.select_row(1, &table);
    let range = view.selection().unwrap();
    assert_eq!(
        (range.top, range.bottom, range.left, range.right),
        (1, 1, 0, 2)
    );
    assert_eq!(view.copy(&table).as_deref(), Some("d\te\tf\n"));
    let before = view;
    view.select_row(2, &table);
    assert_eq!(view, before);
    view.select_row(0, &ResultTable::new(vec!["a".into()]));
    assert_eq!(view, before);
    view.select_row(0, &ResultTable::new(Vec::new()));
    assert_eq!(view, before);

    let table = self::table(&[&["only"]]);
    view.select_row(0, &table);
    assert!(view.selection().is_some());
    assert_eq!(view.copy(&table).as_deref(), Some("only\n"));
}

#[test]
fn row_number_targets_use_the_visible_rows_and_ignore_headers_and_empty_space() {
    use ratatui::layout::Rect;
    use sqln::ui::{grid_cell_at, grid_row_number_at};

    let mut result = ResultTab::new(1, vec!["a".into(), "b".into()]);
    result
        .table
        .append((0..12).map(|_| row(&["a", "b"])).collect(), 0);
    result.view.top = 10;
    result.view.left = 1;
    let area = Rect::new(7, 3, 40, 8);
    assert_eq!(grid_row_number_at(&result, area, 7, 4), Some(10));
    assert_eq!(grid_row_number_at(&result, area, 8, 5), Some(11));
    assert_eq!(grid_cell_at(&result, area, 12, 4), Some((10, 1)));
    for (column, row) in [(6, 4), (9, 4), (10, 4), (12, 4), (7, 3), (7, 6), (7, 11)] {
        assert_eq!(grid_row_number_at(&result, area, column, row), None);
    }
    for width in 0..=3 {
        assert_eq!(
            grid_row_number_at(&result, Rect { width, ..area }, 7, 4),
            None
        );
    }
    result.table.rows.clear();
    assert_eq!(grid_row_number_at(&result, area, 7, 4), None);
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
    let row: Row = [
        Value::Null,
        Value::Number("12345"),
        Value::binary(&[0xFF, 0xAB]),
    ]
    .into_iter()
    .collect();
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

/// Values at each length where the head of a cell changes size.
#[test]
fn a_row_returns_each_kind_of_value_at_each_length() {
    let bytes = |length: usize| "a".repeat(length);
    for length in [0, 1, 30, 31, 32, 157, 158, 159, 16_414, 16_415, 2_097_182] {
        let text = bytes(length);
        let hex = vec![0xFF; length];
        let values = [
            Value::Text(&text),
            Value::Null,
            Value::Number(&text),
            Value::binary(text.as_bytes()),
            Value::binary(&hex),
            Value::Text("last"),
        ];
        let row: Row = values.into_iter().collect();
        assert_eq!(row.cells().collect::<Vec<_>>(), values, "{length}");
        assert_eq!(row.columns(), values.len(), "{length}");
        assert_eq!(row.get(5), Some(Value::Text("last")), "{length}");
        assert_eq!(row.get(6), None, "{length}");
    }
}

#[test]
fn a_row_keeps_text_of_many_bytes_and_each_value_kind_apart() {
    let row: Row = [
        Value::Text("ação 日本語 🦀"),
        Value::Number("1"),
        Value::Text("NULL"),
        Value::Null,
        Value::Text(""),
        Value::binary(b"text"),
        Value::binary(&[0xFF]),
        Value::binary(&[]),
    ]
    .into_iter()
    .collect();
    assert_eq!(row.get(0).unwrap().display(), "ação 日本語 🦀");
    assert!(matches!(row.get(1), Some(Value::Number("1"))));
    assert_ne!(row.get(2), row.get(3), "a text is not a NULL");
    assert_ne!(row.get(3), row.get(4), "an empty text is not a NULL");
    let binary = |column: usize| match row.get(column) {
        Some(Value::Binary(bytes)) => bytes,
        other => panic!("not binary: {other:?}"),
    };
    assert_eq!(binary(5).text(), Some("text"));
    assert_eq!(binary(6).text(), None);
    assert_eq!(&binary(6)[..], &[0xFF]);
    assert_eq!(binary(7).text(), Some(""), "no bytes are valid text");
}

#[test]
fn a_writer_starts_each_row_empty() {
    let mut writer = RowWriter::default();
    let large = "a".repeat(100_000);
    for text in ["short", &large, "after"] {
        writer.push(Value::Text(text));
        let row = writer.finish();
        assert_eq!(row.cells().collect::<Vec<_>>(), [Value::Text(text)]);
    }
    assert_eq!(writer.finish(), Row::default());
}

#[test]
fn the_budget_counts_the_block_of_a_row_and_finds_a_value_above_the_limit() {
    let row: Row = [text("a"), text("bc")].into_iter().collect();
    assert_eq!(row.bytes(), 5, "a head and its value for each cell");
    let budget = Budget::new(size_of::<Row>() + 4);
    assert_eq!(budget.reserve(&row), Reservation::BudgetReached);
    let budget = Budget::new(1);
    assert_eq!(budget.reserve(&row), Reservation::ValueTooLarge);
}

#[test]
fn the_copy_of_a_row_with_fewer_cells_pads_with_nulls() {
    let mut table = ResultTable::new(vec!["a".into(), "b".into()]);
    let short: Row = vec![text("x")].into_iter().collect();
    table.append(vec![short], 0);
    assert_eq!(table.tsv(0, 0, 0, 1), "x\t\n");
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
        Value::Number("12345"),
        Value::binary(&[0xFF, 0x00, 0xAB, 0x10]),
        Value::binary(b"text"),
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
    let long = "x".repeat(50_000_000);
    let value = text(&long);
    let started = Instant::now();
    for _ in 0..1000 {
        assert_eq!(value.preview(40).len(), 40);
    }
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_sink_that_nobody_finishes_returns_its_rows_to_the_budget() {
    let (events, _inbox) = tokio::sync::mpsc::channel::<ExecEvent>(8);
    let budget = std::sync::Arc::new(Budget::new(1 << 20));
    let key = StatementKey {
        console: 1,
        execution: 1,
        index: 0,
    };
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
    assert_eq!(
        rows_of("日本", 1),
        ["日", "本"],
        "a wide character needs its own row"
    );
    assert_eq!(rows_of("ação", 2), ["aç", "ão"]);
}

#[test]
fn every_row_joins_back_to_the_text() {
    let text =
        "SELECT 'ação' FROM t;\n\tdois  espaços  日本語 and a_very_long_word_without_spaces\n";
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
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_sink_drops_the_rows_after_the_stop_request() {
    let (events, _inbox) = tokio::sync::mpsc::channel::<ExecEvent>(8);
    let budget = std::sync::Arc::new(Budget::new(1 << 20));
    let key = StatementKey {
        console: 1,
        execution: 1,
        index: 0,
    };
    let mut sink = ResultSink::new(key, events, budget.clone(), 1000);
    sink.set_columns_blocking(vec!["a".to_owned()]);
    assert_eq!(sink.push_blocking(row(&["kept"])), Flow::Continue);
    let used = budget.used();

    sink.stop_flag()
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(sink.push_blocking(row(&["dropped"])), Flow::Discard);
    assert_eq!(budget.used(), used);
    assert_eq!(sink.finish_blocking().0, 1);
}

#[test]
fn json_keeps_the_order_of_the_keys_and_each_digit() {
    assert_eq!(
        pretty(r#"{"z":1,"a":12345678901234567890123.50,"z":2}"#).unwrap(),
        "{\n  \"z\": 1,\n  \"a\": 12345678901234567890123.50,\n  \"z\": 2\n}"
    );
}

#[test]
fn json_nests_and_keeps_empty_parts_on_one_row() {
    assert_eq!(
        pretty(r#"[ {"a": [1, 2], "b": { }}, [ ] ]"#).unwrap(),
        "[\n  {\n    \"a\": [\n      1,\n      2\n    ],\n    \"b\": {}\n  },\n  []\n]"
    );
}

#[test]
fn json_keeps_each_string_as_it_is() {
    let text = r#"{"a":"x \" , : { } [ ] é 😀 \\","b":"  "}"#;
    assert_eq!(
        pretty(text).unwrap(),
        "{\n  \"a\": \"x \\\" , : { } [ ] é 😀 \\\\\",\n  \"b\": \"  \"\n}"
    );
}

#[test]
fn json_ignores_the_space_around_the_value() {
    assert_eq!(pretty(" \n[1]\t").unwrap(), "[\n  1\n]");
}

#[test]
fn only_a_valid_object_or_array_is_json_for_the_window() {
    for text in [
        "", "1", "\"a\"", "null", "true", "{", "[1,]", "{\"a\":}", "{} x", "{a:1}",
    ] {
        assert_eq!(pretty(text), None, "{text}");
    }
}

#[test]
fn json_above_the_limit_keeps_its_text() {
    let fits = format!("[\"{}\"]", "x".repeat(FORMAT_LIMIT - 4));
    assert!(pretty(&fits).is_some());
    let long = format!("[\"{}\"]", "x".repeat(FORMAT_LIMIT - 3));
    assert_eq!(pretty(&long), None);
}

#[test]
fn json_with_a_very_long_form_stays_as_text_and_ends_fast() {
    let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    let wide = format!("{}{}{}", "[".repeat(2000), "1,".repeat(400_000), "1");
    let wide = wide + &"]".repeat(2000);
    let start = Instant::now();
    assert_eq!(pretty(&deep), None);
    assert_eq!(pretty(&wide), None);
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn json_of_a_normal_depth_opens_formatted() {
    let text = format!("{}1{}", "[".repeat(64), "]".repeat(64));
    let formatted = pretty(&text).unwrap();
    assert!(formatted.contains(&format!("\n{}1\n", "  ".repeat(64))));
}

fn result_of(columns: &[&str], rows: Vec<Row>) -> ResultTab {
    let names = columns.iter().map(|name| (*name).to_owned()).collect();
    let mut tab = ResultTab::new(1, names);
    tab.table.append(rows, 0);
    tab
}

#[test]
fn the_row_window_writes_one_aligned_line_for_each_column() {
    let values: Row = vec![
        Value::Number("1"),
        Value::Null,
        Value::binary(&[0, 255]),
        text("first\nsecond"),
    ]
    .into_iter()
    .collect();
    let tab = result_of(&["id", "deleted_at", "hash", "note"], vec![values]);
    let window = tab.row_window(0).unwrap();
    assert_eq!(
        window.text,
        "id         : 1\ndeleted_at : NULL\nhash       : 0x00FF\nnote       : first↵second"
    );
    assert_eq!(window.title, " row 1 of 1 loaded ");
    assert_eq!(window.footer, " 4 columns ");
    assert_eq!(window.indent, 13);
    assert_eq!(tab.row_window(1), None);
}

#[test]
fn the_row_window_marks_a_cut_value_and_a_cut_name() {
    let exact = "x".repeat(4096);
    let long = "y".repeat(5000);
    let tab = result_of(&["a", &"n".repeat(40)], vec![row(&[&exact, &long])]);
    let window = tab.row_window(0).unwrap();
    let lines: Vec<&str> = window.text.lines().collect();
    assert!(lines[0].ends_with(&format!(" : {exact}")), "an exact value");
    assert!(lines[1].starts_with(&format!("{} : y", "n".repeat(32))));
    assert!(lines[1].ends_with(&format!("{}…", "y".repeat(4096))));
    assert!(!lines[1].contains(&"y".repeat(4097)));
}

fn indented_rows(text: &str, indent: usize, width: usize) -> Vec<(usize, String)> {
    let mut wrapped = WrappedText::with_indent(text.to_owned(), indent);
    wrapped.set_width(width);
    let mut rows = Vec::new();
    while let Some(row) = wrapped.row(rows.len()) {
        let row = row.to_owned();
        rows.push((wrapped.row_indent(rows.len()), row));
    }
    rows
}

#[test]
fn a_wrapped_row_keeps_the_columns_of_its_indent() {
    assert_eq!(
        indented_rows("name : one two three four\nid   : 1", 7, 16),
        [
            (0, "name : one two ".to_owned()),
            (7, "three ".to_owned()),
            (7, "four".to_owned()),
            (0, "id   : 1".to_owned()),
        ]
    );
}

#[test]
fn a_narrow_window_gives_the_indent_to_the_text() {
    let rows = indented_rows("name : one two three", 7, 10);
    assert!(rows.iter().all(|(indent, _)| *indent == 0), "{rows:?}");
    assert_eq!(
        rows.iter().map(|(_, row)| row.as_str()).collect::<String>(),
        "name : one two three"
    );
}
