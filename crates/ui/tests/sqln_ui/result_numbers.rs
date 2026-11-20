//! Result tabs named by the place of their statement in the editor text.

use std::{cell::RefCell, rc::Rc};

use crossterm::event::KeyCode;
use edtui::Index2;
use sqln_core::clipboard::MemoryClipboard;
use sqln_core::grid::{Budget, Reservation, Row, Value};
use sqln_exec::exec::{ExecEvent, ExecutionEnd, ReadLimit, StatementKey, StatementStatus};
use sqln_session::connector::StatementEnd;
use sqln_sql::sql::Dialect;
use sqln_testkit::{Fixture, shift};
use sqln_ui::console::Console;
use sqln_ui::editor::{KeyboardMode, RunPlan, RunScope, SqlEditor};

const SCRIPT: &str = "SELECT 1;\nSELECT 2;\nSELECT 3;";

fn console() -> Console {
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let editor = SqlEditor::new(
        SCRIPT,
        KeyboardMode::Conventional,
        Dialect::Sqlite,
        clipboard,
    );
    Console::new(1, "test".into(), editor)
}

/// A plan of statements at these places of the text.
fn plan(ordinals: &[usize]) -> RunPlan {
    RunPlan {
        statements: ordinals
            .iter()
            .map(|ordinal| format!("SELECT {}", ordinal + 1))
            .collect(),
        ordinals: ordinals.to_vec(),
        ..RunPlan::default()
    }
}

fn key(execution: u64, index: usize) -> StatementKey {
    StatementKey {
        console: 1,
        execution,
        index,
    }
}

/// Delivers one grid of a statement with one row of `value`.
fn grid(console: &mut Console, budget: &Budget, key: StatementKey, column: &str, value: &str) {
    let row: Row = [Value::Text(value)].into_iter().collect();
    let Reservation::Accepted(bytes) = budget.reserve(&row) else {
        panic!("the row must fit")
    };
    let events = [
        ExecEvent::Columns {
            key,
            columns: vec![column.into()],
            live: None,
            progress: None,
        },
        ExecEvent::Rows {
            key,
            rows: vec![row],
            bytes,
        },
        ExecEvent::ResultDone {
            key,
            rows: 1,
            limit: ReadLimit::None,
        },
    ];
    for event in events {
        console.on_event(event, budget);
    }
}

fn finish_statement(
    console: &mut Console,
    budget: &Budget,
    key: StatementKey,
    status: StatementStatus,
) {
    console.on_event(
        ExecEvent::StatementFinished {
            key,
            status,
            end: Some(StatementEnd {
                has_columns: false,
                ..StatementEnd::default()
            }),
            error_offset: None,
        },
        budget,
    );
}

fn finish(console: &mut Console, budget: &Budget, execution: u64, end: ExecutionEnd) {
    console.on_event(
        ExecEvent::ExecutionFinished {
            console: 1,
            execution,
            end,
            message: None,
        },
        budget,
    );
}

/// Runs one statement at a place of the text that returns one grid.
fn run_one(console: &mut Console, budget: &Budget, execution: u64, ordinal: usize, value: &str) {
    console.begin_execution(execution, &plan(&[ordinal]), budget);
    grid(console, budget, key(execution, 0), "value", value);
    finish_statement(console, budget, key(execution, 0), StatementStatus::Success);
    finish(console, budget, execution, ExecutionEnd::Completed);
}

fn labels(console: &Console) -> Vec<String> {
    console
        .results
        .iter()
        .map(|result| result.label())
        .collect()
}

fn first_value(console: &Console, index: usize) -> String {
    match &console.results[index].table.rows[0].cells().next() {
        Some(Value::Text(text)) => text.to_string(),
        other => panic!("unexpected value {other:?}"),
    }
}

#[test]
fn the_plan_numbers_each_statement_by_its_place_in_the_whole_text() {
    let mut f = Fixture::conventional(SCRIPT);
    f.editor.set_cursor(Index2::new(1, 3));
    assert_eq!(f.editor.plan_run(RunScope::Default).unwrap().ordinals, [1]);
    assert_eq!(
        f.editor.plan_run(RunScope::Script).unwrap().ordinals,
        [0, 1, 2]
    );

    // A selection keeps the numbers of the whole text, even when it starts inside a
    // statement.
    f.editor.set_cursor(Index2::new(1, 2));
    f.press_all(&[shift(KeyCode::Down), shift(KeyCode::End)]);
    let plan = f.editor.plan_run(RunScope::Default).unwrap();
    assert_eq!(plan.statements.len(), 2);
    assert_eq!(plan.ordinals, [1, 2]);
}

#[test]
fn a_run_replaces_only_the_tab_of_its_statement_number() {
    let budget = Budget::new(65536);
    let mut console = console();
    run_one(&mut console, &budget, 1, 0, "first");
    run_one(&mut console, &budget, 2, 1, "second");
    assert_eq!(labels(&console), ["#1", "#2"]);
    let two_tabs = budget.used();

    run_one(&mut console, &budget, 3, 0, "again");
    assert_eq!(labels(&console), ["#1", "#2"]);
    assert_eq!(first_value(&console, 0), "again");
    assert_eq!(first_value(&console, 1), "second");
    assert_eq!(
        budget.used(),
        two_tabs,
        "the replaced tab leaves the budget"
    );
    assert_eq!(
        console.active, 0,
        "the last result of the run comes on screen"
    );
}

#[test]
fn tabs_stay_in_number_order_and_the_selected_tab_stays_selected() {
    let budget = Budget::new(65536);
    let mut console = console();
    run_one(&mut console, &budget, 1, 2, "third");
    run_one(&mut console, &budget, 2, 0, "first");
    assert_eq!(labels(&console), ["#1", "#3"]);

    // While a run makes a tab before the selected one, the same tab stays selected.
    console.active = 1;
    console.begin_execution(3, &plan(&[1]), &budget);
    assert_eq!(console.active, 1, "accepting a run keeps the tab on screen");
    grid(&mut console, &budget, key(3, 0), "value", "second");
    assert_eq!(labels(&console), ["#1", "#2", "#3"]);
    assert_eq!(console.active, 1, "the first grid of a run comes on screen");
}

#[test]
fn several_grids_of_one_statement_are_replaced_together() {
    let budget = Budget::new(65536);
    let mut console = console();
    console.begin_execution(1, &plan(&[2]), &budget);
    grid(&mut console, &budget, key(1, 0), "a", "one");
    grid(&mut console, &budget, key(1, 0), "b", "two");
    finish_statement(&mut console, &budget, key(1, 0), StatementStatus::Success);
    finish(&mut console, &budget, 1, ExecutionEnd::Completed);
    assert_eq!(labels(&console), ["#3", "#3.2"]);

    run_one(&mut console, &budget, 2, 2, "alone");
    assert_eq!(labels(&console), ["#3"]);
    assert_eq!(first_value(&console, 0), "alone");
}

#[test]
fn a_statement_without_a_grid_removes_its_old_tab_and_a_skipped_one_keeps_it() {
    for status in [
        StatementStatus::Success,
        StatementStatus::Error("no table".into()),
        StatementStatus::Timeout,
        StatementStatus::Stopped,
        StatementStatus::Unknown,
    ] {
        let budget = Budget::new(65536);
        let mut console = console();
        run_one(&mut console, &budget, 1, 0, "first");
        run_one(&mut console, &budget, 2, 1, "second");
        console.begin_execution(3, &plan(&[0, 1]), &budget);
        finish_statement(&mut console, &budget, key(3, 0), status.clone());
        finish_statement(
            &mut console,
            &budget,
            key(3, 1),
            StatementStatus::NotExecuted,
        );
        finish(&mut console, &budget, 3, ExecutionEnd::Failed);
        assert_eq!(labels(&console), ["#2"], "{status:?}");
        assert_eq!(
            console.active,
            console.results.len(),
            "{status:?} shows Messages"
        );
    }
}

#[test]
fn events_of_an_old_execution_make_no_tab() {
    let budget = Budget::new(65536);
    let mut console = console();
    run_one(&mut console, &budget, 1, 0, "first");
    console.begin_execution(2, &plan(&[1]), &budget);
    console.on_event(
        ExecEvent::Columns {
            key: key(1, 0),
            columns: vec!["late".into()],
            live: None,
            progress: None,
        },
        &budget,
    );
    finish_statement(&mut console, &budget, key(1, 0), StatementStatus::Success);
    assert_eq!(labels(&console), ["#1"]);
    assert_eq!(first_value(&console, 0), "first");
}

#[test]
fn each_tab_keeps_the_statement_schema_and_parameters_of_its_run() {
    let budget = Budget::new(65536);
    let mut console = console();
    console.schema = Some("sales".into());
    run_one(&mut console, &budget, 1, 0, "first");
    console.schema = Some("hr".into());
    console.begin_execution(2, &plan(&[1, 2]), &budget);
    grid(&mut console, &budget, key(2, 0), "value", "second");
    let first = &console.results[0];
    assert_eq!(console.statement_of(first), Some("SELECT 1"));
    assert_eq!(first.source.schema.as_deref(), Some("sales"));
    assert!(first.source.alone);
    let second = &console.results[1];
    assert_eq!(console.statement_of(second), Some("SELECT 2"));
    assert_eq!(second.source.schema.as_deref(), Some("hr"));
    assert!(!second.source.alone);
}

#[test]
fn a_full_budget_blocks_a_run_unless_the_replaced_tabs_free_it() {
    let budget = Budget::new(64);
    let mut console = console();
    run_one(
        &mut console,
        &budget,
        1,
        0,
        "a value that fills the small budget",
    );
    assert!(budget.reserve_bytes(budget.limit() - budget.used()));
    assert!(console.run_blocked(&plan(&[1]), false, &budget));
    assert!(!console.run_blocked(&plan(&[0]), false, &budget));
    assert!(
        console.run_blocked(&plan(&[0]), true, &budget),
        "a plan does not replace the data tab"
    );
}
