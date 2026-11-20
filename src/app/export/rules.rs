//! The decisions of an export that need no `App`: the destination of the dialog, whether
//! a console can read all rows again, and what happens to the file at the end.

use std::path::{Path, PathBuf};

use crate::console::Console;
use crate::exec::{ConsoleId, ExecutionEnd};
use crate::export::resolve_path;
use crate::export_form::AllRows;
use crate::grid::printable;
use crate::result::ResultTab;
use crate::sql::{Dialect, is_plain_read};
use crate::table::TableTab;
use crate::ui::text;

/// Returns the full path of the text of the dialog, or the text of the problem.
pub(super) fn destination_of(path: &str) -> Result<PathBuf, String> {
    if path.trim().is_empty() {
        return Err(text::EXPORT_NO_FILE.to_owned());
    }
    let path = resolve_path(path, std::env::home_dir())
        .map_err(|error| text::export_failed(&error.to_string()))?;
    if path.is_dir() {
        return Err(text::EXPORT_IS_FOLDER.to_owned());
    }
    Ok(path)
}

pub(super) fn shown_path(path: &Path) -> String {
    printable(&path.display().to_string())
}

/// Decides whether a console can read a result again. The session of an execution ends
/// with it, so a statement of a script can depend on the statements before it.
pub(super) fn console_all_rows(console: &Console, result: &ResultTab, dialect: Dialect) -> AllRows {
    let plain = console
        .statement_of(result)
        .is_some_and(|sql| is_plain_read(sql, dialect));
    if !plain {
        AllRows::Refused(text::EXPORT_ALL_READ)
    } else if result.source.alone || console.in_transaction() {
        AllRows::Available
    } else {
        AllRows::Refused(text::EXPORT_ALL_SCRIPT)
    }
}

pub(super) fn result_in<'a>(
    tables: &'a [TableTab],
    consoles: &'a [Console],
    id: ConsoleId,
    result: usize,
) -> Option<&'a ResultTab> {
    if let Some(tab) = tables.iter().find(|tab| tab.id == id) {
        return Some(&tab.result);
    }
    let console = consoles.iter().find(|console| console.id == id)?;
    console.results.get(result)
}

/// What happens to the file at the end of an export.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Ending {
    /// The console closes: the partial file goes away without a message.
    Discard,
    /// The export ended well: the file takes its name.
    Promote,
    /// Nothing reached the file: it goes away, and the status line says why.
    Refused(String),
    /// Some rows reached the file: a question asks to keep or delete it.
    AskAboutPart,
}

pub(super) fn ending(
    closing: bool,
    end: ExecutionEnd,
    failure: Option<&str>,
    rows: usize,
) -> Ending {
    if closing {
        Ending::Discard
    } else if end == ExecutionEnd::Completed && failure.is_none() {
        Ending::Promote
    } else if rows == 0 {
        Ending::Refused(
            failure.map_or_else(|| text::EXPORT_STOPPED.to_owned(), text::export_failed),
        )
    } else {
        Ending::AskAboutPart
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_path_and_a_folder_are_refused() {
        assert_eq!(destination_of("  "), Err(text::EXPORT_NO_FILE.to_owned()));
        let folder = std::env::temp_dir();
        let folder = folder.to_str().unwrap();
        assert_eq!(
            destination_of(folder),
            Err(text::EXPORT_IS_FOLDER.to_owned())
        );
    }

    #[test]
    fn the_end_keeps_a_complete_file_and_asks_about_a_part() {
        let completed = ExecutionEnd::Completed;
        assert_eq!(ending(true, completed, None, 10), Ending::Discard);
        assert_eq!(ending(false, completed, None, 10), Ending::Promote);
        assert_eq!(
            ending(false, completed, Some("disk"), 10),
            Ending::AskAboutPart
        );
        assert_eq!(
            ending(false, ExecutionEnd::Stopped, None, 3),
            Ending::AskAboutPart
        );
        let stopped = ending(false, ExecutionEnd::Stopped, None, 0);
        assert_eq!(stopped, Ending::Refused(text::EXPORT_STOPPED.to_owned()));
        let failed = ending(false, ExecutionEnd::Failed, Some("disk"), 0);
        assert_eq!(failed, Ending::Refused(text::export_failed("disk")));
    }
}
