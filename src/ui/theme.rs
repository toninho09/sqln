//! Styles. The `astrodark` theme uses RGB colors, and it needs a truecolor terminal.
//! The `ansi` theme uses the colors of the terminal profile. `NO_COLOR` gives the
//! `plain` theme, which has no color.

use ratatui::style::{Color, Modifier, Style};

use super::icons::Icons;
use crate::store::config::{IconName, ThemeName};

/// What the terminal environment says about colors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColorEnv {
    pub no_color: bool,
    pub truecolor: bool,
}

impl ColorEnv {
    pub fn read() -> Self {
        // The `NO_COLOR` convention ignores an empty value.
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        let depth = std::env::var("COLORTERM")
            .unwrap_or_default()
            .to_ascii_lowercase();
        Self {
            no_color,
            truecolor: matches!(depth.as_str(), "truecolor" | "24bit"),
        }
    }
}

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
    /// The change of the tab style for the close mark.
    pub tab_close: Style,
    /// The marks of the state of a run in the tab row.
    pub run_active: Style,
    pub run_done: Style,
    pub run_failed: Style,
    pub run_stopped: Style,
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
    /// The whole screen, under all other styles.
    pub base: Style,
    /// The explorer, under its rows.
    pub panel: Style,
    pub panel_title: Style,
    pub panel_title_focused: Style,
    /// The row of the main tabs, under its tabs.
    pub tabline: Style,
    pub tab_mark: Style,
    pub result_tab: Style,
    pub result_tab_active: Style,
    /// The selected row of the explorer, when another panel has the focus.
    pub tree_cursor: Style,
    pub statusline: Style,
    pub status_detail: Style,
    pub status_warning: Style,
    pub status_error: Style,
    pub label_editor: Style,
    pub label_insert: Style,
    pub label_visual: Style,
    pub label_results: Style,
    pub label_explorer: Style,
    pub float_border: Style,
    pub float_title: Style,
    /// The text row of the cursor. It adds a background only.
    pub cursor_line: Style,
    pub gutter_current: Style,
    pub scrollbar_thumb: Style,
    pub scrollbar_track: Style,
    pub row_number: Style,
    /// The grid row of the cursor. It adds a background only.
    pub grid_line: Style,
    pub grid_rule: Style,
    pub field: Style,
    /// The value of the form row with the focus.
    pub field_focus: Style,
    /// A key in a menu.
    pub key: Style,
    pub welcome_title: Style,
    pub icons: Icons,
    /// The text on each side of a button label.
    pub button_marks: (&'static str, &'static str),
}

/// The colors of the `astrodark` palette of AstroTheme.
mod astro {
    use ratatui::style::Color;

    const fn rgb(value: u32) -> Color {
        Color::from_u32(value)
    }

    pub const RED: Color = rgb(0xFF838B);
    pub const ORANGE: Color = rgb(0xF5983A);
    pub const GREEN: Color = rgb(0x87C05F);
    pub const CYAN: Color = rgb(0x4AC2B8);
    pub const BLUE: Color = rgb(0x5EB7FF);
    pub const PURPLE: Color = rgb(0xDD97F1);
    pub const TEXT: Color = rgb(0xADB0BB);
    pub const COMMENT: Color = rgb(0x696C76);
    pub const UI_RED: Color = rgb(0xF8747E);
    pub const UI_ORANGE: Color = rgb(0xEB8332);
    pub const UI_YELLOW: Color = rgb(0xD09214);
    pub const UI_GREEN: Color = rgb(0x75AD47);
    pub const UI_CYAN: Color = rgb(0x00B298);
    pub const UI_PURPLE: Color = rgb(0xCC83E3);
    pub const ACCENT: Color = rgb(0x50A4E9);
    pub const BASE: Color = rgb(0x1A1D23);
    pub const PANEL: Color = rgb(0x16181D);
    pub const FLOAT: Color = rgb(0x14161B);
    pub const DARK: Color = rgb(0x111317);
    pub const BORDER: Color = rgb(0x3A3E47);
    pub const CURRENT_LINE: Color = rgb(0x1E222A);
    pub const SELECTION: Color = rgb(0x26343F);
    pub const HIGHLIGHT: Color = rgb(0x23272F);
    pub const PROMPT: Color = rgb(0x21242A);
    pub const UI_TEXT: Color = rgb(0x9B9FA9);
    pub const INACTIVE: Color = rgb(0x494D56);
    pub const WINBAR: Color = rgb(0x797D87);
    pub const MATCH: Color = rgb(0xE0E0EE);
    /// A dark tone of the palette yellow, for the background of a search match.
    pub const SEARCH: Color = rgb(0x4A3A12);
}

impl Theme {
    pub fn from_env() -> Self {
        Self::automatic(ColorEnv::read())
    }

    /// Returns the theme that the terminal supports.
    pub fn automatic(env: ColorEnv) -> Self {
        if env.no_color {
            Self::plain()
        } else if env.truecolor {
            Self::astrodark()
        } else {
            Self::ansi()
        }
    }

    /// Applies the choices of the configuration to this theme. `NO_COLOR` has priority
    /// over the configuration, and a choice without a value keeps this theme.
    pub fn with_choices(
        self,
        theme: Option<ThemeName>,
        icons: Option<IconName>,
        env: ColorEnv,
    ) -> Self {
        if env.no_color {
            return Self::plain();
        }
        let chosen = match theme {
            None => self,
            Some(ThemeName::Auto) => Self::automatic(env),
            Some(ThemeName::Astrodark) => Self::astrodark(),
            Some(ThemeName::Ansi) => Self::ansi(),
        };
        // The plain set stays, because that theme identifies an item by its text.
        match icons {
            Some(_) if chosen.icons == Icons::plain() => chosen,
            Some(IconName::Unicode) => Self {
                icons: Icons::unicode(),
                ..chosen
            },
            Some(IconName::Nerd) => Self {
                icons: Icons::nerd(),
                ..chosen
            },
            None => chosen,
        }
    }

    /// Writes a button of a dialog.
    pub fn button(&self, label: &str) -> String {
        format!("{}{label}{}", self.button_marks.0, self.button_marks.1)
    }

    /// Writes a button of a toolbar: the icon and the label, or the plain button.
    pub fn tool(&self, icon: &str, label: &str) -> String {
        if icon.is_empty() {
            self.button(label)
        } else {
            format!("{icon} {label}")
        }
    }

    /// Writes an icon and its space. A set without the icon writes nothing.
    pub fn icon(icon: &str) -> String {
        if icon.is_empty() {
            String::new()
        } else {
            format!("{icon} ")
        }
    }

    pub fn astrodark() -> Self {
        use astro::*;
        let color = |color| Style::new().fg(color);
        let fill = |background| Style::new().bg(background);
        let label = |background| {
            Style::new()
                .bg(background)
                .fg(DARK)
                .add_modifier(Modifier::BOLD)
        };
        let bold = Modifier::BOLD;
        let cursor = Style::new().bg(TEXT).fg(BASE);
        Self {
            keyword: color(PURPLE),
            string: color(GREEN),
            number: color(ORANGE),
            identifier: color(CYAN),
            comment: color(COMMENT).add_modifier(Modifier::ITALIC),
            gutter: color(INACTIVE),
            cursor,
            selection: fill(SELECTION),
            search_match: Style::new().bg(SEARCH).fg(MATCH),
            search_current: Style::new().bg(UI_YELLOW).fg(DARK),
            list: Style::new().bg(FLOAT).fg(TEXT),
            list_selected: Style::new().bg(SELECTION).fg(MATCH),
            list_detail: Style::new().bg(FLOAT).fg(COMMENT),
            hint: color(COMMENT),
            status: color(UI_YELLOW),
            bar: color(WINBAR),
            header: Style::new().bg(PANEL).fg(BLUE).add_modifier(bold),
            null: color(COMMENT).add_modifier(Modifier::ITALIC),
            cell_cursor: cursor,
            cell_selected: fill(SELECTION),
            tab: Style::new().bg(DARK).fg(WINBAR),
            tab_active: Style::new().bg(BASE).fg(TEXT).add_modifier(bold),
            tab_close: color(COMMENT).remove_modifier(bold),
            run_active: label(UI_ORANGE),
            run_done: label(UI_GREEN),
            run_failed: label(UI_RED),
            run_stopped: label(COMMENT),
            mark_good: color(GREEN),
            mark_bad: color(RED),
            mark_neutral: color(UI_YELLOW),
            error: color(RED),
            error_spot: color(RED).add_modifier(Modifier::UNDERLINED),
            dialog: Style::new().bg(FLOAT).fg(TEXT),
            dialog_option: Style::new().bg(HIGHLIGHT).fg(TEXT),
            dialog_focus: Style::new().bg(ACCENT).fg(DARK).add_modifier(bold),
            tree_selected: Style::new().bg(SELECTION).fg(MATCH),
            tree_detail: color(COMMENT),
            divider: color(BORDER),
            control: color(BLUE),
            control_off: color(INACTIVE),
            base: Style::new().bg(BASE).fg(TEXT),
            panel: Style::new().bg(PANEL).fg(UI_TEXT),
            panel_title: color(COMMENT).add_modifier(bold),
            panel_title_focused: color(ACCENT).add_modifier(bold),
            tabline: fill(DARK),
            tab_mark: Style::new().bg(BASE).fg(ACCENT),
            result_tab: color(COMMENT),
            result_tab_active: color(TEXT).add_modifier(bold),
            tree_cursor: fill(HIGHLIGHT),
            statusline: Style::new().bg(DARK).fg(UI_TEXT),
            status_detail: Style::new().bg(DARK).fg(COMMENT),
            status_warning: Style::new().bg(DARK).fg(UI_ORANGE).add_modifier(bold),
            status_error: Style::new().bg(DARK).fg(UI_RED).add_modifier(bold),
            label_editor: label(ACCENT),
            label_insert: label(UI_GREEN),
            label_visual: label(UI_PURPLE),
            label_results: label(UI_CYAN),
            label_explorer: label(UI_ORANGE),
            float_border: color(BORDER),
            float_title: color(ACCENT).add_modifier(bold),
            cursor_line: fill(CURRENT_LINE),
            gutter_current: color(TEXT).add_modifier(bold),
            scrollbar_thumb: color(ACCENT),
            scrollbar_track: color(HIGHLIGHT),
            row_number: color(INACTIVE),
            grid_line: fill(CURRENT_LINE),
            grid_rule: color(HIGHLIGHT),
            field: fill(PROMPT),
            field_focus: Style::new().bg(SELECTION).fg(MATCH),
            key: color(BLUE).add_modifier(bold),
            welcome_title: color(ACCENT).add_modifier(bold),
            icons: Icons::unicode(),
            button_marks: (" ", " "),
        }
    }

    pub fn ansi() -> Self {
        let color = |color| Style::new().fg(color);
        let reversed = Style::new().add_modifier(Modifier::REVERSED);
        let bold = Modifier::BOLD;
        let label = |background| {
            Style::new()
                .bg(background)
                .fg(Color::Black)
                .add_modifier(bold)
        };
        let bar = Style::new().bg(Color::DarkGray).fg(Color::White);
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
            tab: bar,
            tab_active: Style::new().add_modifier(bold),
            tab_close: Style::new(),
            run_active: label(Color::Yellow),
            run_done: label(Color::Green),
            run_failed: label(Color::Red),
            run_stopped: label(Color::Gray),
            mark_good: color(Color::Green),
            mark_bad: color(Color::Red),
            mark_neutral: color(Color::Yellow),
            error: color(Color::Red),
            error_spot: color(Color::Red).add_modifier(Modifier::UNDERLINED),
            dialog: Style::new(),
            dialog_option: reversed,
            dialog_focus: Style::new()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(bold),
            tree_selected: reversed,
            tree_detail: color(Color::DarkGray),
            divider: color(Color::DarkGray),
            control: color(Color::Cyan),
            control_off: color(Color::DarkGray),
            // These styles add no background, because a terminal profile can be light or dark.
            base: Style::new(),
            panel: Style::new(),
            panel_title: color(Color::DarkGray).add_modifier(bold),
            panel_title_focused: color(Color::LightBlue).add_modifier(bold),
            tabline: Style::new().bg(Color::DarkGray),
            tab_mark: color(Color::LightBlue),
            result_tab: color(Color::DarkGray),
            result_tab_active: Style::new().add_modifier(bold),
            tree_cursor: Style::new().add_modifier(Modifier::UNDERLINED),
            statusline: bar,
            status_detail: Style::new().bg(Color::DarkGray).fg(Color::Gray),
            status_warning: Style::new()
                .bg(Color::DarkGray)
                .fg(Color::Yellow)
                .add_modifier(bold),
            status_error: Style::new()
                .bg(Color::DarkGray)
                .fg(Color::LightRed)
                .add_modifier(bold),
            label_editor: label(Color::Blue),
            label_insert: label(Color::Green),
            label_visual: label(Color::Magenta),
            label_results: label(Color::Cyan),
            label_explorer: label(Color::Yellow),
            float_border: color(Color::DarkGray),
            float_title: color(Color::LightBlue).add_modifier(bold),
            cursor_line: Style::new(),
            gutter_current: Style::new().add_modifier(bold),
            scrollbar_thumb: color(Color::LightBlue),
            scrollbar_track: color(Color::DarkGray),
            row_number: color(Color::DarkGray),
            grid_line: Style::new(),
            grid_rule: color(Color::DarkGray),
            field: Style::new().add_modifier(Modifier::UNDERLINED),
            field_focus: Style::new().add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
            key: color(Color::LightBlue).add_modifier(bold),
            welcome_title: color(Color::LightBlue).add_modifier(bold),
            icons: Icons::unicode(),
            button_marks: (" ", " "),
        }
    }

    pub fn plain() -> Self {
        let modifier = |modifier| Style::new().add_modifier(modifier);
        let none = Style::new();
        let bold = modifier(Modifier::BOLD);
        let dim = modifier(Modifier::DIM);
        let reversed = modifier(Modifier::REVERSED);
        Self {
            keyword: bold,
            string: none,
            number: none,
            identifier: none,
            comment: dim,
            gutter: dim,
            cursor: modifier(Modifier::REVERSED | Modifier::UNDERLINED),
            selection: reversed,
            search_match: modifier(Modifier::UNDERLINED),
            search_current: modifier(Modifier::REVERSED | Modifier::BOLD),
            list: none,
            list_selected: reversed,
            list_detail: dim,
            hint: dim,
            status: bold,
            bar: none,
            header: bold,
            null: dim,
            cell_cursor: reversed,
            cell_selected: modifier(Modifier::UNDERLINED),
            tab: dim,
            tab_active: modifier(Modifier::BOLD | Modifier::UNDERLINED),
            tab_close: none,
            run_active: reversed,
            run_done: reversed,
            run_failed: reversed,
            run_stopped: reversed,
            mark_good: none,
            mark_bad: bold,
            mark_neutral: none,
            error: bold,
            error_spot: modifier(Modifier::UNDERLINED),
            dialog: none,
            dialog_option: none,
            dialog_focus: reversed,
            tree_selected: reversed,
            tree_detail: dim,
            divider: dim,
            control: none,
            control_off: dim,
            base: none,
            panel: none,
            panel_title: dim,
            panel_title_focused: bold,
            tabline: none,
            tab_mark: bold,
            result_tab: dim,
            result_tab_active: modifier(Modifier::BOLD | Modifier::UNDERLINED),
            tree_cursor: modifier(Modifier::UNDERLINED),
            statusline: none,
            status_detail: dim,
            status_warning: bold,
            status_error: modifier(Modifier::BOLD | Modifier::UNDERLINED),
            label_editor: reversed,
            label_insert: reversed,
            label_visual: reversed,
            label_results: reversed,
            label_explorer: reversed,
            float_border: none,
            float_title: bold,
            cursor_line: none,
            gutter_current: bold,
            scrollbar_thumb: none,
            scrollbar_track: dim,
            row_number: dim,
            grid_line: none,
            grid_rule: dim,
            field: none,
            field_focus: none,
            key: bold,
            welcome_title: bold,
            icons: Icons::plain(),
            button_marks: ("[", "]"),
        }
    }
}
