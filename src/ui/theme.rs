//! Styles. The themes of `schemes` use RGB colors, and they need a truecolor terminal.
//! The `ansi` theme uses the colors of the terminal profile. `NO_COLOR` gives the
//! `plain` theme, which has no color.

use std::collections::BTreeMap;

use ratatui::style::{Color, Modifier, Style};

use super::icons::Icons;
use super::schemes::{self, Scheme};
use crate::store::Scope;
use crate::store::config::{IconName, ThemeName};

/// A custom theme of the store, loaded from a file of one scope.
#[derive(Clone, Debug)]
pub struct CustomTheme {
    pub id: String,
    pub name: String,
    pub scheme: Scheme,
    pub scope: Scope,
    /// The hash of the file at the load. `None` for a theme that no file holds yet.
    pub file: Option<u64>,
}

impl CustomTheme {
    /// Builds a new theme that no file holds.
    pub fn new(id: String, name: String, scheme: Scheme, scope: Scope) -> Self {
        Self {
            id,
            name,
            scheme,
            scope,
            file: None,
        }
    }
}

/// The custom themes of both scopes, after the precedence. A project id wins over
/// a global id with the same name.
#[derive(Clone, Debug, Default)]
pub struct ThemeCatalog {
    themes: BTreeMap<String, CustomTheme>,
}

impl ThemeCatalog {
    /// Adds a theme. A later theme with the same id replaces it, so the project
    /// loads after the global.
    pub fn insert(&mut self, theme: CustomTheme) {
        self.themes.insert(theme.id.clone(), theme);
    }

    pub fn get(&self, id: &str) -> Option<&CustomTheme> {
        self.themes.get(id)
    }

    pub fn scheme(&self, id: &str) -> Option<Scheme> {
        self.themes.get(id).map(|theme| theme.scheme)
    }

    pub fn name_of(&self, id: &str) -> Option<&str> {
        self.themes.get(id).map(|theme| theme.name.as_str())
    }

    pub fn list(&self) -> impl Iterator<Item = &CustomTheme> {
        self.themes.values()
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.themes.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.themes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.themes.is_empty()
    }
}

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

impl Theme {
    /// Uses the same SQL token styles in the editor and in command messages.
    pub fn sql_token_style(
        &self,
        kind: crate::sql::TokenKind,
        word: &str,
        dialect: crate::sql::Dialect,
    ) -> Option<Style> {
        use crate::sql::TokenKind;
        match kind {
            TokenKind::Word if crate::complete::keywords::is_keyword(word, dialect) => {
                Some(self.keyword)
            }
            TokenKind::Number => Some(self.number),
            TokenKind::String => Some(self.string),
            TokenKind::QuotedIdentifier => Some(self.identifier),
            TokenKind::Comment => Some(self.comment),
            _ => None,
        }
    }

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

    /// Returns the theme of a name. `custom` gives the scheme of a custom id, and
    /// a missing theme falls back to `Auto`.
    pub fn named<F>(name: &ThemeName, custom: F, env: ColorEnv) -> Self
    where
        F: Fn(&str) -> Option<Scheme>,
    {
        match name {
            ThemeName::Auto => Self::automatic(env),
            ThemeName::Astrodark => Self::astrodark(),
            ThemeName::Ansi => Self::ansi(),
            ThemeName::TokyoNight => Self::from_scheme(&schemes::TOKYO_NIGHT),
            ThemeName::CatppuccinMocha => Self::from_scheme(&schemes::CATPPUCCIN_MOCHA),
            ThemeName::CatppuccinLatte => Self::from_scheme(&schemes::CATPPUCCIN_LATTE),
            ThemeName::Gruvbox => Self::from_scheme(&schemes::GRUVBOX),
            ThemeName::Nord => Self::from_scheme(&schemes::NORD),
            ThemeName::Dracula => Self::from_scheme(&schemes::DRACULA),
            ThemeName::OneDark => Self::from_scheme(&schemes::ONE_DARK),
            ThemeName::Monokai => Self::from_scheme(&schemes::MONOKAI),
            ThemeName::SolarizedDark => Self::from_scheme(&schemes::SOLARIZED_DARK),
            ThemeName::SolarizedLight => Self::from_scheme(&schemes::SOLARIZED_LIGHT),
            ThemeName::GithubDark => Self::from_scheme(&schemes::GITHUB_DARK),
            ThemeName::RosePine => Self::from_scheme(&schemes::ROSE_PINE),
            ThemeName::HighContrast => Self::from_scheme(&schemes::HIGH_CONTRAST),
            ThemeName::HighContrastLight => Self::from_scheme(&schemes::HIGH_CONTRAST_LIGHT),
            ThemeName::Everforest => Self::from_scheme(&schemes::EVERFOREST),
            ThemeName::Kanagawa => Self::from_scheme(&schemes::KANAGAWA),
            ThemeName::NightOwl => Self::from_scheme(&schemes::NIGHT_OWL),
            ThemeName::RosePineDawn => Self::from_scheme(&schemes::ROSE_PINE_DAWN),
            ThemeName::EverforestLight => Self::from_scheme(&schemes::EVERFOREST_LIGHT),
            ThemeName::Ember => Self::from_scheme(&schemes::EMBER),
            ThemeName::Custom(id) => custom(id)
                .map(|scheme| Self::from_scheme(&scheme))
                .unwrap_or_else(|| Self::automatic(env)),
        }
    }

    /// Applies the choices of the configuration to this theme. `NO_COLOR` has priority
    /// over the configuration, and a choice without a value keeps this theme.
    pub fn with_choices<F>(
        self,
        theme: Option<ThemeName>,
        custom: F,
        icons: Option<IconName>,
        env: ColorEnv,
    ) -> Self
    where
        F: Fn(&str) -> Option<Scheme>,
    {
        if env.no_color {
            return Self::plain();
        }
        let chosen = theme.map_or(self, |name| Self::named(&name, custom, env));
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
        Self::from_scheme(&schemes::ASTRODARK)
    }

    /// Gives each color of a scheme its places in the interface.
    pub fn from_scheme(s: &Scheme) -> Self {
        let color = |color| Style::new().fg(color);
        let fill = |background| Style::new().bg(background);
        let label = |background| {
            Style::new()
                .bg(background)
                .fg(s.label_text)
                .add_modifier(Modifier::BOLD)
        };
        let bold = Modifier::BOLD;
        let cursor = Style::new().bg(s.text).fg(s.base);
        Self {
            keyword: color(s.purple),
            string: color(s.green),
            number: color(s.orange),
            identifier: color(s.cyan),
            comment: color(s.comment).add_modifier(Modifier::ITALIC),
            gutter: color(s.inactive),
            cursor,
            selection: fill(s.selection),
            search_match: Style::new().bg(s.search).fg(s.strong),
            search_current: Style::new().bg(s.ui_yellow).fg(s.label_text),
            list: Style::new().bg(s.float).fg(s.text),
            list_selected: Style::new().bg(s.selection).fg(s.strong),
            list_detail: Style::new().bg(s.float).fg(s.comment),
            hint: color(s.comment),
            status: color(s.ui_yellow),
            bar: color(s.winbar),
            header: Style::new().bg(s.panel).fg(s.blue).add_modifier(bold),
            null: color(s.comment).add_modifier(Modifier::ITALIC),
            cell_cursor: cursor,
            cell_selected: fill(s.selection),
            tab: Style::new().bg(s.deep).fg(s.winbar),
            tab_active: Style::new().bg(s.base).fg(s.text).add_modifier(bold),
            tab_close: color(s.comment).remove_modifier(bold),
            run_active: label(s.ui_orange),
            run_done: label(s.ui_green),
            run_failed: label(s.ui_red),
            run_stopped: label(s.comment),
            mark_good: color(s.green),
            mark_bad: color(s.red),
            mark_neutral: color(s.ui_yellow),
            error: color(s.red),
            error_spot: color(s.red).add_modifier(Modifier::UNDERLINED),
            dialog: Style::new().bg(s.float).fg(s.text),
            dialog_option: Style::new().bg(s.highlight).fg(s.text),
            dialog_focus: Style::new()
                .bg(s.accent)
                .fg(s.label_text)
                .add_modifier(bold),
            tree_selected: Style::new().bg(s.selection).fg(s.strong),
            tree_detail: color(s.comment),
            divider: color(s.border),
            control: color(s.blue),
            control_off: color(s.inactive),
            base: Style::new().bg(s.base).fg(s.text),
            panel: Style::new().bg(s.panel).fg(s.ui_text),
            panel_title: color(s.comment).add_modifier(bold),
            panel_title_focused: color(s.accent).add_modifier(bold),
            tabline: fill(s.deep),
            tab_mark: Style::new().bg(s.base).fg(s.accent),
            result_tab: color(s.comment),
            result_tab_active: color(s.text).add_modifier(bold),
            tree_cursor: fill(s.highlight),
            statusline: Style::new().bg(s.deep).fg(s.ui_text),
            status_detail: Style::new().bg(s.deep).fg(s.comment),
            status_warning: Style::new().bg(s.deep).fg(s.ui_orange).add_modifier(bold),
            status_error: Style::new().bg(s.deep).fg(s.ui_red).add_modifier(bold),
            label_editor: label(s.accent),
            label_insert: label(s.ui_green),
            label_visual: label(s.ui_purple),
            label_results: label(s.ui_cyan),
            label_explorer: label(s.ui_orange),
            float_border: color(s.border),
            float_title: color(s.accent).add_modifier(bold),
            cursor_line: fill(s.current_line),
            gutter_current: color(s.text).add_modifier(bold),
            scrollbar_thumb: color(s.accent),
            scrollbar_track: color(s.highlight),
            row_number: color(s.inactive),
            grid_line: fill(s.current_line),
            grid_rule: color(s.highlight),
            field: fill(s.prompt),
            field_focus: Style::new().bg(s.selection).fg(s.strong),
            key: color(s.blue).add_modifier(bold),
            welcome_title: color(s.accent).add_modifier(bold),
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
