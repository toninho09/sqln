//! Compare tabs and dialogs.

use super::plural;

pub const COMPARE_REFUSED: &str = "This source has no tables to compare.";
pub const COMPARE_CONSOLE: &str = "A console has no tables. Use the row of a schema or a table.";
pub const COMPARE_VIEW: &str = "A view has no keys to compare. Use the row of a table.";
pub const COMPARE_NO_CONNECTION: &str = "Add a database connection first.";
pub const COMPARE_TABLE_ON_BOTH: &str = "Write the source table too, or clear the target table.";
pub const COMPARE_SAME_SIDES: &str = "The target is the source. Choose another target.";
pub const COMPARE_LIMITED: &str =
    "A limit stopped the read. The compare needs each row, so it shows nothing.";
pub const COMPARE_STOPPED: &str = "The read stopped. The compare did not change.";
pub const COMPARE_BUDGET: &str = "The data budget is full. Close a result and read again.";
pub const COMPARE_EQUAL: &str = "No differences. The target is equal to the source.";
pub const COMPARE_NO_SCRIPT: &str = "No script: the target has no differences.";
pub const COMPARE_NO_SCRIPT_DIALECTS: &str = "No script: the two sources have different dialects.";
pub const COMPARE_NOT_READ: &str = "The compare has no result yet.";
pub const COMPARE_LOADING: &str = "loading…";
pub const COMPARE_TWO_DIALECTS: &str = "no script: two dialects";
pub const COMPARE_COLUMNS: [&str; 6] = ["Change", "Object", "Table", "Name", "Source", "Target"];
pub const COMPARE_CREATE: &str = "create";
pub const COMPARE_DROP: &str = "drop";
pub const COMPARE_ALTER: &str = "alter";
pub const COMPARE_TABLE: &str = "table";
pub const COMPARE_COLUMN: &str = "column";
pub const COMPARE_PRIMARY_KEY: &str = "primary key";
pub const COMPARE_UNIQUE: &str = "unique key";
pub const COMPARE_FOREIGN_KEY: &str = "foreign key";
pub const COMPARE_CHECK: &str = "check";
pub const COMPARE_INDEX: &str = "index";
pub const COMPARE_NO_KEY: &str = "The source table has no primary key, so its rows have no match.";
pub const COMPARE_DATA_BUDGET: &str =
    "The source table does not fit in the data budget. The compare shows nothing.";
pub const COMPARE_DATA_TABLE: &str = "A data compare needs a source table.";
pub const COMPARE_CHANGE: &str = "Change";
pub const COMPARE_INSERT: &str = "insert";
pub const COMPARE_DELETE: &str = "delete";
pub const COMPARE_TARGET_ROW: &str = "  target";
pub const KIND_STRUCTURE: &str = "Structure";
pub const KIND_DATA: &str = "Data";
pub const FIELD_COMPARE_KIND: &str = "Compare";
pub const COMPARE_DATA_HINT: &str =
    "The script makes the rows of the target table equal to the rows of the source table.";
pub const TITLE_COMPARE: &str = "Compare";
pub const BUTTON_COMPARE: &str = "Compare";
pub const FIELD_COMPARE_SOURCE: &str = "Source connection";
pub const FIELD_COMPARE_SOURCE_SCHEMA: &str = "Source schema";
pub const FIELD_COMPARE_SOURCE_TABLE: &str = "Source table";
pub const FIELD_COMPARE_TARGET: &str = "Target connection";
pub const FIELD_COMPARE_TARGET_SCHEMA: &str = "Target schema";
pub const FIELD_COMPARE_TARGET_TABLE: &str = "Target table";
pub const FIELD_COMPARE_REMOVALS: &str = "Removals as statements";
pub const COMPARE_HINT: &str =
    "The script makes the target equal to the source. An empty table compares the schema.";

pub fn compare_columns(count: usize) -> String {
    plural(count, "column")
}

pub fn compare_label(name: &str) -> String {
    format!("{name} (compare)")
}

pub fn compare_title(source: &str, target: &str) -> String {
    format!("{source} → {target} · compare")
}

pub fn compare_summary(differences: usize) -> String {
    plural(differences, "difference")
}

pub fn data_compare_label(name: &str) -> String {
    format!("{name} (data)")
}

pub fn data_compare_title(source: &str, target: &str) -> String {
    format!("{source} → {target} · data compare")
}

pub fn compare_no_target_table(table: &str) -> String {
    format!("The target has no table {table}. Compare the structure first.")
}

pub fn compare_missing_column(column: &str) -> String {
    format!("The target has no column {column}. Compare the structure first.")
}

pub fn compare_update(columns: &str) -> String {
    format!("update {columns}")
}

pub fn data_compare_header(source: &str, target: &str, differences: usize) -> String {
    format!(
        "-- SQLN data compare\n-- Source: {source}\n-- Target: {target}\n-- {}",
        plural(differences, "difference")
    )
}

pub fn compare_wait_catalog(connection: &str) -> String {
    format!("Wait for the schemas of {connection}.")
}

pub fn compare_no_table(table: &str) -> String {
    format!("The source has no table {table}.")
}

pub fn compare_script_header(source: &str, target: &str, differences: usize) -> String {
    format!(
        "-- SQLN structure compare\n-- Source: {source}\n-- Target: {target}\n-- {}",
        plural(differences, "difference")
    )
}
