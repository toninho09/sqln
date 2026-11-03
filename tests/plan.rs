// The plan of a statement: the text of `EXPLAIN`, the readers of each database, the tree,
// and the keys of a console. The texts come from PostgreSQL 18 and MySQL 8.4, and
// `tests/servers.rs` reads the same plans from real servers.

mod common;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use common::{ctrl, key, shift, temp_db};
use crossterm::event::{Event, KeyCode};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::connection::Definition;
use sqln::connector::sqlite::SqliteTarget;
use sqln::grid::{Budget, ResultTable, Row, Value};
use sqln::plan::{Explain, PlanView, plan_view};
use sqln::result::ResultTab;
use sqln::sql::Dialect;
use sqln::ui::text;
use sqln::ui::theme::Theme;

const POSTGRES_ESTIMATE: &str = r#"[{"Plan": {
    "Node Type": "Hash Join", "Join Type": "Left", "Total Cost": 2.14, "Plan Rows": 3,
    "Hash Cond": "(o.user_id = u.id)",
    "Plans": [
        {"Node Type": "Seq Scan", "Relation Name": "orders", "Alias": "o",
         "Total Cost": 1.05, "Plan Rows": 4, "Filter": "(total > 5)"},
        {"Node Type": "Hash", "Total Cost": 1.03, "Plan Rows": 3, "Plans": [
            {"Node Type": "Index Scan", "Relation Name": "users",
             "Index Name": "users_pkey", "Total Cost": 1.03, "Plan Rows": 3,
             "Index Cond": "(id > 0)"}
        ]}
    ]
}}]"#;

const POSTGRES_ANALYZE: &str = r#"[{"Plan": {
    "Node Type": "Limit", "Total Cost": 2.22, "Plan Rows": 2,
    "Actual Total Time": 0.074, "Actual Rows": 2.00, "Actual Loops": 1,
    "Plans": [{
        "Node Type": "Sort", "Total Cost": 2.22, "Plan Rows": 3,
        "Actual Total Time": 0.070, "Actual Rows": 2.00, "Actual Loops": 1,
        "Sort Key": ["(sum(o.total)) DESC"],
        "Plans": [{
            "Node Type": "Seq Scan", "Relation Name": "orders", "Total Cost": 1.05,
            "Plan Rows": 4, "Actual Total Time": 0.025, "Actual Rows": 1.50,
            "Actual Loops": 2
        }]
    }]
}, "Planning Time": 0.725, "Triggers": [], "Execution Time": 0.155}]"#;

const MYSQL_ESTIMATE: &str = "\
-> Limit: 2 row(s)
    -> Nested loop inner join  (cost=1.12 rows=1.33)
        -> Filter: ((o.total > 5) and (o.user_id is not null))  (cost=0.65 rows=1.33)
            -> Table scan on o  (cost=0.65 rows=4)
        -> Single-row index lookup on u using PRIMARY (id=o.user_id)  (cost=0.325 rows=1)
";

const MYSQL_ANALYZE: &str = "\
-> Limit: 2 row(s)  (actual time=0.346..0.346 rows=2 loops=1)
    -> Nested loop inner join  (cost=1.12 rows=1.33) (actual time=0.093..0.108 rows=4 loops=1)
        -> Table scan on o  (cost=0.65 rows=4) (actual time=0.031..0.0356 rows=4 loops=1)
        -> Single-row index lookup on u  (cost=0.325 rows=1) (actual time=0.00385..0.00401 rows=1 loops=4)
";

fn raw(cell: &str) -> ResultTable {
    let mut table = ResultTable::new(vec!["QUERY PLAN".to_owned()]);
    let row: Row = vec![Value::Text(cell)].into_iter().collect();
    table.append(vec![row], cell.len());
    table
}

/// Returns the values of a column of the grid of a plan.
fn cells(view: &PlanView, title: &str) -> Vec<String> {
    let column = view.columns.iter().position(|item| item == title).unwrap();
    view.rows
        .iter()
        .map(|row| row.get(column).unwrap().full_text())
        .collect()
}

fn view(dialect: Dialect, text: &str) -> PlanView {
    plan_view(dialect, &raw(text)).unwrap()
}

// ---------------------------------------------------------------------------
// The text of `EXPLAIN`.

#[test]
fn each_database_receives_its_own_explain() {
    let wrap = |dialect, analyze| Explain::new(dialect, analyze).unwrap().wrap("SELECT 1");
    assert_eq!(
        wrap(Dialect::Postgres, false),
        "EXPLAIN (FORMAT JSON) SELECT 1"
    );
    assert_eq!(
        wrap(Dialect::Postgres, true),
        "EXPLAIN (ANALYZE, FORMAT JSON) SELECT 1"
    );
    assert_eq!(wrap(Dialect::MySql, false), "EXPLAIN FORMAT=TREE SELECT 1");
    assert_eq!(wrap(Dialect::MySql, true), "EXPLAIN ANALYZE SELECT 1");
    assert_eq!(wrap(Dialect::Sqlite, false), "EXPLAIN QUERY PLAN SELECT 1");
}

#[test]
fn a_source_without_the_plan_has_no_explain() {
    assert!(Explain::new(Dialect::Sqlite, true).is_none());
    assert!(Explain::new(Dialect::Redis, false).is_none());
    assert!(Explain::new(Dialect::Redis, true).is_none());
}

#[test]
fn the_prefix_length_is_the_text_before_the_statement() {
    let explain = Explain::new(Dialect::Postgres, true).unwrap();
    let wrapped = explain.wrap("SELECT 1");
    assert_eq!(&wrapped[explain.prefix_len()..], "SELECT 1");
}

// ---------------------------------------------------------------------------
// The readers.

#[test]
fn a_postgres_plan_becomes_a_tree() {
    let view = view(Dialect::Postgres, POSTGRES_ESTIMATE);
    assert_eq!(
        cells(&view, text::PLAN_NODE),
        [
            "Hash Join (Left)",
            "├─ Seq Scan",
            "└─ Hash",
            "   └─ Index Scan",
        ]
    );
    assert_eq!(
        cells(&view, text::PLAN_OBJECT),
        ["", "orders", "", "users (users_pkey)"]
    );
    assert_eq!(
        cells(&view, text::PLAN_DETAIL),
        [
            "Hash Cond: (o.user_id = u.id)",
            "Filter: (total > 5)",
            "",
            "Index Cond: (id > 0)",
        ]
    );
    assert_eq!(
        cells(&view, text::PLAN_COST),
        ["2.14", "1.05", "1.03", "1.03"]
    );
    assert_eq!(cells(&view, text::PLAN_ROWS), ["3", "4", "3", "3"]);
}

#[test]
fn a_plan_without_a_real_run_has_no_columns_of_a_real_run() {
    let view = view(Dialect::Postgres, POSTGRES_ESTIMATE);
    for title in [text::PLAN_TIME, text::PLAN_ACTUAL_ROWS, text::PLAN_LOOPS] {
        assert!(!view.columns.iter().any(|item| item == title), "{title}");
    }
}

#[test]
fn the_share_of_an_estimate_uses_the_cost_of_the_node_alone() {
    let view = view(Dialect::Postgres, POSTGRES_ESTIMATE);
    let shares = cells(&view, text::PLAN_SHARE);
    // The costs alone are 0.06, 1.05, 0, and 1.03, so the total is 2.14.
    assert_eq!(shares[0], "░░░░░░░░░░ 3%");
    assert_eq!(shares[1], "█████░░░░░ 49%");
    assert_eq!(shares[2], "░░░░░░░░░░ 0%");
    assert_eq!(shares[3], "█████░░░░░ 48%");
}

#[test]
fn a_real_run_of_postgres_multiplies_the_time_of_a_loop() {
    let view = view(Dialect::Postgres, POSTGRES_ANALYZE);
    assert_eq!(
        cells(&view, text::PLAN_TIME),
        ["0.074", "0.07", "0.05"],
        "the scan runs two loops of 0.025 ms"
    );
    assert_eq!(cells(&view, text::PLAN_ACTUAL_ROWS), ["2", "2", "1.5"]);
    assert_eq!(cells(&view, text::PLAN_LOOPS), ["1", "1", "2"]);
}

#[test]
fn a_real_run_uses_the_time_for_the_share_and_the_summary() {
    let view = view(Dialect::Postgres, POSTGRES_ANALYZE);
    let shares = cells(&view, text::PLAN_SHARE);
    assert_eq!(shares[0], "█░░░░░░░░░ 5%");
    assert_eq!(shares[1], "███░░░░░░░ 27%");
    assert_eq!(shares[2], "███████░░░ 68%");
    assert_eq!(
        view.summary,
        "3 nodes · cost 2.22 · 0.155 ms · most: Seq Scan 68%"
    );
}

#[test]
fn a_mysql_plan_becomes_a_tree() {
    let view = view(Dialect::MySql, MYSQL_ESTIMATE);
    assert_eq!(
        cells(&view, text::PLAN_NODE),
        [
            "Limit: 2 row(s)",
            "└─ Nested loop inner join",
            "   ├─ Filter: ((o.total > 5) and (o.user_id is not null))",
            "   │  └─ Table scan on o",
            "   └─ Single-row index lookup on u using PRIMARY (id=o.user_id)",
        ]
    );
    assert_eq!(
        cells(&view, text::PLAN_COST),
        ["", "1.12", "0.65", "0.65", "0.325"]
    );
    assert_eq!(
        cells(&view, text::PLAN_ROWS),
        ["", "1.33", "1.33", "4", "1"]
    );
}

#[test]
fn a_node_without_a_cost_has_no_share() {
    let view = view(Dialect::MySql, MYSQL_ESTIMATE);
    let shares = cells(&view, text::PLAN_SHARE);
    assert_eq!(shares[0], "");
    assert_eq!(shares[2], "░░░░░░░░░░ 0%", "its child holds all the cost");
    assert_eq!(shares[3], "██████░░░░ 58%");
}

#[test]
fn a_real_run_of_mysql_reads_each_field() {
    let view = view(Dialect::MySql, MYSQL_ANALYZE);
    assert_eq!(
        cells(&view, text::PLAN_NODE)[1],
        "└─ Nested loop inner join",
        "the numbers leave the name"
    );
    assert_eq!(
        cells(&view, text::PLAN_TIME),
        ["0.346", "0.108", "0.036", "0.016"]
    );
    assert_eq!(cells(&view, text::PLAN_ACTUAL_ROWS), ["2", "4", "4", "1"]);
    assert_eq!(cells(&view, text::PLAN_LOOPS), ["1", "1", "1", "4"]);
    assert_eq!(
        view.summary,
        "4 nodes · cost 1.12 · 0.346 ms · most: Limit: 2 row(s) 69%"
    );
}

#[test]
fn a_node_that_never_ran_keeps_a_clean_name() {
    let view = view(
        Dialect::MySql,
        "-> Filter: (a > 1)  (cost=1 rows=2) (never executed)\n",
    );
    assert_eq!(cells(&view, text::PLAN_NODE), ["Filter: (a > 1)"]);
    assert_eq!(cells(&view, text::PLAN_COST), ["1"]);
}

#[test]
fn a_line_break_in_a_condition_stays_in_its_node() {
    let view = view(
        Dialect::MySql,
        "-> Filter: (a = 'x\ny')  (cost=1 rows=2)\n    -> Table scan on t  (cost=1 rows=2)\n",
    );
    assert_eq!(
        cells(&view, text::PLAN_NODE),
        ["Filter: (a = 'x y')", "└─ Table scan on t"]
    );
    assert_eq!(cells(&view, text::PLAN_COST), ["1", "1"]);
}

#[test]
fn a_condition_that_looks_like_a_cost_stays_in_the_name() {
    let view = view(Dialect::MySql, "-> Filter: (a = '(cost=9 rows=9) b')\n");
    assert_eq!(
        cells(&view, text::PLAN_NODE),
        ["Filter: (a = '(cost=9 rows=9) b')"]
    );
    assert!(!view.columns.iter().any(|item| item == text::PLAN_COST));
}

#[test]
fn the_answer_of_mysql_for_a_write_shows_as_one_node() {
    let view = view(Dialect::MySql, "-> <not executable by iterator executor>\n");
    assert_eq!(
        cells(&view, text::PLAN_NODE),
        ["<not executable by iterator executor>"]
    );
}

#[test]
fn a_sqlite_plan_becomes_a_tree() {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INT);",
        )
        .unwrap();
    let mut statement = connection
        .prepare(
            "EXPLAIN QUERY PLAN SELECT * FROM users \
             WHERE id IN (SELECT user_id FROM orders) ORDER BY name",
        )
        .unwrap();
    let rows: Vec<(i64, i64, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut table = ResultTable::new(
        ["id", "parent", "notused", "detail"]
            .map(str::to_owned)
            .to_vec(),
    );
    let grid_rows = rows
        .iter()
        .map(|(id, parent, detail)| {
            let (id, parent) = (id.to_string(), parent.to_string());
            let cells = [
                Value::Number(&id),
                Value::Number(&parent),
                Value::Number("0"),
                Value::Text(detail),
            ];
            cells.into_iter().collect::<Row>()
        })
        .collect();
    table.append(grid_rows, 0);

    let view = plan_view(Dialect::Sqlite, &table).unwrap();
    assert_eq!(view.columns, [text::PLAN_NODE]);
    let nodes = cells(&view, text::PLAN_NODE);
    assert!(
        nodes.iter().any(|node| node.contains("LIST SUBQUERY")),
        "{nodes:?}"
    );
    assert!(
        nodes.iter().any(|node| node.starts_with("└─ SCAN orders")),
        "{nodes:?}"
    );
    assert_eq!(view.summary, format!("{} nodes", nodes.len()));
}

#[test]
fn a_text_that_is_not_a_plan_gives_no_view() {
    assert!(plan_view(Dialect::Postgres, &raw("not JSON")).is_none());
    assert!(plan_view(Dialect::Postgres, &raw("[{\"Other\": 1}]")).is_none());
    assert!(plan_view(Dialect::Postgres, &raw("[{\"Plan\": {}}]")).is_none());
    assert!(plan_view(Dialect::MySql, &raw("no arrow")).is_none());
    assert!(plan_view(Dialect::MySql, &raw("")).is_none());
    assert!(plan_view(Dialect::Sqlite, &raw("one cell")).is_none());
    assert!(plan_view(Dialect::Redis, &raw("PONG")).is_none());
    let empty = ResultTable::new(vec!["QUERY PLAN".to_owned()]);
    assert!(plan_view(Dialect::Postgres, &empty).is_none());
    assert!(plan_view(Dialect::Sqlite, &empty).is_none());
}

// ---------------------------------------------------------------------------
// The budget.

/// Builds a result with the text of a plan, with its place in the budget.
fn charged_result(budget: &Budget, text: &str) -> ResultTab {
    let mut result = ResultTab::new(1, vec!["QUERY PLAN".to_owned()]);
    let row: Row = vec![Value::Text(text)].into_iter().collect();
    let bytes = match budget.reserve(&row) {
        sqln::grid::Reservation::Accepted(bytes) => bytes,
        other => panic!("{other:?}"),
    };
    result.table.append(vec![row], bytes);
    result
}

#[test]
fn a_plan_takes_the_place_of_the_text_in_the_budget() {
    let budget = Budget::new(1_000_000);
    let mut result = charged_result(&budget, POSTGRES_ESTIMATE);
    let view = plan_view(Dialect::Postgres, &result.table).unwrap();
    assert!(result.show_plan(view, &budget));
    assert!(result.plan);
    assert_eq!(result.table.rows.len(), 4);
    assert_eq!(budget.used(), result.table.bytes);
    assert!(result.summary.starts_with("4 nodes"));
}

#[test]
fn the_tree_keeps_its_width_past_a_normal_column() {
    let budget = Budget::new(1_000_000);
    let mut result = charged_result(&budget, MYSQL_ESTIMATE);
    let view = plan_view(Dialect::MySql, &result.table).unwrap();
    let longest = "   └─ Single-row index lookup on u using PRIMARY (id=o.user_id)";
    assert!(result.show_plan(view, &budget));
    assert_eq!(result.table.width(0), longest.chars().count());
}

#[test]
fn a_full_budget_keeps_the_text() {
    let budget = Budget::new(10_000);
    let mut result = charged_result(&budget, MYSQL_ESTIMATE);
    let view = plan_view(Dialect::MySql, &result.table).unwrap();
    let used = budget.used();
    budget.set_limit(used);
    assert!(!result.show_plan(view, &budget));
    assert!(!result.plan);
    assert_eq!(result.table.rows.len(), 1);
    assert_eq!(budget.used(), used, "a refused plan keeps no bytes");
}

// ---------------------------------------------------------------------------
// The keys of a console, with a real SQLite file.

struct Run {
    app: App,
    path: PathBuf,
}

impl Run {
    fn with(name: &str, change: impl FnOnce(&mut Definition)) -> Self {
        let path = temp_db(name);
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE t (a INTEGER PRIMARY KEY, b INT);
                 CREATE TABLE u (a INT REFERENCES t (a));
                 INSERT INTO t VALUES (1, 2), (3, 4);",
            )
            .unwrap();
        let mut definition = Definition::sqlite(SqliteTarget::new(&path));
        change(&mut definition);
        let options = Options {
            connections: vec![definition],
            ..Options::default()
        };
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        let app = App::new(options, clipboard, Theme::plain());
        Self { app, path }
    }

    fn new(name: &str) -> Self {
        Self::with(name, |_| {})
    }

    fn write(&mut self, sql: &str) {
        self.app.on_event(Event::Paste(sql.to_owned()));
    }

    fn explain(&mut self) {
        self.app.on_event(Event::Key(ctrl('w')));
        self.app.on_event(Event::Key(shift(KeyCode::Char('E'))));
        self.idle();
    }

    fn analyze(&mut self) {
        self.app.on_event(Event::Key(ctrl('w')));
        self.app.on_event(Event::Key(shift(KeyCode::Char('A'))));
        self.idle();
    }

    fn idle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.app.console().is_busy() {
            self.app.drain_events();
            assert!(Instant::now() < deadline, "the run did not end");
            std::thread::sleep(Duration::from_millis(1));
        }
        self.app.drain_events();
    }

    fn rows(&self) -> i64 {
        rusqlite::Connection::open(&self.path)
            .unwrap()
            .query_row("SELECT count(*) FROM t", [], |row| row.get(0))
            .unwrap()
    }

    fn screen(&mut self) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 34)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        let rows = terminal.backend().buffer().content().chunks(120);
        rows.map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn results(&self) -> usize {
        self.app.console().results.len()
    }
}

#[test]
fn the_key_shows_the_plan_of_the_statement_in_a_plan_tab() {
    let mut run = Run::new("plan-key.db");
    run.write("SELECT * FROM t JOIN u ON u.a = t.a");
    run.explain();
    let screen = run.screen();
    assert!(screen.contains("Plan"), "{screen}");
    assert!(!screen.contains("Result 1"), "{screen}");
    assert!(screen.contains("SCAN u"), "{screen}");
    assert!(
        screen.contains("SEARCH t USING INTEGER PRIMARY KEY"),
        "{screen}"
    );
    let result = &run.app.console().results[0];
    assert!(result.plan);
    assert_eq!(result.table.columns, [text::PLAN_NODE]);
}

#[test]
fn a_plan_does_not_run_the_statement() {
    let mut run = Run::new("plan-no-run.db");
    run.write("DELETE FROM t");
    run.explain();
    assert!(!run.screen().contains("Confirm the run"));
    assert_eq!(run.rows(), 2);
    assert!(run.app.console().results[0].plan);
}

#[test]
fn a_plan_uses_the_statement_at_the_cursor() {
    let mut run = Run::new("plan-cursor.db");
    run.write("SELECT a FROM t;\nSELECT a FROM u;");
    run.explain();
    let screen = run.screen();
    assert!(screen.contains("SCAN u"), "{screen}");
    assert!(!screen.contains("SCAN t"), "{screen}");
}

#[test]
fn a_selection_with_two_statements_is_refused() {
    let mut run = Run::new("plan-two.db");
    run.write("SELECT 1;\nSELECT 2;");
    run.app.on_event(Event::Key(ctrl('a')));
    run.explain();
    let screen = run.screen();
    assert!(screen.contains(text::PLAN_ONE_STATEMENT), "{screen}");
    assert_eq!(run.results(), 0);
}

#[test]
fn a_statement_with_explain_is_refused() {
    let mut run = Run::new("plan-explain.db");
    run.write("explain SELECT 1");
    run.explain();
    let screen = run.screen();
    assert!(screen.contains(text::PLAN_HAS_EXPLAIN), "{screen}");
    assert_eq!(run.results(), 0);
}

#[test]
fn a_comment_before_explain_does_not_hide_it() {
    let mut run = Run::new("plan-explain-comment.db");
    run.write("/* note */ explain SELECT 1");
    run.explain();
    let screen = run.screen();
    assert!(screen.contains(text::PLAN_HAS_EXPLAIN), "{screen}");
    assert_eq!(run.results(), 0);
}

#[test]
fn explain_in_a_comment_does_not_refuse_the_plan() {
    let mut run = Run::new("plan-comment-word.db");
    run.write("-- explain this\nSELECT * FROM t JOIN u ON u.a = t.a");
    run.explain();
    let screen = run.screen();
    assert!(!screen.contains(text::PLAN_HAS_EXPLAIN), "{screen}");
    assert!(screen.contains("SCAN u"), "{screen}");
}

#[test]
fn sqlite_refuses_a_real_run() {
    let mut run = Run::new("plan-analyze.db");
    run.write("SELECT 1");
    run.analyze();
    let screen = run.screen();
    assert!(screen.contains(text::PLAN_ANALYZE_REFUSED), "{screen}");
    assert_eq!(run.results(), 0);
}

#[test]
fn a_bad_statement_marks_its_own_text_in_the_editor() {
    let mut run = Run::new("plan-error.db");
    run.write("SELECT * FROM missing");
    run.explain();
    let screen = run.screen();
    assert!(screen.contains("no such table"), "{screen}");
    assert_eq!(run.results(), 0);
}

#[test]
fn a_normal_run_after_a_plan_shows_a_normal_result() {
    let mut run = Run::new("plan-then-run.db");
    run.write("SELECT a FROM t");
    run.explain();
    assert!(run.app.console().results[0].plan);
    run.app.on_event(Event::Key(key(KeyCode::F(5))));
    run.idle();
    let result = &run.app.console().results[0];
    assert!(!result.plan);
    assert_eq!(result.table.rows.len(), 2);
    assert!(run.screen().contains("Result 1"));
}
