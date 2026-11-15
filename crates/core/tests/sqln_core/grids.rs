use sqln_core::input::{Action, Grid, Region};

/// The grid actions that some grids do not have, with the grids that have them: the
/// results of a console, a table tab, and a compare tab.
const LIMITED: &[(Action, [bool; 3])] = &[
    (Action::GridCloseResult, [true, false, false]),
    (Action::GridKeepPrevious, [true, false, false]),
    (Action::GridComparePrevious, [true, false, false]),
    (Action::GridPreviousResult, [true, true, false]),
    (Action::GridNextResult, [true, true, false]),
    (Action::GridImport, [true, true, false]),
    (Action::GridApply, [false, true, false]),
    (Action::GridClearFilters, [false, true, false]),
    (Action::GridFocusFilter, [false, true, false]),
    (Action::GridFocusOrder, [false, true, false]),
    (Action::GridSortColumn, [false, true, false]),
    (Action::GridLoadMore, [false, true, false]),
    (Action::GridCountRows, [false, true, false]),
    (Action::GridEditCell, [false, true, false]),
    (Action::GridSetNull, [false, true, false]),
    (Action::GridNewRow, [false, true, false]),
    (Action::GridDeleteRow, [false, true, false]),
    (Action::GridReviewEdits, [false, true, false]),
    (Action::GridSaveEdits, [false, true, false]),
    (Action::GridDiscardEdits, [false, true, false]),
    (Action::GridStructure, [false, true, false]),
    (Action::GridFollowKey, [false, true, false]),
    (Action::GridCancelQuery, [false, true, false]),
    (Action::GridEndSession, [false, true, false]),
    (Action::GridReload, [false, true, true]),
    (Action::CompareScript, [false, false, true]),
    (Action::CompareCopy, [false, false, true]),
];

#[test]
fn each_grid_has_its_actions() {
    for action in Action::in_region(Region::Grid) {
        let expected = LIMITED
            .iter()
            .find(|(item, _)| *item == action)
            .map_or([true; 3], |(_, grids)| *grids);
        let found = [Grid::Console, Grid::Table, Grid::Compare].map(|grid| action.on_grid(grid));
        assert_eq!(found, expected, "{action:?}");
    }
}

#[test]
fn an_action_outside_the_grid_has_no_grid_limit() {
    for region in [Region::Global, Region::Editor, Region::Navigator] {
        for action in Action::in_region(region) {
            for grid in [Grid::Console, Grid::Table, Grid::Compare] {
                assert!(action.on_grid(grid), "{action:?}");
            }
        }
    }
}
