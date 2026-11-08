//! Local calculations use only loaded values and release their memory reservations.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sqln::analysis::{ChartKind, Job, MEMORY_ERROR, STALE_ERROR};
use sqln::grid::{Budget, ResultTable, Value};

fn table(columns: &[&str], rows: &[Vec<Value<'_>>]) -> ResultTable {
    let mut table = ResultTable::new(columns.iter().map(|name| (*name).to_owned()).collect());
    let rows = rows
        .iter()
        .map(|row| row.iter().copied().collect())
        .collect();
    table.append(rows, 0);
    table
}

fn report(
    mut job: Job,
    current: &ResultTable,
    previous: Option<&ResultTable>,
) -> Result<sqln::analysis::Report, &'static str> {
    for _ in 0..100_000 {
        if job.step(current, previous, Instant::now() + Duration::from_millis(5))? {
            return Ok(job.finish());
        }
    }
    panic!("analysis did not finish");
}

#[test]
fn profile_counts_nulls_and_distinct_decimal_values_without_float_rounding() {
    use Value::{Null, Number};
    let table = table(
        &["value"],
        &[
            vec![Null],
            vec![Number("10.50")],
            vec![Number("1.05e1")],
            vec![Number("9007199254740993")],
            vec![Number("9007199254740992")],
            vec![Number("-20")],
        ],
    );
    let budget = Arc::new(Budget::new(1024 * 1024));
    let result = report(Job::profile(&table, 0, true, &budget), &table, None).unwrap();
    assert!(result.text.contains("NULL: 1 (16.7%)"), "{}", result.text);
    assert!(result.text.contains("Distinct (without NULL): 4"));
    assert!(result.text.contains("Min: -20"));
    assert!(result.text.contains("Max: 9007199254740993"));
    assert!(result.text.contains("Histogram:"));
    assert!(budget.used() > 0);
    drop(result);
    assert_eq!(budget.used(), 0);
}

#[test]
fn empty_text_binary_and_all_null_profiles_need_no_numeric_range() {
    for rows in [
        vec![],
        vec![vec![Value::Null]],
        vec![vec![Value::Text("é界\x1b[2J")]],
        vec![vec![Value::binary(&[0, 255])]],
    ] {
        let table = table(&["value"], &rows);
        let budget = Arc::new(Budget::new(65536));
        let result = report(Job::profile(&table, 0, false, &budget), &table, None).unwrap();
        assert!(result.text.contains("incomplete result"));
        assert!(!result.text.contains("Min:"));
        assert!(!result.text.contains("Histogram:"));
    }
}

#[test]
fn numeric_min_max_compare_negative_fractions_exactly() {
    let table = table(
        &["n"],
        &[
            vec![Value::Number("-0.001")],
            vec![Value::Number("-1.01")],
            vec![Value::Number("-1.1")],
            vec![Value::Number("-0")],
        ],
    );
    let budget = Arc::new(Budget::new(65536));
    let result = report(Job::profile(&table, 0, true, &budget), &table, None).unwrap();
    assert!(result.text.contains("Min: -1.1"));
    assert!(result.text.contains("Max: -0"));
}

#[test]
fn constant_columns_have_one_histogram_bucket_and_out_of_range_values_are_labeled() {
    let table = table(
        &["n"],
        &[
            vec![Value::Number("3")],
            vec![Value::Number("3")],
            vec![Value::Number("NaN")],
            vec![Value::Number("1e400")],
        ],
    );
    let budget = Arc::new(Budget::new(65536));
    let result = report(Job::profile(&table, 0, true, &budget), &table, None).unwrap();
    assert!(
        result.text.contains("3.0000e0: #################### 2"),
        "{}",
        result.text
    );
    assert!(result.text.contains("excludes 2"));
    assert!(result.text.contains("Max: 1e400"));
}

#[test]
fn both_charts_keep_order_and_every_repeated_x_value() {
    use Value::{Null, Number};
    let table = table(
        &["x", "y"],
        &[
            vec![Number("3"), Number("-2")],
            vec![Number("1"), Number("4")],
            vec![Number("3"), Number("7")],
            vec![Null, Number("8")],
            vec![Number("5"), Number("NaN")],
        ],
    );
    for kind in [ChartKind::Bar, ChartKind::Line] {
        let budget = Arc::new(Budget::new(65536));
        let result = report(Job::chart(&table, 0, 1, kind, false, &budget), &table, None).unwrap();
        let first = result.text.find("X=3 Y=-2").unwrap();
        let second = result.text.find("X=1 Y=4").unwrap();
        let third = result.text.find("X=3 Y=7").unwrap();
        assert!(first < second && second < third);
        assert!(
            result
                .text
                .contains("Points: 3 / 5 loaded rows; skipped: 2")
        );
        assert!(result.text.contains("incomplete result"));
        assert!(result.text.contains("approximate numbers"));
        drop(result);
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn line_sections_keep_all_points_instead_of_sampling() {
    let rows: Vec<_> = (0..101)
        .map(|_| vec![Value::Number("1"), Value::Number("2")])
        .collect();
    let table = table(&["x", "y"], &rows);
    let budget = Arc::new(Budget::new(65536));
    let result = report(
        Job::chart(&table, 0, 1, ChartKind::Line, true, &budget),
        &table,
        None,
    )
    .unwrap();
    assert_eq!(result.text.matches("X=1 Y=2").count(), 101);
    assert_eq!(result.text.matches("Line: points").count(), 3);
}

#[test]
fn charts_refuse_empty_and_non_numeric_pairs() {
    for rows in [vec![], vec![vec![Value::Text("1"), Value::Number("2")]]] {
        let table = table(&["x", "y"], &rows);
        let budget = Arc::new(Budget::new(65536));
        let error = report(
            Job::chart(&table, 0, 1, ChartKind::Bar, true, &budget),
            &table,
            None,
        )
        .err()
        .unwrap();
        assert!(error.contains("no finite numeric pairs"));
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn differences_match_keys_not_row_positions_and_normalize_numbers() {
    use Value::{Number, Text};
    let old = table(
        &["id", "value"],
        &[
            vec![Number("1"), Number("10.50")],
            vec![Number("2"), Text("before")],
            vec![Number("3"), Text("removed")],
        ],
    );
    let current = table(
        &["id", "value"],
        &[
            vec![Number("2"), Text("after")],
            vec![Number("1.0"), Number("10.5")],
            vec![Number("4"), Text("added")],
        ],
    );
    let budget = Arc::new(Budget::new(65536));
    let job = Job::diff(&current, &old, vec![0], &budget).unwrap();
    let result = report(job, &current, Some(&old)).unwrap();
    assert!(
        result.text.contains("value: before -> after"),
        "{}",
        result.text
    );
    assert!(
        result
            .text
            .contains("Summary: 1 added, 1 removed, 1 changed")
    );
    drop(result);
    assert_eq!(budget.used(), 0);
}

#[test]
fn composite_keys_and_nulls_follow_existing_compare_rules() {
    use Value::{Null, Number, Text};
    let old = table(
        &["id", "part", "v"],
        &[
            vec![Number("1"), Text("a"), Null],
            vec![Number("1"), Text("b"), Text("x")],
        ],
    );
    let current = table(
        &["id", "part", "v"],
        &[
            vec![Number("1"), Text("b"), Text("x")],
            vec![Number("1"), Text("a"), Null],
        ],
    );
    let budget = Arc::new(Budget::new(65536));
    let result = report(
        Job::diff(&current, &old, vec![0, 1], &budget).unwrap(),
        &current,
        Some(&old),
    )
    .unwrap();
    assert!(result.text.contains("0 added, 0 removed, 0 changed"));
}

#[test]
fn duplicate_keys_on_either_side_cancel_without_a_partial_report() {
    let duplicate = table(
        &["id"],
        &[vec![Value::Number("1")], vec![Value::Number("1.0")]],
    );
    let unique = table(&["id"], &[vec![Value::Number("1")]]);
    for (old, current) in [(&duplicate, &unique), (&unique, &duplicate)] {
        let budget = Arc::new(Budget::new(65536));
        let error = report(
            Job::diff(current, old, vec![0], &budget).unwrap(),
            current,
            Some(old),
        )
        .err()
        .unwrap();
        assert!(error.contains("duplicate key"));
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn incompatible_columns_and_missing_keys_refuse_comparison() {
    let old = table(&["id"], &[]);
    let current = table(&["other"], &[]);
    let budget = Arc::new(Budget::new(65536));
    assert!(Job::diff(&current, &old, vec![0], &budget).is_err());
    assert!(Job::diff(&old, &old, vec![], &budget).is_err());
}

#[test]
fn stale_jobs_and_memory_failures_release_all_reservations() {
    let mut table = table(&["x", "y"], &[vec![Value::Number("1"), Value::Number("2")]]);
    let budget = Arc::new(Budget::new(65536));
    let mut stale = Job::profile(&table, 0, true, &budget);
    table.invalidate();
    assert_eq!(
        stale.step(&table, None, Instant::now()).unwrap_err(),
        STALE_ERROR
    );
    for kind in [ChartKind::Bar, ChartKind::Line] {
        let budget = Arc::new(Budget::new(1));
        let error = report(Job::chart(&table, 0, 1, kind, true, &budget), &table, None)
            .err()
            .unwrap();
        assert_eq!(error, MEMORY_ERROR);
        assert_eq!(budget.used(), 0);
    }
    let budget = Arc::new(Budget::new(1));
    assert_eq!(
        report(Job::profile(&table, 0, true, &budget), &table, None)
            .err()
            .unwrap(),
        MEMORY_ERROR
    );
    assert_eq!(budget.used(), 0);
}

#[test]
fn large_profile_values_advance_without_copying_the_complete_value() {
    let text = "x".repeat(1024 * 1024);
    let table = table(&["value"], &[vec![Value::Text(&text)]]);
    let budget = Arc::new(Budget::new(65536));
    let mut job = Job::profile(&table, 0, true, &budget);
    for _ in 0..10 {
        assert!(!job.step(&table, None, Instant::now()).unwrap());
    }
    assert!(budget.used() < 4096);
    drop(job);
    assert_eq!(budget.used(), 0);
}

#[test]
fn profile_compares_large_duplicates_in_chunks_and_keeps_value_equality() {
    let text = "界é".repeat(16384);
    let different = format!("{text}x");
    let table = table(
        &["value"],
        &[
            vec![Value::Text(&text)],
            vec![Value::binary(text.as_bytes())],
            vec![Value::Text(&different)],
            vec![Value::Number("1.00")],
            vec![Value::Text("1")],
            vec![Value::binary(b"1")],
            vec![Value::Null],
        ],
    );
    let budget = Arc::new(Budget::new(65536));
    let mut job = Job::profile(&table, 0, true, &budget);
    let mut steps = 0;
    while !job.step(&table, None, Instant::now()).unwrap() {
        steps += 1;
        assert!(steps < 10000);
    }
    assert!(steps > 3 * text.len().div_ceil(4096));
    let result = job.finish();
    assert!(
        result.text.contains("Distinct (without NULL): 3"),
        "{}",
        result.text
    );
    drop(result);
    assert_eq!(budget.used(), 0);
}

#[test]
fn large_difference_values_advance_in_budgeted_chunks() {
    let bytes = vec![255; 1024 * 1024];
    let previous = table(&["id", "value"], &[]);
    let current = table(
        &["id", "value"],
        &[vec![Value::Number("1"), Value::binary(&bytes)]],
    );
    let budget = Arc::new(Budget::new(65536));
    let mut job = Job::diff(&current, &previous, vec![0], &budget).unwrap();
    for _ in 0..10 {
        assert!(!job.step(&current, Some(&previous), Instant::now()).unwrap());
    }
    assert!(budget.used() < budget.limit());
    assert_eq!(
        report(job, &current, Some(&previous)).err().unwrap(),
        MEMORY_ERROR
    );
    assert_eq!(budget.used(), 0);
}

#[test]
fn difference_chunks_preserve_unicode_controls_binary_and_column_names() {
    let name = "界é".repeat(1000);
    let text = format!("{}tail", "é界\t\u{1b}\r\n".repeat(1000));
    let bytes = [0, 255].repeat(5000);
    let previous = table(
        &["id", &name, "bytes"],
        &[vec![Value::Number("1"), Value::Null, Value::binary(&[])]],
    );
    let current = table(
        &["id", &name, "bytes"],
        &[vec![
            Value::Number("1"),
            Value::Text(&text),
            Value::binary(&bytes),
        ]],
    );
    let budget = Arc::new(Budget::new(256 * 1024));
    let result = report(
        Job::diff(&current, &previous, vec![0], &budget).unwrap(),
        &current,
        Some(&previous),
    )
    .unwrap();
    assert!(result.text.contains(&format!(
        "  {name}: NULL -> {}\n",
        sqln::grid::printable(&text)
    )));
    assert!(
        result
            .text
            .contains(&format!("  bytes:  -> 0x{}\n", "00FF".repeat(5000)))
    );
    assert!(
        result
            .text
            .contains("Summary: 0 added, 0 removed, 1 changed")
    );
    drop(result);
    assert_eq!(budget.used(), 0);
}

#[test]
fn comparison_memory_failure_cancels_the_job() {
    let table = table(&["id"], &[vec![Value::Number("1")]]);
    let budget = Arc::new(Budget::new(100));
    let job = Job::diff(&table, &table, vec![0], &budget).unwrap();
    assert_eq!(
        report(job, &table, Some(&table)).err().unwrap(),
        MEMORY_ERROR
    );
    assert_eq!(budget.used(), 0);
}

#[test]
fn dropping_an_unfinished_job_releases_its_scratch_memory() {
    let table = table(&["value"], &vec![vec![Value::Text("hello")]; 1000]);
    let budget = Arc::new(Budget::new(65536));
    let mut job = Job::profile(&table, 0, true, &budget);
    assert!(!job.step(&table, None, Instant::now()).unwrap());
    assert!(budget.used() > 0);
    drop(job);
    assert_eq!(budget.used(), 0);
}

#[test]
fn retained_rows_keep_one_budget_charge_across_credential_preparation_and_replacement() {
    use sqln::clipboard::MemoryClipboard;
    use sqln::console::Console;
    use sqln::editor::{KeyboardMode, RunPlan, SqlEditor};
    use sqln::exec::{ExecEvent, ExecutionEnd, ReadLimit, StatementKey};
    use sqln::grid::{Reservation, Row};
    use sqln::sql::Dialect;
    use std::{cell::RefCell, rc::Rc};

    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let editor = SqlEditor::new(
        "SELECT 1",
        KeyboardMode::Conventional,
        Dialect::Sqlite,
        clipboard,
    );
    let mut console = Console::new(1, "test".into(), editor);
    let budget = Budget::new(65536);
    let plan = RunPlan {
        statements: vec!["SELECT 1".into()],
        rows: vec![],
        starts: vec![],
    };
    let deliver = |console: &mut Console, execution| {
        let key = StatementKey {
            console: 1,
            execution,
            index: 0,
        };
        let row: Row = [Value::Number("1")].into_iter().collect();
        let Reservation::Accepted(bytes) = budget.reserve(&row) else {
            panic!("row must fit")
        };
        console.on_event(
            ExecEvent::Columns {
                key,
                columns: vec!["value".into()],
            },
            &budget,
        );
        console.on_event(
            ExecEvent::Rows {
                key,
                rows: vec![row],
                bytes,
            },
            &budget,
        );
        console.on_event(
            ExecEvent::ResultDone {
                key,
                rows: 1,
                limit: ReadLimit::None,
            },
            &budget,
        );
        console.on_event(
            ExecEvent::ExecutionFinished {
                console: 1,
                execution,
                end: ExecutionEnd::Completed,
                message: None,
            },
            &budget,
        );
    };
    console.begin_execution(1, &plan, &budget);
    deliver(&mut console, 1);
    let one_row = budget.used();
    console.retain_result = true;
    console.await_credentials(&plan, &budget);
    let generation = console.previous_result.as_ref().unwrap().generation;
    assert_eq!(budget.used(), one_row);
    console.begin_execution(2, &plan, &budget);
    assert_eq!(
        console.previous_result.as_ref().unwrap().generation,
        generation
    );
    assert_eq!(budget.used(), one_row);
    console.on_event(
        ExecEvent::ExecutionFinished {
            console: 1,
            execution: 1,
            end: ExecutionEnd::Failed,
            message: None,
        },
        &budget,
    );
    assert!(
        console.previous_result.is_some(),
        "stale events do not cancel the current comparison"
    );
    deliver(&mut console, 2);
    assert_eq!(budget.used(), 2 * one_row);
    console.begin_execution(3, &plan, &budget);
    assert_eq!(
        budget.used(),
        one_row,
        "replacement releases the older snapshot"
    );
    console.cancel_comparison("test canceled the run", &budget);
    assert_eq!(budget.used(), 0);
    assert!(console.previous_result.is_none());
    assert_eq!(console.warnings(), 1);
}
