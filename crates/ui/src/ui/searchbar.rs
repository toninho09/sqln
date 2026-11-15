//! The row of a grid search.

use ratatui::text::{Line, Span};

use super::text;
use super::theme::Theme;
use crate::grid::{ResultTable, RowSearch, printable};

pub fn search_line(search: &RowSearch, table: &ResultTable, theme: &Theme) -> Line<'static> {
    let count = if search.query().is_empty() {
        String::new()
    } else {
        text::search_count(search.position(), search.total(), search.is_partial(table))
    };
    Line::from(vec![
        Span::raw(format!(
            " {}{}",
            text::FIND_LABEL,
            printable(search.query())
        )),
        Span::styled(" ", theme.cursor),
        Span::styled(format!(" {count}"), theme.hint),
    ])
}
