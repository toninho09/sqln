//! The decisions of an import that need no `App`: the columns of the table, and the
//! messages at the end of each step.

use crate::exec::ExecutionEnd;
use crate::grid::{Row as GridRow, Value};
use crate::import_form::TableColumn;
use crate::ui::text;

/// Reads the name and the type of each row of a structure query.
pub(super) fn table_columns(rows: &[GridRow]) -> Vec<TableColumn> {
    let text_of = |value: Option<Value<'_>>| value.map_or_else(String::new, Value::display);
    rows.iter()
        .map(|row| {
            let mut cells = row.cells();
            let name = text_of(cells.next());
            let data_type = text_of(cells.next());
            TableColumn { name, data_type }
        })
        .collect()
}

/// Returns the message of a read of the columns that did not end, or `None` when the
/// dialog can open.
pub(super) fn columns_refusal(end: ExecutionEnd, failure: Option<String>) -> Option<String> {
    (end != ExecutionEnd::Completed).then(|| {
        let reason = failure.unwrap_or_else(|| text::IMPORT_STOPPED.to_owned());
        text::import_failed(&reason)
    })
}

/// The counts of a write that ended.
pub(super) struct Counts<'a> {
    pub rows: usize,
    pub errors: usize,
    pub ignored: u64,
    /// The log of the bad rows. Only a write with errors shows it.
    pub log: &'a str,
}

/// Returns the title and the text of the report of a write.
pub(super) fn report(
    counts: &Counts<'_>,
    end: ExecutionEnd,
    failure: Option<String>,
) -> (&'static str, String) {
    let log = (counts.errors > 0).then_some(counts.log);
    let reason = match end {
        ExecutionEnd::Completed => None,
        ExecutionEnd::Stopped | ExecutionEnd::CanceledBeforeStart => {
            Some(text::IMPORT_STOPPED.to_owned())
        }
        _ => failure,
    };
    let body = text::import_report(&text::ImportReport {
        rows: counts.rows,
        errors: counts.errors,
        ignored: counts.ignored,
        log,
        failure: reason.as_deref(),
    });
    let title = if end == ExecutionEnd::Completed {
        text::TITLE_IMPORT_DONE
    } else {
        text::TITLE_IMPORT_FAILED
    };
    (title, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::RowWriter;

    fn row(cells: &[Option<&str>]) -> GridRow {
        let mut writer = RowWriter::with_capacity(64);
        for cell in cells {
            writer.push(cell.map_or(Value::Null, Value::Text));
        }
        writer.finish()
    }

    #[test]
    fn each_row_gives_a_name_and_a_type() {
        let rows = [row(&[Some("id"), Some("int")]), row(&[Some("note"), None])];
        let columns = table_columns(&rows);
        assert_eq!(columns[0].name, "id");
        assert_eq!(columns[0].data_type, "int");
        assert_eq!(columns[1].name, "note");
    }

    #[test]
    fn a_read_of_columns_that_did_not_end_gives_its_reason() {
        assert_eq!(columns_refusal(ExecutionEnd::Completed, None), None);
        let stopped = columns_refusal(ExecutionEnd::Stopped, None);
        assert_eq!(stopped, Some(text::import_failed(text::IMPORT_STOPPED)));
        let failed = columns_refusal(ExecutionEnd::Failed, Some("denied".to_owned()));
        assert_eq!(failed, Some(text::import_failed("denied")));
    }

    #[test]
    fn the_report_names_the_log_only_with_errors_and_the_stop_as_its_reason() {
        let counts = |errors| Counts {
            rows: 5,
            errors,
            ignored: 0,
            log: "/tmp/import.log",
        };
        let (title, body) = report(&counts(0), ExecutionEnd::Completed, None);
        assert_eq!(title, text::TITLE_IMPORT_DONE);
        assert!(!body.contains("/tmp/import.log"), "{body}");
        let (_, body) = report(&counts(2), ExecutionEnd::Completed, None);
        assert!(body.contains("/tmp/import.log"), "{body}");
        let (title, body) = report(&counts(0), ExecutionEnd::Stopped, Some("x".to_owned()));
        assert_eq!(title, text::TITLE_IMPORT_FAILED);
        let expected = text::import_report(&text::ImportReport {
            rows: 5,
            errors: 0,
            ignored: 0,
            log: None,
            failure: Some(text::IMPORT_STOPPED),
        });
        assert_eq!(body, expected);
    }
}
