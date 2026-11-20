//! The refusals of the edits of a table tab, without `App`. Each function returns the
//! reason that blocks the edit, or `None` when the edit can start.

use crate::table::TableTab;
use crate::ui::text;

/// The reasons that block every edit of the grid of a table tab. `writable` is true
/// when the connection is not read only and its source edits rows in a transaction.
pub(super) fn base_refusal(tab: &TableTab, writable: bool) -> Option<String> {
    if tab.is_busy() {
        return Some(text::TABLE_BUSY.to_owned());
    }
    if tab.edit_uncertain {
        return Some(text::EDIT_COMMIT_UNKNOWN.to_owned());
    }
    if tab.is_view || tab.structure.is_some() || !tab.filters || !writable {
        return Some(text::EDIT_UNAVAILABLE.to_owned());
    }
    None
}

/// The columns of the table changed since the edit metadata was read.
fn schema_changed(tab: &TableTab) -> Option<String> {
    let metadata = tab.edit_metadata.as_ref()?;
    (!metadata.matches(&tab.result.table.columns)).then(|| text::EDIT_SCHEMA_CHANGED.to_owned())
}

/// The reasons that block a new pending insert row.
pub(super) fn new_row_refusal(tab: Option<&TableTab>, writable: bool) -> Option<String> {
    let Some(tab) = tab else {
        return Some(text::EDIT_UNAVAILABLE.to_owned());
    };
    base_refusal(tab, writable).or_else(|| schema_changed(tab))
}

/// The reasons that block the delete mark of the row of the cursor.
pub(super) fn delete_row_refusal(tab: Option<&TableTab>, writable: bool) -> Option<String> {
    let Some(tab) = tab else {
        return Some(text::EDIT_UNAVAILABLE.to_owned());
    };
    if let Some(reason) = base_refusal(tab, writable) {
        return Some(reason);
    }
    if tab.result.table.rows.is_empty() {
        return Some(text::EDIT_NO_CELL.to_owned());
    }
    schema_changed(tab)
}

/// The reasons that block an edit of the cell of the cursor.
pub(super) fn cell_refusal(tab: &TableTab, writable: bool) -> Option<String> {
    if let Some(reason) = base_refusal(tab, writable) {
        return Some(reason);
    }
    if tab.result.table.rows.is_empty() {
        return Some(text::EDIT_NO_CELL.to_owned());
    }
    let row = tab.result.view.row;
    let columns = &tab.result.table.columns;
    if tab.result.edits.is_insert(row) {
        let metadata = tab.edit_metadata.as_ref()?;
        return metadata.editable_insert(columns, tab.result.view.col).err();
    }
    if tab.result.edits.deletes.contains_key(&row) {
        return Some(text::EDIT_ROW_DELETED.to_owned());
    }
    let metadata = tab.edit_metadata.as_ref()?;
    let row = tab.result.table.rows.get(row)?;
    metadata.editable(columns, row, tab.result.view.col).err()
}

/// Returns true while the tab holds changes that the user must apply or discard first.
pub(super) fn has_pending(tab: &TableTab) -> bool {
    !tab.result.edits.is_empty() || tab.edit_uncertain
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::RelationRef;
    use crate::grid::{RowWriter, Value};
    use crate::result::ResultTab;
    use crate::sql::Dialect;

    fn tab() -> TableTab {
        let relation = RelationRef {
            schema: "main".to_owned(),
            relation: "orders".to_owned(),
        };
        TableTab::new(7, 1, relation, false, Dialect::Sqlite, 100)
    }

    fn with_row(mut tab: TableTab) -> TableTab {
        let mut result = ResultTab::new(0, vec!["id".to_owned()]);
        let mut writer = RowWriter::with_capacity(8);
        writer.push(Value::Number("1"));
        result.table.rows.push(writer.finish());
        tab.result = result;
        tab
    }

    #[test]
    fn a_view_a_structure_and_a_read_only_connection_refuse_edits() {
        let mut view = tab();
        view.is_view = true;
        assert_eq!(
            base_refusal(&view, true),
            Some(text::EDIT_UNAVAILABLE.to_owned())
        );
        assert_eq!(
            base_refusal(&tab(), false),
            Some(text::EDIT_UNAVAILABLE.to_owned())
        );
        let mut uncertain = tab();
        uncertain.edit_uncertain = true;
        let refused = base_refusal(&uncertain, true);
        assert_eq!(refused, Some(text::EDIT_COMMIT_UNKNOWN.to_owned()));
        assert!(has_pending(&uncertain));
        assert_eq!(base_refusal(&tab(), true), None);
    }

    #[test]
    fn an_empty_grid_takes_a_new_row_but_no_delete_or_cell_edit() {
        let empty = tab();
        assert_eq!(new_row_refusal(Some(&empty), true), None);
        let no_cell = Some(text::EDIT_NO_CELL.to_owned());
        assert_eq!(delete_row_refusal(Some(&empty), true), no_cell);
        assert_eq!(cell_refusal(&empty, true), no_cell);
        let unavailable = Some(text::EDIT_UNAVAILABLE.to_owned());
        assert_eq!(new_row_refusal(None, true), unavailable);
        assert_eq!(delete_row_refusal(None, true), unavailable);
    }

    #[test]
    fn a_row_marked_for_delete_refuses_a_cell_edit() {
        let mut tab = with_row(tab());
        assert_eq!(delete_row_refusal(Some(&tab), true), None);
        assert_eq!(
            cell_refusal(&tab, true),
            None,
            "no metadata yet: the read decides"
        );
        let budget = std::sync::Arc::new(crate::grid::Budget::new(1 << 20));
        let row = tab.result.table.rows[0].clone();
        let saved = crate::data_edit::SavedRow::reserve(row, &budget).unwrap();
        tab.result.edits.deletes.insert(0, saved);
        assert_eq!(
            cell_refusal(&tab, true),
            Some(text::EDIT_ROW_DELETED.to_owned())
        );
        assert!(has_pending(&tab));
    }
}
