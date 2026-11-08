//! The sizes and the placement of the shared pieces of the interface.

use ratatui::layout::Rect;

// The width of a modal.

pub const MODAL_FREE_COLUMNS: u16 = 8;
/// The rows that stay free above and below a modal that grows.
pub const MODAL_FREE_ROWS: u16 = 4;
pub const MODAL_WIDTH_LIMIT: u16 = 140;
pub const SHORT_MODAL_WIDTH_LIMIT: u16 = 100;
const LIST_ROWS_LIMIT: u16 = 30;

/// Returns a size that leaves `free` cells of the screen, from `base` to `limit`.
pub fn fitted(screen: u16, free: u16, base: u16, limit: u16) -> u16 {
    screen.saturating_sub(free).clamp(base, limit.max(base))
}

/// Returns the width of a modal. A long path needs the free columns of a wide screen.
pub fn modal_width(screen: u16, base: u16) -> u16 {
    fitted(screen, MODAL_FREE_COLUMNS, base, MODAL_WIDTH_LIMIT)
}

/// Returns the width of a modal with short rows, such as a list of names.
pub fn short_modal_width(screen: u16, base: u16) -> u16 {
    fitted(screen, MODAL_FREE_COLUMNS, base, SHORT_MODAL_WIDTH_LIMIT)
}

/// Returns the largest count of rows of a list. `chrome` is the other rows of its window.
pub fn list_rows(screen: u16, chrome: u16, base: u16) -> u16 {
    fitted(screen, MODAL_FREE_ROWS + chrome, base, LIST_ROWS_LIMIT)
}

pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

// The columns of a modal with a sidebar.

/// The columns of the sidebar, without the divider.
pub const SIDEBAR_WIDTH: u16 = 16;
/// The columns before a label: a space, the focus mark, and a space.
pub const LEFT: u16 = 3;
/// The empty columns after a value.
pub const RIGHT: u16 = 2;
/// The rows of the note above the buttons.
pub const NOTE_ROWS: u16 = 2;
/// The rows after the last field: a gap, the note, the buttons, and a gap.
pub const FOOTER_ROWS: u16 = NOTE_ROWS + 3;
/// The rows of the top and bottom borders.
pub const BORDER_ROWS: u16 = 2;
