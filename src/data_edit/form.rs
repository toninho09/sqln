//! Modal state for a cell value or a read-only review of pending changes.

use crate::editor::SqlEditor;
use crate::exec::ConsoleId;
use crate::ui::wrap::WrappedText;

pub enum Body {
    Cell {
        row: usize,
        col: usize,
        editor: Box<SqlEditor>,
    },
    Review {
        contents: Box<WrappedText>,
        scroll: usize,
    },
}

pub struct EditForm {
    pub console: ConsoleId,
    pub title: String,
    pub body: Body,
    /// Zero selects the contents; the other values select buttons.
    pub focus: usize,
    pub keys: Vec<crate::input::Chord>,
    pub note: Option<String>,
    pub format_key: Option<String>,
}
