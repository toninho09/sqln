//! Export and import.

pub const TITLE_EXPORT: &str = "Export";
pub const TITLE_EXPORT_EXISTS: &str = "File exists";
pub const TITLE_EXPORT_PART: &str = "Incomplete export";
pub const BUTTON_EXPORT: &str = "Export";
pub const OPTION_EXPORT_REPLACE: &str = "Replace";
pub const OPTION_EXPORT_KEEP: &str = "Keep";
pub const OPTION_EXPORT_DELETE: &str = "Delete";
pub const FIELD_EXPORT_ROWS: &str = "Rows";
pub const FIELD_EXPORT_FILE: &str = "File";
pub const FIELD_EXPORT_HEADER: &str = "Header";
pub const FIELD_EXPORT_DELIMITER: &str = "Delimiter";
pub const FIELD_EXPORT_NULL: &str = "NULL text";
pub const FIELD_EXPORT_BOM: &str = "UTF-8 BOM";
pub const FIELD_EXPORT_TABLE: &str = "Table";
pub const FIELD_EXPORT_GROUPING: &str = "INSERT";
pub const FIELD_EXPORT_BATCH: &str = "Rows per INSERT";
pub const VALUE_EXPORT_ALL_ROWS: &str = "All rows";
pub const VALUE_EXPORT_ONE_ROW: &str = "One per row";
pub const VALUE_EXPORT_GROUPED: &str = "Grouped";
pub const VALUE_EXPORT_NULL_EMPTY: &str = "Empty";
pub const VALUE_EXPORT_NULL_WORD: &str = "NULL";
pub const EXPORT_NO_FILE: &str = "Write the path of the file.";
pub const EXPORT_IS_FOLDER: &str = "This path is a folder. Write a file name.";
pub const EXPORT_NO_TABLE: &str = "Write the table of the INSERT statements.";
pub const EXPORT_IN_USE: &str = "Another export writes to this file.";
pub const EXPORT_ALL_READ: &str =
    "All rows needs a plain read. This statement can change data, so it does not run again.";
pub const EXPORT_ALL_SCRIPT: &str =
    "All rows needs one statement. Run this statement alone, then export.";
pub const EXPORT_ALL_SOURCE: &str =
    "All rows is not available for this source. Load more rows first.";
pub const EXPORT_RESULT_CLOSED: &str = "The result closed before the end of the export.";
pub const EXPORT_DELETED: &str = "The incomplete file is deleted.";
pub const EXPORT_STOPPED: &str = "The export stopped before the first row.";
pub const TITLE_IMPORT: &str = "Import";
pub const TITLE_IMPORT_DONE: &str = "Import complete";
pub const TITLE_IMPORT_FAILED: &str = "Import not complete";
pub const TITLE_IMPORT_PRODUCTION: &str = "Import to production";
pub const BUTTON_IMPORT: &str = "Import";
pub const FIELD_IMPORT_FORMAT: &str = "Format";
pub const FIELD_IMPORT_ENCODING: &str = "Encoding";
pub const FIELD_IMPORT_MODE: &str = "On failure";
pub const FIELD_IMPORT_BATCH: &str = "Rows per batch";
pub const FIELD_IMPORT_LOG: &str = "Log file";
pub const FIELD_IMPORT_LIMIT: &str = "Error limit";
pub const VALUE_IMPORT_UTF8: &str = "UTF-8";
pub const VALUE_IMPORT_WINDOWS_1252: &str = "Windows-1252";
pub const VALUE_IMPORT_ALL: &str = "All or nothing";
pub const VALUE_IMPORT_BATCH: &str = "By batch";
pub const VALUE_IMPORT_LOG: &str = "Log errors";
pub const VALUE_IMPORT_IGNORE: &str = "ignore";
pub const VALUE_IMPORT_NULL: &str = "NULL";
pub const IMPORT_HEAD_COLUMN: &str = "Column";
pub const IMPORT_HEAD_TYPE: &str = "Type";
pub const IMPORT_HEAD_SOURCE: &str = "File column";
pub const IMPORT_NO_FILE: &str = "Write the path of the file.";
pub const IMPORT_READING: &str = "The dialog still reads the file.";
pub const IMPORT_READING_LINE: &str = "Reading the file.";
pub const IMPORT_CHOOSE_FILE: &str = "Choose a file. Press Tab to read the path.";
pub const IMPORT_NO_COLUMNS: &str = "The file has no columns.";
pub const IMPORT_NO_MAPPING: &str = "Choose a file column for one table column or more.";
pub const IMPORT_JSON_FORMAT: &str = "JSON files have no format options.";
pub const IMPORT_STOPPED: &str = "The import stopped.";
pub const IMPORT_NOT_UTF8: &str = "This row is not valid UTF-8. Try Windows-1252.";
pub const IMPORT_NOT_OBJECT: &str = "This value is not a JSON object.";
pub const IMPORT_NOT_ARRAY: &str = "A JSON file must hold an array of objects.";
pub const IMPORT_JSON_OPEN: &str = "The JSON file ends before its array ends.";
pub const IMPORT_LOST: &str = "The connection stopped. The last batch has an unknown state.";
pub const IMPORT_TASK_FAILED: &str = "The import task stopped.";

pub fn export_selection(rows: usize, columns: usize) -> String {
    format!("Selection ({rows} × {columns})")
}

pub fn export_loaded(rows: usize) -> String {
    format!("Loaded rows ({rows})")
}

pub fn export_delimiter(symbol: char) -> String {
    format!("{symbol}")
}

pub fn export_exists(path: &str) -> String {
    format!("{path} exists. Replace this file?")
}

pub fn export_progress(rows: usize, path: &str, stop: Option<&str>) -> String {
    let progress = format!("Export: {rows} rows written to {path}");
    match stop {
        Some(key) => format!("{key} stops. {progress}"),
        None => progress,
    }
}

pub fn export_done(rows: usize, path: &str) -> String {
    format!("Exported {rows} rows to {path}.")
}

pub fn import_title(table: &str) -> String {
    format!("Import to {table}")
}

pub fn import_summary(columns: usize, mapped: usize, table: usize) -> String {
    format!("{columns} file columns. {mapped} of {table} table columns get values.")
}

pub fn import_failed(reason: &str) -> String {
    format!("The import failed: {reason}")
}

pub fn import_closed(rows: usize) -> String {
    format!("The tab closed. The import saved {rows} rows.")
}

pub fn import_progress(rows: usize, errors: usize, percent: u64, stop: Option<&str>) -> String {
    let stop = stop.map_or_else(String::new, |key| format!(" {key} stops."));
    format!("Importing: {rows} rows, {errors} errors, {percent}% of the file.{stop}")
}

pub fn import_production(path: &str, table: &str, connection: &str) -> String {
    format!("Import {path} to the table {table} of the production connection {connection}?")
}

/// The end of an import.
pub struct ImportReport<'a> {
    pub rows: usize,
    pub errors: usize,
    pub ignored: u64,
    pub log: Option<&'a str>,
    pub failure: Option<&'a str>,
}

pub fn import_report(report: &ImportReport<'_>) -> String {
    let mut body = format!("Rows inserted: {}.", report.rows);
    if report.errors > 0 {
        body.push_str(&format!("\nRows with errors: {}.", report.errors));
    }
    if report.ignored > 0 {
        let ignored = report.ignored;
        body.push_str(&format!(
            "\n{ignored} objects had keys that are not columns."
        ));
    }
    if let Some(log) = report.log {
        body.push_str(&format!("\nLog: {log}"));
    }
    if let Some(failure) = report.failure {
        body.push_str(&format!("\n{failure}"));
    }
    body
}

pub fn import_open_quote(line: u64) -> String {
    format!("The quotes that start at line {line} have no end.")
}

pub fn import_field_count(count: usize, width: usize) -> String {
    format!("This row has {count} fields. The file has {width} columns.")
}

/// Returns the name of a column of a file without a header.
pub fn import_column(number: usize) -> String {
    format!("column_{number}")
}

pub fn import_json_after(line: u64) -> String {
    format!("Line {line} has text after the end of the JSON array.")
}

pub fn import_json_syntax(line: u64) -> String {
    format!("Line {line} does not continue the JSON array.")
}

pub fn import_nul(column: &str) -> String {
    format!("The value of {column} holds a NUL byte.")
}

pub fn import_base64(column: &str) -> String {
    format!("The value of {column} is not valid Base64.")
}

pub fn import_bad_row(line: u64, reason: &str) -> String {
    format!("Line {line}: {reason}")
}

pub fn import_bad_lot(first: u64, last: u64, reason: &str) -> String {
    format!("The batch of lines {first} to {last} failed: {reason}")
}

pub fn import_error_limit(limit: usize) -> String {
    format!("The import stopped after {limit} errors.")
}

pub fn import_log_failed(reason: &str) -> String {
    format!("The log was not written: {reason}")
}

pub fn export_failed(reason: &str) -> String {
    format!("The export failed: {reason}")
}

pub fn export_choices_failed(reason: &str) -> String {
    format!("The export choices are not saved: {reason}")
}

/// Asks about the rows of an export that did not end. `failure` holds the reason.
pub fn export_part(rows: usize, failure: Option<&str>, replaces: bool) -> String {
    let start = failure.map_or_else(
        || format!("The export stopped after {rows} rows."),
        |reason| format!("The export failed after {rows} rows: {reason}"),
    );
    let question = if replaces {
        "Keep replaces the old file."
    } else {
        "Keep this incomplete file?"
    };
    format!("{start} {question}")
}
