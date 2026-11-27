//! Results, table tabs, plans, diagrams, structures, sessions, and statements.

use super::{NO_MATCHES, count, plural};

pub const TITLE_GENERATE: &str = "Generate SQL";
pub const NO_COLUMNS_TO_LIST: &str = "This relation has no known columns.";
pub const TEMPLATE_DDL: &str = "DDL";
pub const TITLE_FOLLOW: &str = "Follow a key";
/// The marks of a key in the list of `F`: a key that leaves the table, and a key of
/// another table that reaches it.
pub const FOLLOW_LEAVING: &str = "→";
pub const FOLLOW_REACHING: &str = "←";
pub const FOLLOW_NONE: &str = "The row has no key with a value to follow.";
pub const FOLLOW_REFUSED: &str = "Key navigation acts on the rows of a table.";
pub const PLAN_REFUSED: &str = "This source has no query plan.";
pub const PLAN_ANALYZE_REFUSED: &str = "SQLite cannot run a statement inside a plan.";
pub const PLAN_ONE_STATEMENT: &str = "A plan needs one statement. Select only one.";
pub const PLAN_HAS_EXPLAIN: &str = "The statement has EXPLAIN already. Use Run, or remove it.";
pub const PLAN_UNREADABLE: &str =
    "SQLN cannot show this plan as a tree. The grid shows the text of the database.";
pub const PLAN_ANALYZE_RUNS: &str =
    "The database runs this statement to measure it. Its changes can stay.";
pub const PLAN_TAB: &str = "Plan";
pub const PLAN_NODE: &str = "Node";
pub const PLAN_OBJECT: &str = "Object";
pub const PLAN_SHARE: &str = "Share";
pub const PLAN_COST: &str = "Cost";
pub const PLAN_ROWS: &str = "Rows";
pub const PLAN_TIME: &str = "Time (ms)";
pub const PLAN_ACTUAL_ROWS: &str = "Actual rows";
pub const PLAN_LOOPS: &str = "Loops";
pub const PLAN_DETAIL: &str = "Detail";
pub const RESULT_READING: &str = "This result receives rows. Ctrl+W x stops the script.";
pub const NO_MESSAGES: &str = "No messages.";
pub const MESSAGES_TAB: &str = "Messages";
/// Returns the label of the tab. The number says that a warning waits on that tab.
pub fn messages_tab(warnings: usize) -> String {
    match warnings {
        0 => MESSAGES_TAB.to_owned(),
        warnings => format!("{MESSAGES_TAB} ({warnings})"),
    }
}
pub fn more_notices(count: usize) -> String {
    format!("{count} more notices.")
}
pub fn limit_added(rows: usize) -> String {
    format!("SQLN added LIMIT {rows} to this statement.")
}
pub fn scan_added(scan: u64) -> String {
    format!(
        "SQLN added SCAN {scan} to this read, because it had no SCAN. Write your own SCAN to read more messages."
    )
}
pub const SECTION_COLUMNS: &str = "Columns";
pub const SECTION_INDEXES: &str = "Indexes";
pub const SECTION_FOREIGN_KEYS: &str = "Foreign keys";
pub const SECTION_STATISTICS: &str = "Statistics";
pub const ALL_SCHEMAS: &str = "all schemas";
pub const STRUCTURE_REFUSED: &str = "This source has no structure. Keys have no columns.";
pub const SESSIONS_REFUSED: &str = "SQLN lists sessions only on PostgreSQL and MySQL.";
pub const LOCKS_REFUSED: &str = "SQLN lists lock waits only on PostgreSQL and MySQL.";
pub const DIAGRAM_REFUSED: &str = "This source has no diagram. Keys have no columns.";
pub const DIAGRAM_NO_SCHEMA: &str =
    "The connection has no current schema. Use the row of a schema.";
pub const DIAGRAM_VIEW: &str = "A view has no keys. Use the row of its schema.";
pub const DIAGRAM_CONSOLE: &str = "A console has no tables. Use the row of a schema or a table.";
pub const DIAGRAM_EMPTY: &str = "This schema has no tables.";
pub const DIAGRAM_STOPPED: &str = "The read stopped. The diagram did not change.";
pub const DIAGRAM_PARTIAL: &str = "partial: a limit stopped the read";
pub const DIAGRAM_KEYS_ONLY: &str = "keys only";
pub const SESSIONS_READ_ONLY: &str = "A read-only connection cannot change a session.";
pub const SESSION_SIGNAL_SENT: &str = "The database accepted the request.";
pub const SESSION_GONE: &str = "The session ended before the request.";
pub const TITLE_CANCEL_QUERY: &str = "Cancel the query";
pub const TITLE_END_SESSION: &str = "End the session";
pub const OPTION_CANCEL_QUERY: &str = "Cancel the query";
pub const OPTION_END_SESSION: &str = "End the session";

pub const LABEL_WHERE: &str = "WHERE";
pub const LABEL_ORDER_BY: &str = "ORDER BY";
pub const BUTTON_APPLY: &str = "Apply";
pub const BUTTON_CLEAR: &str = "Clear";
pub const BUTTON_APPLY_FILTERS: &str = "Apply filters";
pub const BUTTON_CLEAR_FILTERS: &str = "Clear filters";
pub const BUTTON_REVIEW: &str = "Review";
pub const BUTTON_DISCARD: &str = "Discard";
pub const BUTTON_LOAD_MORE: &str = "Load more";
pub const BUTTON_PREVIOUS: &str = "Previous";
pub const BUTTON_NEXT: &str = "Next";
/// Writes the place of a page. The total stays open while the table can have more rows.
pub fn table_page(page: usize, pages: usize, more: bool) -> String {
    let open = if more { "+" } else { "" };
    format!("Page {page} of {pages}{open}")
}

/// Writes the place of a page for a narrow row.
pub fn table_page_short(page: usize, pages: usize, more: bool) -> String {
    let open = if more { "+" } else { "" };
    format!("{page}/{pages}{open}")
}
pub const TABLE_TEXT_REFUSED: &str = "The WHERE and ORDER BY text must stay in one statement. Remove a semicolon, an open quote, or an open comment.";
pub const TABLE_BUSY: &str = "The table has a load in progress. Ctrl+W x stops it.";
pub const TABLE_NO_MORE: &str = "The table has no more rows to load.";
pub const TABLE_BLOCKED: &str = "The data budget is full. Close a tab to free space.";
pub const TABLE_STOPPED: &str = "The load stopped. The rows already loaded stay.";
pub const TABLE_TIMEOUT: &str = "The time limit ended. The database did not confirm the result.";
pub const TABLE_CLOSED: &str = "The connection closed. The result is unknown.";
pub const TABLE_CANCELED: &str = "The load stopped before it started. No SQL was sent.";
pub const TABLE_MORE: &str = "more rows available";
pub const TABLE_NO_MORE_ROWS: &str = "no more rows";
pub const TABLE_WAITING: &str = "waiting for a connection";
pub const TABLE_LOADING: &str = "loading";
pub const TABLE_COUNTING: &str = "counting";
pub const TABLE_STOPPING: &str = "stopping";
pub const TABLE_BUDGET: &str = "data budget reached; close a tab to free space";
pub const TABLE_VALUE_TOO_LARGE: &str = "a value is larger than the data budget";
pub const TABLE_NO_ORDER: &str = "No ORDER BY: pages can repeat or skip rows.";
pub const BUTTON_RELOAD: &str = "Reload";
pub const NOTE_PARTIAL_KEYS: &str = "(partial list)";
/// The connection shows only the schemas of its list.
pub const NOTE_FILTERED: &str = "(filtered)";

/// Explains why the editor refuses a key during a review.
pub fn review_read_only(accept: &str, reject: &str, close: &str, close_label: &str) -> String {
    format!(
        "The editor is read only during the review. {accept} accepts, {reject} rejects, {close}: {close_label}."
    )
}
pub const STOPPED_BEFORE_SEND: &str = "The statement stopped before it reached the database.";

pub fn search_count(position: usize, total: usize, partial: bool) -> String {
    match (total, partial) {
        (0, false) => NO_MATCHES.to_owned(),
        (_, false) => format!("{} of {}", count(position), count(total)),
        (_, true) => format!("{} of {}+", count(position), count(total)),
    }
}

/// Names a result tab by the number of its statement: `#3`, `#3.2` for the next grid
/// of the same statement, and `Plan #3` for a plan.
pub fn result_tab(number: usize, part: usize, plan: bool) -> String {
    let base = if part > 1 {
        format!("#{number}.{part}")
    } else {
        format!("#{number}")
    };
    if plan {
        format!("{PLAN_TAB} {base}")
    } else {
        base
    }
}

/// Writes the row under the grid of a plan. `top` is the node with the largest share.
pub fn plan_summary(
    nodes: usize,
    cost: Option<&str>,
    time: Option<&str>,
    top: Option<(&str, f64)>,
) -> String {
    let mut parts = vec![plural(nodes, "node")];
    parts.extend(cost.map(|cost| format!("cost {cost}")));
    parts.extend(time.map(|time| format!("{time} ms")));
    parts.extend(top.map(|(name, share)| format!("most: {name} {:.0}%", share * 100.0)));
    parts.join(" · ")
}

pub fn rows_loaded(rows: usize) -> String {
    format!("{} loaded", plural(rows, "row"))
}

pub fn rows_of_total(rows: usize, total: usize) -> String {
    format!("{} of {} loaded", count(rows), plural(total, "row"))
}

pub fn table_label(name: &str, kind: &str) -> String {
    format!("{name} ({kind})")
}

pub fn table_title(connection: &str, schema: &str, relation: &str, kind: &str) -> String {
    format!("{connection} · {schema}.{relation} · {kind}")
}

pub fn structure_title(connection: &str, name: &str) -> String {
    format!("{connection} · {name} · structure")
}

pub fn diagram_label(name: &str) -> String {
    format!("{name} (diagram)")
}

pub fn diagram_title(connection: &str, name: &str) -> String {
    format!("{connection} · {name} · diagram")
}

/// Writes the row under a diagram.
pub fn diagram_summary(tables: usize, keys: usize) -> String {
    format!("{} · {}", plural(tables, "table"), plural(keys, "key"))
}

pub fn sessions_title(connection: &str) -> String {
    format!("{connection} · sessions")
}

pub fn locks_title(connection: &str) -> String {
    format!("{connection} · lock waits")
}

pub fn rows_read(rows: usize) -> String {
    plural(rows, "row")
}

/// The rows of a read as a sentence, before more sentences.
pub fn rows_read_sentence(rows: usize) -> String {
    format!("{} read.", plural(rows, "row"))
}

/// The summary of a live result: the rows in the grid and the oldest rows that left it.
pub fn live_rows(rows: usize, dropped: usize) -> String {
    if dropped == 0 {
        format!("{} received. Live.", plural(rows, "row"))
    } else {
        format!(
            "{} shown, {} dropped. Live: the grid keeps the newest rows.",
            plural(rows, "row"),
            count(dropped)
        )
    }
}

/// The summary of a live result after its end.
pub fn live_rows_done(rows: usize, dropped: usize) -> String {
    if dropped == 0 {
        format!("{} received.", plural(rows, "row"))
    } else {
        format!(
            "{} shown, {} dropped. The grid kept the newest rows.",
            plural(rows, "row"),
            count(dropped)
        )
    }
}

pub fn row_cap_reached(rows: usize) -> String {
    format!(
        "{} read. Row cap reached. The result can have more rows.",
        plural(rows, "row")
    )
}

pub const RUN_BUDGET_FULL: &str = "Run refused: the data budget is full. Close result tabs with x or Delete, or increase the limit in Settings.";

pub fn budget_reached(rows: usize) -> String {
    format!(
        "{} read. Data budget reached. Close result tabs, increase the limit, or run less data.",
        plural(rows, "row")
    )
}

pub fn statement_changed(index: usize, rows: u64) -> String {
    let rows = usize::try_from(rows).unwrap_or(usize::MAX);
    format!("Statement {}: {} changed.", index + 1, plural(rows, "row"))
}

pub fn statement_done(index: usize) -> String {
    format!("Statement {}: done.", index + 1)
}

pub fn statement_error(index: usize, message: &str) -> String {
    format!("Statement {}: {message}", index + 1)
}

pub fn statement_stopped(index: usize) -> String {
    format!("Statement {}: stopped.", index + 1)
}

pub fn stopped_before(index: usize) -> String {
    format!(
        "Stopped before statement {}. The database did not receive it.",
        index + 1
    )
}

pub fn statement_timeout(index: usize) -> String {
    format!(
        "Statement {}: the time limit ended. The database did not confirm the result.",
        index + 1
    )
}

pub fn statement_unknown(index: usize) -> String {
    format!(
        "Statement {}: the connection closed. The result is unknown.",
        index + 1
    )
}

pub fn row_title(row: usize, loaded: usize) -> String {
    format!(" row {} of {} loaded ", count(row + 1), count(loaded))
}

pub fn row_footer(columns: usize) -> String {
    format!(" {} ", plural(columns, "column"))
}

pub const ANALYSIS_UNAVAILABLE: &str = "Analysis unavailable. Wait for the result. A comparison needs another complete result tab with the same columns.";
