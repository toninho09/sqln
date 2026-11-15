//! The semantic styles of the interface. A screen chooses a meaning, not a theme field.

use ratatui::style::Style;

use super::super::theme::Theme;
use super::buttons::ButtonState;

/// The meaning of a short note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteLevel {
    Info,
    Success,
    Warning,
    Error,
    Hint,
}

/// The state of a row of a list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListRow {
    Selected,
    Normal,
}

pub fn note_style(level: NoteLevel, theme: &Theme) -> Style {
    match level {
        NoteLevel::Info | NoteLevel::Hint => theme.hint,
        NoteLevel::Success => theme.mark_good,
        NoteLevel::Warning => theme.mark_neutral,
        NoteLevel::Error => theme.error,
    }
}

pub fn list_row_style(state: ListRow, theme: &Theme) -> Style {
    match state {
        ListRow::Selected => theme.list_selected,
        ListRow::Normal => theme.list,
    }
}

pub fn field_style(focused: bool, theme: &Theme) -> Style {
    if focused {
        theme.field_focus
    } else {
        theme.field
    }
}

pub fn label_style(focused: bool, theme: &Theme) -> Style {
    if focused {
        theme.panel_title_focused
    } else {
        Style::new()
    }
}

pub fn title_style(focused: bool, theme: &Theme) -> Style {
    if focused {
        theme.panel_title_focused
    } else {
        theme.panel_title
    }
}

pub fn button_style(state: ButtonState, theme: &Theme) -> Style {
    super::buttons::style(state, theme)
}
