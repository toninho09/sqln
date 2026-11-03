//! The plan of one statement: the `EXPLAIN` text of each database, and the tree of its
//! nodes for the results grid.

use std::collections::HashMap;

use serde_json::Value as Json;
use unicode_width::UnicodeWidthStr;

use crate::grid::{ResultTable, Row, RowWriter, Value};
use crate::sql::Dialect;
use crate::ui::text;

/// The cells of the bar in the `Share` column.
const BAR_CELLS: usize = 10;
/// The spaces of one level in the plan text of MySQL.
const MYSQL_INDENT: usize = 4;
/// The fields of a PostgreSQL node that explain what the node does.
const POSTGRES_DETAILS: [&str; 8] = [
    "Index Cond",
    "Hash Cond",
    "Merge Cond",
    "Join Filter",
    "Filter",
    "Recheck Cond",
    "Sort Key",
    "Group Key",
];

/// The plan that one execution of a console asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Explain {
    pub dialect: Dialect,
    prefix: &'static str,
}

impl Explain {
    /// Returns `None` for a source without this plan. With `analyze`, the database runs
    /// the statement. SQLite cannot do this, and Redis has no plan.
    pub fn new(dialect: Dialect, analyze: bool) -> Option<Self> {
        let prefix = match (dialect, analyze) {
            (Dialect::Postgres, false) => "EXPLAIN (FORMAT JSON) ",
            (Dialect::Postgres, true) => "EXPLAIN (ANALYZE, FORMAT JSON) ",
            (Dialect::MySql, false) => "EXPLAIN FORMAT=TREE ",
            (Dialect::MySql, true) => "EXPLAIN ANALYZE ",
            (Dialect::Sqlite, false) => "EXPLAIN QUERY PLAN ",
            _ => return None,
        };
        Some(Self { dialect, prefix })
    }

    /// Returns the text that the database receives.
    pub fn wrap(&self, statement: &str) -> String {
        format!("{}{statement}", self.prefix)
    }

    /// Returns the bytes before the statement. An error position must skip them.
    pub fn prefix_len(&self) -> usize {
        self.prefix.len()
    }
}

/// One step of a plan.
#[derive(Clone, Debug, Default, PartialEq)]
struct Node {
    depth: usize,
    name: String,
    object: Option<String>,
    /// The estimate of this node and of the nodes under it.
    cost: Option<f64>,
    rows: Option<f64>,
    /// The milliseconds of all the loops of this node and of the nodes under it.
    time: Option<f64>,
    actual_rows: Option<f64>,
    loops: Option<f64>,
    detail: Option<String>,
}

/// What a reader finds in the text of a database.
struct Reading {
    nodes: Vec<Node>,
    /// The time of the whole statement, when the database reports it.
    time: Option<f64>,
}

/// A plan, ready for the grid.
#[derive(Debug)]
pub struct PlanView {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    pub summary: String,
    /// The width of the tree. The first column holds the main text of a plan, so it
    /// does not stop at the width of a normal column.
    pub tree_width: usize,
}

/// One column of the grid. A column without a value stays out.
struct Column {
    title: &'static str,
    number: bool,
    cells: Vec<Option<String>>,
}

/// Builds the grid of a plan from the rows of the database. It returns `None` for a
/// text that this module cannot read.
pub fn plan_view(dialect: Dialect, raw: &ResultTable) -> Option<PlanView> {
    let first_cell = || raw.rows.first()?.get(0).map(Value::full_text);
    let reading = match dialect {
        Dialect::Postgres => read_postgres(&first_cell()?)?,
        Dialect::MySql => read_mysql(&first_cell()?)?,
        Dialect::Sqlite => read_sqlite(raw)?,
        Dialect::Redis => return None,
    };
    let shares = shares(&reading.nodes);
    let columns = columns(&reading.nodes, &shares);
    let tree = columns.first()?.cells.iter().flatten();
    Some(PlanView {
        tree_width: tree.map(|cell| cell.width()).max()?,
        columns: columns.iter().map(|item| item.title.to_owned()).collect(),
        rows: grid_rows(&columns, reading.nodes.len()),
        summary: summary(&reading, &shares),
    })
}

fn read_postgres(text: &str) -> Option<Reading> {
    let root: Json = serde_json::from_str(text).ok()?;
    let entry = root.get(0)?;
    let mut nodes = Vec::new();
    push_postgres_node(entry.get("Plan")?, 0, &mut nodes)?;
    let time = entry.get("Execution Time").and_then(Json::as_f64);
    Some(Reading { nodes, time })
}

/// Adds a node and the nodes under it. The JSON reader has a depth limit, so this
/// recursion has one too.
fn push_postgres_node(node: &Json, depth: usize, nodes: &mut Vec<Node>) -> Option<()> {
    let number = |key: &str| node.get(key).and_then(Json::as_f64);
    let label = |key: &str| node.get(key).and_then(Json::as_str);
    let kind = label("Node Type")?;
    let name = match label("Join Type").filter(|join| *join != "Inner") {
        Some(join) => format!("{kind} ({join})"),
        None => kind.to_owned(),
    };
    let loops = number("Actual Loops");
    nodes.push(Node {
        depth,
        name,
        object: postgres_object(node),
        cost: number("Total Cost"),
        rows: number("Plan Rows"),
        // PostgreSQL reports the average time of one loop.
        time: number("Actual Total Time").map(|time| time * loops.unwrap_or(1.0)),
        actual_rows: number("Actual Rows"),
        loops,
        detail: postgres_detail(node),
    });
    let children = node.get("Plans").and_then(Json::as_array);
    for child in children.into_iter().flatten() {
        push_postgres_node(child, depth + 1, nodes)?;
    }
    Some(())
}

fn postgres_object(node: &Json) -> Option<String> {
    let label = |key: &str| node.get(key).and_then(Json::as_str);
    let target = label("Relation Name")
        .or(label("CTE Name"))
        .or(label("Function Name"));
    match (target, label("Index Name")) {
        (Some(target), Some(index)) => Some(format!("{target} ({index})")),
        (target, index) => target.or(index).map(str::to_owned),
    }
}

fn postgres_detail(node: &Json) -> Option<String> {
    let parts: Vec<String> = POSTGRES_DETAILS
        .iter()
        .filter_map(|key| {
            let value = match node.get(key)? {
                Json::String(text) => text.clone(),
                Json::Array(items) => {
                    let items: Vec<&str> = items.iter().filter_map(Json::as_str).collect();
                    items.join(", ")
                }
                _ => return None,
            };
            Some(format!("{key}: {value}"))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// Reads the tree of `EXPLAIN FORMAT=TREE` and `EXPLAIN ANALYZE`. A line without an
/// arrow continues the node before it, because a string in a condition can hold a break.
fn read_mysql(text: &str) -> Option<Reading> {
    let mut lines: Vec<(usize, String)> = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let body = line.trim_start();
        match body.strip_prefix("-> ") {
            Some(label) => lines.push((line.len() - body.len(), label.to_owned())),
            None => {
                let (_, label) = lines.last_mut()?;
                label.push(' ');
                label.push_str(body);
            }
        }
    }
    let nodes: Vec<Node> = lines
        .iter()
        .map(|(spaces, label)| mysql_node(spaces / MYSQL_INDENT, label))
        .collect();
    (!nodes.is_empty()).then_some(Reading { nodes, time: None })
}

/// Removes the last `(<start>...)` part of a label. It keeps the label when that part
/// is not at the end, because the text of a condition can look the same.
fn split_tail<'a>(label: &'a str, start: &str) -> Option<(&'a str, &'a str)> {
    let at = label.rfind(start)?;
    let tail = label[at + start.len()..].strip_suffix(')')?;
    (!tail.contains(')')).then_some((label[..at].trim_end(), tail))
}

/// Returns the value of `key=` in the words of a plan part.
fn field(tail: &str, key: &str) -> Option<f64> {
    tail.split_whitespace()
        .find_map(|word| word.strip_prefix(key)?.parse().ok())
}

/// Returns the last number of `first..last`, or the only number.
fn range_end(tail: &str) -> Option<f64> {
    let word = tail.split_whitespace().next()?;
    word.rsplit("..").next()?.parse().ok()
}

fn mysql_node(depth: usize, label: &str) -> Node {
    let (label, actual) =
        split_tail(label, "(actual time=").map_or((label, None), |(rest, tail)| (rest, Some(tail)));
    let label = label.strip_suffix("(never executed)").unwrap_or(label);
    let (label, estimate) = split_tail(label.trim_end(), "(cost=")
        .map_or((label.trim_end(), None), |(rest, tail)| (rest, Some(tail)));
    let loops = actual.and_then(|tail| field(tail, "loops="));
    Node {
        depth,
        name: label.to_owned(),
        cost: estimate.and_then(range_end),
        rows: estimate.and_then(|tail| field(tail, "rows=")),
        // MySQL reports the average time of one loop.
        time: actual
            .and_then(range_end)
            .map(|time| time * loops.unwrap_or(1.0)),
        actual_rows: actual.and_then(|tail| field(tail, "rows=")),
        loops,
        ..Node::default()
    }
}

/// Reads the rows of `EXPLAIN QUERY PLAN`: `id`, `parent`, `notused`, and `detail`.
fn read_sqlite(raw: &ResultTable) -> Option<Reading> {
    let mut depths: HashMap<i64, usize> = HashMap::new();
    let mut nodes = Vec::with_capacity(raw.rows.len());
    for row in &raw.rows {
        let number = |column: usize| row.get(column)?.full_text().parse::<i64>().ok();
        let (id, parent) = (number(0)?, number(1)?);
        let depth = depths.get(&parent).map_or(0, |depth| depth + 1);
        depths.insert(id, depth);
        nodes.push(Node {
            depth,
            name: row.get(3)?.full_text(),
            ..Node::default()
        });
    }
    (!nodes.is_empty()).then_some(Reading { nodes, time: None })
}

/// Returns the part of the work that each node does by itself, from 0 to 1. It uses the
/// real time when the plan has it, and the cost when it does not. A database reports a
/// value with the nodes under it, so each node subtracts its children.
fn shares(nodes: &[Node]) -> Vec<Option<f64>> {
    let timed = nodes.iter().any(|node| node.time.is_some());
    let deepest = nodes.iter().map(|node| node.depth).max().unwrap_or(0);
    // The sum of the values at each level that wait for their parent.
    let mut below = vec![0.0; deepest + 2];
    let mut own = vec![None; nodes.len()];
    for (index, node) in nodes.iter().enumerate().rev() {
        let children = std::mem::take(&mut below[node.depth + 1]);
        match if timed { node.time } else { node.cost } {
            Some(value) => {
                own[index] = Some((value - children).max(0.0));
                below[node.depth] += value;
            }
            // A node without a value gives its children to its parent.
            None => below[node.depth] += children,
        }
    }
    let total: f64 = own.iter().flatten().sum();
    own.into_iter()
        .map(|value: Option<f64>| value.filter(|_| total > 0.0).map(|value| value / total))
        .collect()
}

/// Writes the lines of the tree before each node. The walk starts at the end, because a
/// node must know if a sibling comes after it.
fn tree_prefixes(nodes: &[Node]) -> Vec<String> {
    let deepest = nodes.iter().map(|node| node.depth).max().unwrap_or(0);
    let mut open = vec![false; deepest + 1];
    let mut prefixes = vec![String::new(); nodes.len()];
    for (index, node) in nodes.iter().enumerate().rev() {
        let depth = node.depth;
        open[depth + 1..].fill(false);
        let prefix = &mut prefixes[index];
        for level in open.iter().take(depth).skip(1) {
            prefix.push_str(if *level { "│  " } else { "   " });
        }
        if depth > 0 {
            prefix.push_str(if open[depth] { "├─ " } else { "└─ " });
        }
        open[depth] = true;
    }
    prefixes
}

/// Writes a number without zeros at its end.
fn number_text(value: f64) -> String {
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    text.to_owned()
}

fn share_text(share: f64) -> String {
    let filled = ((share * BAR_CELLS as f64).round() as usize).min(BAR_CELLS);
    let empty = BAR_CELLS - filled;
    format!(
        "{}{} {:.0}%",
        "█".repeat(filled),
        "░".repeat(empty),
        share * 100.0
    )
}

fn columns(nodes: &[Node], shares: &[Option<f64>]) -> Vec<Column> {
    let numbers = |read: fn(&Node) -> Option<f64>| -> Vec<Option<String>> {
        nodes
            .iter()
            .map(|node| read(node).map(number_text))
            .collect()
    };
    let names = tree_prefixes(nodes)
        .into_iter()
        .zip(nodes)
        .map(|(prefix, node)| Some(format!("{prefix}{}", node.name)))
        .collect();
    let texts = |read: fn(&Node) -> Option<String>| nodes.iter().map(read).collect();
    let columns = [
        (text::PLAN_NODE, false, names),
        (text::PLAN_OBJECT, false, texts(|node| node.object.clone())),
        (
            text::PLAN_SHARE,
            false,
            shares.iter().map(|share| share.map(share_text)).collect(),
        ),
        (text::PLAN_COST, true, numbers(|node| node.cost)),
        (text::PLAN_ROWS, true, numbers(|node| node.rows)),
        (text::PLAN_TIME, true, numbers(|node| node.time)),
        (
            text::PLAN_ACTUAL_ROWS,
            true,
            numbers(|node| node.actual_rows),
        ),
        (text::PLAN_LOOPS, true, numbers(|node| node.loops)),
        (text::PLAN_DETAIL, false, texts(|node| node.detail.clone())),
    ];
    columns
        .into_iter()
        .map(|(title, number, cells)| Column {
            title,
            number,
            cells,
        })
        .filter(|column| column.cells.iter().any(Option::is_some))
        .collect()
}

fn grid_rows(columns: &[Column], count: usize) -> Vec<Row> {
    let mut writer = RowWriter::default();
    (0..count)
        .map(|index| {
            for column in columns {
                let cell = column.cells[index].as_deref().unwrap_or_default();
                writer.push(if column.number && !cell.is_empty() {
                    Value::Number(cell)
                } else {
                    Value::Text(cell)
                });
            }
            writer.finish()
        })
        .collect()
}

fn summary(reading: &Reading, shares: &[Option<f64>]) -> String {
    let nodes = &reading.nodes;
    let cost = nodes.iter().find_map(|node| node.cost).map(number_text);
    let time = reading
        .time
        .or_else(|| nodes.iter().find_map(|node| node.time))
        .map(number_text);
    let top = nodes
        .iter()
        .zip(shares)
        .filter_map(|(node, share)| Some((node.name.as_str(), (*share)?)))
        .max_by(|a, b| a.1.total_cmp(&b.1));
    text::plan_summary(nodes.len(), cost.as_deref(), time.as_deref(), top)
}
