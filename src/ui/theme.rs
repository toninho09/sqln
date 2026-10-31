//! Styles. The interface uses ANSI colors only, and it respects `NO_COLOR`.

use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub keyword: Style,
    pub string: Style,
    pub number: Style,
    pub identifier: Style,
    pub comment: Style,
    pub gutter: Style,
    pub cursor: Style,
    pub selection: Style,
    pub search_match: Style,
    pub search_current: Style,
    pub list: Style,
    pub list_selected: Style,
    pub list_detail: Style,
    pub hint: Style,
    pub status: Style,
    pub bar: Style,
    pub header: Style,
    pub null: Style,
    pub cell_cursor: Style,
    pub cell_selected: Style,
    pub tab: Style,
    pub tab_active: Style,
    pub mark_good: Style,
    pub mark_bad: Style,
    pub mark_neutral: Style,
    pub error: Style,
    pub error_spot: Style,
    pub dialog: Style,
    pub dialog_option: Style,
    pub dialog_focus: Style,
    pub tree_selected: Style,
    pub tree_detail: Style,
    pub divider: Style,
    pub control: Style,
    pub control_off: Style,
}

impl Theme {
    pub fn from_env() -> Self {
        // The `NO_COLOR` convention ignores an empty value.
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        if no_color { Self::plain() } else { Self::ansi() }
    }

    pub fn ansi() -> Self {
        let color = |color| Style::new().fg(color);
        let reversed = Style::new().add_modifier(Modifier::REVERSED);
        Self {
            keyword: color(Color::Blue).add_modifier(Modifier::BOLD),
            string: color(Color::Green),
            number: color(Color::Magenta),
            identifier: color(Color::Cyan),
            comment: color(Color::DarkGray),
            gutter: color(Color::DarkGray),
            cursor: reversed,
            selection: Style::new().bg(Color::Blue).fg(Color::White),
            search_match: Style::new().bg(Color::Yellow).fg(Color::Black),
            search_current: Style::new().bg(Color::Green).fg(Color::Black),
            list: Style::new().bg(Color::DarkGray).fg(Color::White),
            list_selected: Style::new().bg(Color::Blue).fg(Color::White),
            list_detail: Style::new().bg(Color::DarkGray).fg(Color::Gray),
            hint: color(Color::DarkGray),
            status: color(Color::Yellow),
            bar: color(Color::Cyan),
            header: Style::new().add_modifier(Modifier::BOLD),
            null: color(Color::DarkGray),
            cell_cursor: reversed,
            cell_selected: Style::new().bg(Color::Blue).fg(Color::White),
            tab: color(Color::DarkGray),
            tab_active: Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            mark_good: color(Color::Green),
            mark_bad: color(Color::Red),
            mark_neutral: color(Color::Yellow),
            error: color(Color::Red),
            error_spot: color(Color::Red).add_modifier(Modifier::UNDERLINED),
            dialog: Style::new(),
            dialog_option: Style::new(),
            dialog_focus: reversed,
            tree_selected: reversed,
            tree_detail: color(Color::DarkGray),
            divider: color(Color::DarkGray),
            control: color(Color::Cyan),
            control_off: color(Color::DarkGray),
        }
    }

    pub fn plain() -> Self {
        let modifier = |modifier| Style::new().add_modifier(modifier);
        let none = Style::new();
        Self {
            keyword: modifier(Modifier::BOLD),
            string: none,
            number: none,
            identifier: none,
            comment: modifier(Modifier::DIM),
            gutter: modifier(Modifier::DIM),
            cursor: modifier(Modifier::REVERSED | Modifier::UNDERLINED),
            selection: modifier(Modifier::REVERSED),
            search_match: modifier(Modifier::UNDERLINED),
            search_current: modifier(Modifier::REVERSED | Modifier::BOLD),
            list: none,
            list_selected: modifier(Modifier::REVERSED),
            list_detail: modifier(Modifier::DIM),
            hint: modifier(Modifier::DIM),
            status: modifier(Modifier::BOLD),
            bar: none,
            header: modifier(Modifier::BOLD),
            null: modifier(Modifier::DIM),
            cell_cursor: modifier(Modifier::REVERSED),
            cell_selected: modifier(Modifier::UNDERLINED),
            tab: modifier(Modifier::DIM),
            tab_active: modifier(Modifier::BOLD | Modifier::UNDERLINED),
            mark_good: none,
            mark_bad: modifier(Modifier::BOLD),
            mark_neutral: none,
            error: modifier(Modifier::BOLD),
            error_spot: modifier(Modifier::UNDERLINED),
            dialog: none,
            dialog_option: none,
            dialog_focus: modifier(Modifier::REVERSED),
            tree_selected: modifier(Modifier::REVERSED),
            tree_detail: modifier(Modifier::DIM),
            divider: modifier(Modifier::DIM),
            control: none,
            control_off: modifier(Modifier::DIM),
        }
    }
}
