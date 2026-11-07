//! The shared pieces of the interface. One definition keeps the look the same on
//! every screen.

pub mod buttons;
pub mod feedback;
pub mod form;
pub mod geometry;
pub mod input;
pub mod list;
pub mod measure;
pub mod modal;
pub mod panel;
pub mod prompt;
pub mod style;

pub use geometry::{
    BORDER_ROWS, FOOTER_ROWS, LEFT, MODAL_FREE_COLUMNS, MODAL_FREE_ROWS, MODAL_WIDTH_LIMIT,
    NOTE_ROWS, RIGHT, SHORT_MODAL_WIDTH_LIMIT, SIDEBAR_WIDTH, centered, fitted, list_rows,
    modal_width, short_modal_width,
};
