//! The color schemes of the RGB themes. To add a theme, add a scheme here and a name in
//! `ThemeName`. The tests check the contrast of each scheme.

use ratatui::style::{Color, Style};

use crate::store::themes::{Colors, Hex};

const fn rgb(value: u32) -> Color {
    Color::from_u32(value)
}

fn color_of(hex: Hex) -> Color {
    Color::Rgb((hex.0 >> 16) as u8, (hex.0 >> 8) as u8, hex.0 as u8)
}

fn hex_of(color: Color) -> Hex {
    match color {
        Color::Rgb(red, green, blue) => {
            Hex(u32::from(red) << 16 | u32::from(green) << 8 | u32::from(blue))
        }
        _ => Hex(0),
    }
}

/// The `#RRGGBB` text of a color.
pub fn hex_text(color: Color) -> String {
    hex_of(color).text()
}

/// The color of `#RRGGBB` text.
pub fn color_of_text(text: &str) -> Option<Color> {
    crate::store::themes::parse_hex(text).map(|value| color_of(Hex(value)))
}

/// The colors of one theme. `Theme::from_scheme` gives each color its places.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scheme {
    pub red: Color,
    pub orange: Color,
    pub green: Color,
    pub cyan: Color,
    pub blue: Color,
    pub purple: Color,
    pub text: Color,
    pub comment: Color,
    /// The fills of the labels and of the marks of a run.
    pub ui_red: Color,
    pub ui_orange: Color,
    pub ui_yellow: Color,
    pub ui_green: Color,
    pub ui_cyan: Color,
    pub ui_purple: Color,
    pub accent: Color,
    /// The editor and the grid.
    pub base: Color,
    /// The explorer.
    pub panel: Color,
    /// The dialogs and the lists.
    pub float: Color,
    /// The tab row and the status line.
    pub deep: Color,
    /// The text on a label and on an accent fill.
    pub label_text: Color,
    pub border: Color,
    pub current_line: Color,
    pub selection: Color,
    pub highlight: Color,
    /// The field of a form.
    pub prompt: Color,
    /// The text of the explorer and of the status line.
    pub ui_text: Color,
    pub inactive: Color,
    /// The text of an inactive tab.
    pub winbar: Color,
    /// The text on a selection.
    pub strong: Color,
    /// The background of a search match.
    pub search: Color,
}

/// One color of a scheme, with its place in the palette.
pub struct ColorRole {
    pub label: &'static str,
    pub get: fn(&Scheme) -> Color,
    pub set: fn(&mut Scheme, Color),
}

/// The palette in the order of the theme editor.
pub const COLOR_ROLES: [ColorRole; 30] = [
    ColorRole {
        label: "red",
        get: |s| s.red,
        set: |s, c| s.red = c,
    },
    ColorRole {
        label: "orange",
        get: |s| s.orange,
        set: |s, c| s.orange = c,
    },
    ColorRole {
        label: "green",
        get: |s| s.green,
        set: |s, c| s.green = c,
    },
    ColorRole {
        label: "cyan",
        get: |s| s.cyan,
        set: |s, c| s.cyan = c,
    },
    ColorRole {
        label: "blue",
        get: |s| s.blue,
        set: |s, c| s.blue = c,
    },
    ColorRole {
        label: "purple",
        get: |s| s.purple,
        set: |s, c| s.purple = c,
    },
    ColorRole {
        label: "text",
        get: |s| s.text,
        set: |s, c| s.text = c,
    },
    ColorRole {
        label: "comment",
        get: |s| s.comment,
        set: |s, c| s.comment = c,
    },
    ColorRole {
        label: "ui red",
        get: |s| s.ui_red,
        set: |s, c| s.ui_red = c,
    },
    ColorRole {
        label: "ui orange",
        get: |s| s.ui_orange,
        set: |s, c| s.ui_orange = c,
    },
    ColorRole {
        label: "ui yellow",
        get: |s| s.ui_yellow,
        set: |s, c| s.ui_yellow = c,
    },
    ColorRole {
        label: "ui green",
        get: |s| s.ui_green,
        set: |s, c| s.ui_green = c,
    },
    ColorRole {
        label: "ui cyan",
        get: |s| s.ui_cyan,
        set: |s, c| s.ui_cyan = c,
    },
    ColorRole {
        label: "ui purple",
        get: |s| s.ui_purple,
        set: |s, c| s.ui_purple = c,
    },
    ColorRole {
        label: "accent",
        get: |s| s.accent,
        set: |s, c| s.accent = c,
    },
    ColorRole {
        label: "base",
        get: |s| s.base,
        set: |s, c| s.base = c,
    },
    ColorRole {
        label: "panel",
        get: |s| s.panel,
        set: |s, c| s.panel = c,
    },
    ColorRole {
        label: "float",
        get: |s| s.float,
        set: |s, c| s.float = c,
    },
    ColorRole {
        label: "deep",
        get: |s| s.deep,
        set: |s, c| s.deep = c,
    },
    ColorRole {
        label: "label text",
        get: |s| s.label_text,
        set: |s, c| s.label_text = c,
    },
    ColorRole {
        label: "border",
        get: |s| s.border,
        set: |s, c| s.border = c,
    },
    ColorRole {
        label: "current line",
        get: |s| s.current_line,
        set: |s, c| s.current_line = c,
    },
    ColorRole {
        label: "selection",
        get: |s| s.selection,
        set: |s, c| s.selection = c,
    },
    ColorRole {
        label: "highlight",
        get: |s| s.highlight,
        set: |s, c| s.highlight = c,
    },
    ColorRole {
        label: "prompt",
        get: |s| s.prompt,
        set: |s, c| s.prompt = c,
    },
    ColorRole {
        label: "ui text",
        get: |s| s.ui_text,
        set: |s, c| s.ui_text = c,
    },
    ColorRole {
        label: "inactive",
        get: |s| s.inactive,
        set: |s, c| s.inactive = c,
    },
    ColorRole {
        label: "tab text",
        get: |s| s.winbar,
        set: |s, c| s.winbar = c,
    },
    ColorRole {
        label: "selection text",
        get: |s| s.strong,
        set: |s, c| s.strong = c,
    },
    ColorRole {
        label: "search",
        get: |s| s.search,
        set: |s, c| s.search = c,
    },
];

impl Scheme {
    /// The built-in palette of a theme name, without `Auto`, `Ansi`, and customs.
    pub fn named(name: &crate::store::config::ThemeName) -> Option<Self> {
        use crate::store::config::ThemeName;
        match name {
            ThemeName::Astrodark => Some(ASTRODARK),
            ThemeName::TokyoNight => Some(TOKYO_NIGHT),
            ThemeName::CatppuccinMocha => Some(CATPPUCCIN_MOCHA),
            ThemeName::CatppuccinLatte => Some(CATPPUCCIN_LATTE),
            ThemeName::Gruvbox => Some(GRUVBOX),
            ThemeName::Nord => Some(NORD),
            ThemeName::Dracula => Some(DRACULA),
            ThemeName::OneDark => Some(ONE_DARK),
            ThemeName::Monokai => Some(MONOKAI),
            ThemeName::SolarizedDark => Some(SOLARIZED_DARK),
            ThemeName::SolarizedLight => Some(SOLARIZED_LIGHT),
            ThemeName::GithubDark => Some(GITHUB_DARK),
            ThemeName::RosePine => Some(ROSE_PINE),
            ThemeName::HighContrast => Some(HIGH_CONTRAST),
            ThemeName::HighContrastLight => Some(HIGH_CONTRAST_LIGHT),
            ThemeName::Everforest => Some(EVERFOREST),
            ThemeName::Kanagawa => Some(KANAGAWA),
            ThemeName::NightOwl => Some(NIGHT_OWL),
            ThemeName::RosePineDawn => Some(ROSE_PINE_DAWN),
            ThemeName::EverforestLight => Some(EVERFOREST_LIGHT),
            ThemeName::Ember => Some(EMBER),
            _ => None,
        }
    }

    /// Returns the color of one role of the editor.
    pub fn color(&self, role: usize) -> Color {
        (COLOR_ROLES[role].get)(self)
    }

    /// Changes one role of the editor.
    pub fn set_color(&mut self, role: usize, color: Color) {
        (COLOR_ROLES[role].set)(self, color)
    }

    /// The palette as a theme file holds it.
    pub fn to_colors(&self) -> Colors {
        Colors {
            red: hex_of(self.red),
            orange: hex_of(self.orange),
            green: hex_of(self.green),
            cyan: hex_of(self.cyan),
            blue: hex_of(self.blue),
            purple: hex_of(self.purple),
            text: hex_of(self.text),
            comment: hex_of(self.comment),
            ui_red: hex_of(self.ui_red),
            ui_orange: hex_of(self.ui_orange),
            ui_yellow: hex_of(self.ui_yellow),
            ui_green: hex_of(self.ui_green),
            ui_cyan: hex_of(self.ui_cyan),
            ui_purple: hex_of(self.ui_purple),
            accent: hex_of(self.accent),
            base: hex_of(self.base),
            panel: hex_of(self.panel),
            float: hex_of(self.float),
            deep: hex_of(self.deep),
            label_text: hex_of(self.label_text),
            border: hex_of(self.border),
            current_line: hex_of(self.current_line),
            selection: hex_of(self.selection),
            highlight: hex_of(self.highlight),
            prompt: hex_of(self.prompt),
            ui_text: hex_of(self.ui_text),
            inactive: hex_of(self.inactive),
            winbar: hex_of(self.winbar),
            strong: hex_of(self.strong),
            search: hex_of(self.search),
        }
    }

    /// The palette of a theme file.
    pub fn from_colors(colors: &Colors) -> Self {
        Self {
            red: color_of(colors.red),
            orange: color_of(colors.orange),
            green: color_of(colors.green),
            cyan: color_of(colors.cyan),
            blue: color_of(colors.blue),
            purple: color_of(colors.purple),
            text: color_of(colors.text),
            comment: color_of(colors.comment),
            ui_red: color_of(colors.ui_red),
            ui_orange: color_of(colors.ui_orange),
            ui_yellow: color_of(colors.ui_yellow),
            ui_green: color_of(colors.ui_green),
            ui_cyan: color_of(colors.ui_cyan),
            ui_purple: color_of(colors.ui_purple),
            accent: color_of(colors.accent),
            base: color_of(colors.base),
            panel: color_of(colors.panel),
            float: color_of(colors.float),
            deep: color_of(colors.deep),
            label_text: color_of(colors.label_text),
            border: color_of(colors.border),
            current_line: color_of(colors.current_line),
            selection: color_of(colors.selection),
            highlight: color_of(colors.highlight),
            prompt: color_of(colors.prompt),
            ui_text: color_of(colors.ui_text),
            inactive: color_of(colors.inactive),
            winbar: color_of(colors.winbar),
            strong: color_of(colors.strong),
            search: color_of(colors.search),
        }
    }
}

/// The minimum contrast of the WCAG formula: body text and marks.
const BODY_CONTRAST: f64 = 4.5;
/// The minimum contrast of a label or a short mark on a fill.
const SHORT_CONTRAST: f64 = 3.0;

/// Returns the contrast of two RGB colors, with the formula of WCAG.
fn contrast(first: Color, second: Color) -> f64 {
    let luminance = |color: Color| {
        let Color::Rgb(red, green, blue) = color else {
            return 0.0;
        };
        let channel = |value: u8| {
            let value = f64::from(value) / 255.0;
            if value <= 0.03928 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue)
    };
    let (first, second) = (luminance(first), luminance(second));
    (first.max(second) + 0.05) / (first.min(second) + 0.05)
}

/// Lists the places of the palette below the minimum contrast. The theme editor
/// shows them as a warning, and the tests check that each built-in scheme has none.
pub fn contrast_failures(s: &Scheme) -> Vec<&'static str> {
    let theme = crate::ui::theme::Theme::from_scheme(s);
    let mut failures = Vec::new();
    // A style without its own background sits on the base of the scheme.
    let mut check = |name: &'static str, style: Style, minimum: f64| {
        let Some(foreground) = style.fg else {
            return;
        };
        let background = style.bg.unwrap_or(s.base);
        if contrast(foreground, background) < minimum {
            failures.push(name);
        }
    };
    // Text and marks with a fill of their own.
    for (name, style) in [
        ("base", theme.base),
        ("panel", theme.panel),
        ("dialog", theme.dialog),
        ("list", theme.list),
        ("list selected", theme.list_selected),
        ("tree selected", theme.tree_selected),
        ("field focus", theme.field_focus),
        ("statusline", theme.statusline),
        ("search match", theme.search_match),
    ] {
        check(name, style, BODY_CONTRAST);
    }
    // Short labels and marks on their fills.
    for (name, style) in [
        ("list detail", theme.list_detail),
        ("status detail", theme.status_detail),
        ("status warning", theme.status_warning),
        ("status error", theme.status_error),
        ("tab", theme.tab),
        ("tab active", theme.tab_active),
        ("run active", theme.run_active),
        ("run done", theme.run_done),
        ("run failed", theme.run_failed),
        ("run stopped", theme.run_stopped),
        ("label editor", theme.label_editor),
        ("label insert", theme.label_insert),
        ("label visual", theme.label_visual),
        ("label results", theme.label_results),
        ("label explorer", theme.label_explorer),
        ("dialog focus", theme.dialog_focus),
        ("search current", theme.search_current),
        ("header", theme.header),
    ] {
        check(name, style, SHORT_CONTRAST);
    }
    // Marks and short text on the base.
    for (name, style) in [
        ("keyword", theme.keyword),
        ("string", theme.string),
        ("number", theme.number),
        ("identifier", theme.identifier),
        ("comment", theme.comment),
        ("hint", theme.hint),
        ("status", theme.status),
        ("error", theme.error),
        ("mark good", theme.mark_good),
        ("mark bad", theme.mark_bad),
        ("mark neutral", theme.mark_neutral),
        ("control", theme.control),
        ("key", theme.key),
        ("panel title", theme.panel_title_focused),
    ] {
        check(name, style, SHORT_CONTRAST);
    }
    failures
}

/// The `astrodark` palette of AstroTheme.
pub const ASTRODARK: Scheme = Scheme {
    red: rgb(0xFF838B),
    orange: rgb(0xF5983A),
    green: rgb(0x87C05F),
    cyan: rgb(0x4AC2B8),
    blue: rgb(0x5EB7FF),
    purple: rgb(0xDD97F1),
    text: rgb(0xADB0BB),
    comment: rgb(0x696C76),
    ui_red: rgb(0xF8747E),
    ui_orange: rgb(0xEB8332),
    ui_yellow: rgb(0xD09214),
    ui_green: rgb(0x75AD47),
    ui_cyan: rgb(0x00B298),
    ui_purple: rgb(0xCC83E3),
    accent: rgb(0x50A4E9),
    base: rgb(0x1A1D23),
    panel: rgb(0x16181D),
    float: rgb(0x14161B),
    deep: rgb(0x111317),
    label_text: rgb(0x111317),
    border: rgb(0x3A3E47),
    current_line: rgb(0x1E222A),
    selection: rgb(0x26343F),
    highlight: rgb(0x23272F),
    prompt: rgb(0x21242A),
    ui_text: rgb(0x9B9FA9),
    inactive: rgb(0x494D56),
    winbar: rgb(0x797D87),
    strong: rgb(0xE0E0EE),
    search: rgb(0x4A3A12),
};

/// The `night` style of Tokyo Night. A lighter comment keeps the hints readable.
pub const TOKYO_NIGHT: Scheme = Scheme {
    red: rgb(0xF7768E),
    orange: rgb(0xFF9E64),
    green: rgb(0x9ECE6A),
    cyan: rgb(0x7DCFFF),
    blue: rgb(0x7AA2F7),
    purple: rgb(0xBB9AF7),
    text: rgb(0xC0CAF5),
    comment: rgb(0x737AA2),
    ui_red: rgb(0xF7768E),
    ui_orange: rgb(0xFF9E64),
    ui_yellow: rgb(0xE0AF68),
    ui_green: rgb(0x9ECE6A),
    ui_cyan: rgb(0x1ABC9C),
    ui_purple: rgb(0xBB9AF7),
    accent: rgb(0x7AA2F7),
    base: rgb(0x1A1B26),
    panel: rgb(0x16161E),
    float: rgb(0x13131A),
    deep: rgb(0x0F0F14),
    label_text: rgb(0x0F0F14),
    border: rgb(0x3B4261),
    current_line: rgb(0x1F2335),
    selection: rgb(0x283457),
    highlight: rgb(0x292E42),
    prompt: rgb(0x24283B),
    ui_text: rgb(0xA9B1D6),
    inactive: rgb(0x414868),
    winbar: rgb(0x8089B3),
    strong: rgb(0xE1E6FF),
    search: rgb(0x3D59A1),
};

/// The Mocha flavor of Catppuccin.
pub const CATPPUCCIN_MOCHA: Scheme = Scheme {
    red: rgb(0xF38BA8),
    orange: rgb(0xFAB387),
    green: rgb(0xA6E3A1),
    cyan: rgb(0x94E2D5),
    blue: rgb(0x89B4FA),
    purple: rgb(0xCBA6F7),
    text: rgb(0xCDD6F4),
    comment: rgb(0x7F849C),
    ui_red: rgb(0xF38BA8),
    ui_orange: rgb(0xFAB387),
    ui_yellow: rgb(0xF9E2AF),
    ui_green: rgb(0xA6E3A1),
    ui_cyan: rgb(0x94E2D5),
    ui_purple: rgb(0xCBA6F7),
    accent: rgb(0x89B4FA),
    base: rgb(0x1E1E2E),
    panel: rgb(0x181825),
    float: rgb(0x14141F),
    deep: rgb(0x11111B),
    label_text: rgb(0x11111B),
    border: rgb(0x45475A),
    current_line: rgb(0x2A2B3C),
    selection: rgb(0x45475A),
    highlight: rgb(0x313244),
    prompt: rgb(0x313244),
    ui_text: rgb(0xBAC2DE),
    inactive: rgb(0x585B70),
    winbar: rgb(0x9399B2),
    strong: rgb(0xF5E0DC),
    search: rgb(0x4D4733),
};

/// The Latte flavor of Catppuccin, the light theme. Darker text colors and softer fills
/// keep the contrast on a light background.
pub const CATPPUCCIN_LATTE: Scheme = Scheme {
    red: rgb(0xD20F39),
    orange: rgb(0xCA4A00),
    green: rgb(0x2F7D1E),
    cyan: rgb(0x0F7A80),
    blue: rgb(0x1E66F5),
    purple: rgb(0x8839EF),
    text: rgb(0x4C4F69),
    comment: rgb(0x6C6F85),
    ui_red: rgb(0xE0475F),
    ui_orange: rgb(0xBC4F00),
    ui_yellow: rgb(0x9A6700),
    ui_green: rgb(0x4FAE3A),
    ui_cyan: rgb(0x179299),
    ui_purple: rgb(0xA36BF2),
    accent: rgb(0x1E66F5),
    base: rgb(0xEFF1F5),
    panel: rgb(0xE6E9EF),
    float: rgb(0xE6E9EF),
    deep: rgb(0xDCE0E8),
    label_text: rgb(0x11111B),
    border: rgb(0xBCC0CC),
    current_line: rgb(0xE6E9EF),
    selection: rgb(0xCCD0DA),
    highlight: rgb(0xDCE0E8),
    prompt: rgb(0xDCE0E8),
    ui_text: rgb(0x5C5F77),
    inactive: rgb(0x9CA0B0),
    winbar: rgb(0x6C6F85),
    strong: rgb(0x1E1E2E),
    search: rgb(0xF6E3B8),
};

/// Gruvbox dark, with the warm tones of its bright colors.
pub const GRUVBOX: Scheme = Scheme {
    red: rgb(0xFB4934),
    orange: rgb(0xFE8019),
    green: rgb(0xB8BB26),
    cyan: rgb(0x8EC07C),
    blue: rgb(0x83A598),
    purple: rgb(0xD3869B),
    text: rgb(0xEBDBB2),
    comment: rgb(0x928374),
    ui_red: rgb(0xFB4934),
    ui_orange: rgb(0xFE8019),
    ui_yellow: rgb(0xFABD2F),
    ui_green: rgb(0xB8BB26),
    ui_cyan: rgb(0x8EC07C),
    ui_purple: rgb(0xD3869B),
    accent: rgb(0xFABD2F),
    base: rgb(0x282828),
    panel: rgb(0x1D2021),
    float: rgb(0x1A1C1D),
    deep: rgb(0x141617),
    label_text: rgb(0x141617),
    border: rgb(0x504945),
    current_line: rgb(0x32302F),
    selection: rgb(0x504945),
    highlight: rgb(0x3C3836),
    prompt: rgb(0x3C3836),
    ui_text: rgb(0xD5C4A1),
    inactive: rgb(0x665C54),
    winbar: rgb(0xA89984),
    strong: rgb(0xFBF1C7),
    search: rgb(0x5A4A1A),
};

/// Nord. A lighter red and a lighter comment keep the text readable on its gray base.
pub const NORD: Scheme = Scheme {
    red: rgb(0xD57780),
    orange: rgb(0xD08770),
    green: rgb(0xA3BE8C),
    cyan: rgb(0x8FBCBB),
    blue: rgb(0x81A1C1),
    purple: rgb(0xB48EAD),
    text: rgb(0xD8DEE9),
    comment: rgb(0x7B88A1),
    ui_red: rgb(0xBF616A),
    ui_orange: rgb(0xD08770),
    ui_yellow: rgb(0xEBCB8B),
    ui_green: rgb(0xA3BE8C),
    ui_cyan: rgb(0x88C0D0),
    ui_purple: rgb(0xB48EAD),
    accent: rgb(0x88C0D0),
    base: rgb(0x2E3440),
    panel: rgb(0x292E39),
    float: rgb(0x262B35),
    deep: rgb(0x22262E),
    label_text: rgb(0x22262E),
    border: rgb(0x4C566A),
    current_line: rgb(0x3B4252),
    selection: rgb(0x434C5E),
    highlight: rgb(0x3B4252),
    prompt: rgb(0x3B4252),
    ui_text: rgb(0xC2CAD7),
    inactive: rgb(0x5A657B),
    winbar: rgb(0x8A96AD),
    strong: rgb(0xECEFF4),
    search: rgb(0x5D5540),
};

/// The Dracula palette. A lighter comment keeps the hints readable on the dark base.
pub const DRACULA: Scheme = Scheme {
    red: rgb(0xFF5555),
    orange: rgb(0xFFB86C),
    green: rgb(0x50FA7B),
    cyan: rgb(0x8BE9FD),
    blue: rgb(0x8BE9FD),
    purple: rgb(0xBD93F9),
    text: rgb(0xF8F8F2),
    comment: rgb(0x7F87B8),
    ui_red: rgb(0xFF5555),
    ui_orange: rgb(0xFFB86C),
    ui_yellow: rgb(0xF1FA8C),
    ui_green: rgb(0x50FA7B),
    ui_cyan: rgb(0x8BE9FD),
    ui_purple: rgb(0xBD93F9),
    accent: rgb(0xBD93F9),
    base: rgb(0x282A36),
    panel: rgb(0x21222C),
    float: rgb(0x1E1F29),
    deep: rgb(0x191A21),
    label_text: rgb(0x282A36),
    border: rgb(0x44475A),
    current_line: rgb(0x44475A),
    selection: rgb(0x44475A),
    highlight: rgb(0x343746),
    prompt: rgb(0x343746),
    ui_text: rgb(0xF8F8F2),
    inactive: rgb(0x6272A4),
    winbar: rgb(0xB8BFDE),
    strong: rgb(0xF8F8F2),
    search: rgb(0x44475A),
};

/// The One Dark palette of the Atom editor.
pub const ONE_DARK: Scheme = Scheme {
    red: rgb(0xE06C75),
    orange: rgb(0xD19A66),
    green: rgb(0x98C379),
    cyan: rgb(0x56B6C2),
    blue: rgb(0x61AFEF),
    purple: rgb(0xC678DD),
    text: rgb(0xABB2BF),
    comment: rgb(0x7F848E),
    ui_red: rgb(0xE06C75),
    ui_orange: rgb(0xD19A66),
    ui_yellow: rgb(0xE5C07B),
    ui_green: rgb(0x98C379),
    ui_cyan: rgb(0x56B6C2),
    ui_purple: rgb(0xC678DD),
    accent: rgb(0x61AFEF),
    base: rgb(0x282C34),
    panel: rgb(0x21252B),
    float: rgb(0x21252B),
    deep: rgb(0x1B1F24),
    label_text: rgb(0x1B1F24),
    border: rgb(0x3E4451),
    current_line: rgb(0x2C313A),
    selection: rgb(0x3E4451),
    highlight: rgb(0x323842),
    prompt: rgb(0x3A3F4B),
    ui_text: rgb(0xBFC6D2),
    inactive: rgb(0x5C6370),
    winbar: rgb(0x9DA5B4),
    strong: rgb(0xE6E6E6),
    search: rgb(0x3E4B5C),
};

/// The classic Monokai palette. A lighter comment keeps the hints readable.
pub const MONOKAI: Scheme = Scheme {
    red: rgb(0xF92672),
    orange: rgb(0xFD971F),
    green: rgb(0xA6E22E),
    cyan: rgb(0x66D9EF),
    blue: rgb(0x66D9EF),
    purple: rgb(0xAE81FF),
    text: rgb(0xF8F8F2),
    comment: rgb(0x89846D),
    ui_red: rgb(0xF92672),
    ui_orange: rgb(0xFD971F),
    ui_yellow: rgb(0xE6DB74),
    ui_green: rgb(0xA6E22E),
    ui_cyan: rgb(0x66D9EF),
    ui_purple: rgb(0xAE81FF),
    accent: rgb(0xAE81FF),
    base: rgb(0x272822),
    panel: rgb(0x1E1F1C),
    float: rgb(0x1E1F1C),
    deep: rgb(0x191A17),
    label_text: rgb(0x272822),
    border: rgb(0x49483E),
    current_line: rgb(0x3E3D32),
    selection: rgb(0x49483E),
    highlight: rgb(0x3E3D32),
    prompt: rgb(0x3E3D32),
    ui_text: rgb(0xF8F8F2),
    inactive: rgb(0x75715E),
    winbar: rgb(0xA8A494),
    strong: rgb(0xF8F8F2),
    search: rgb(0x5C5A3F),
};

/// The Solarized Dark palette. The brighter red and orange keep the marks readable.
pub const SOLARIZED_DARK: Scheme = Scheme {
    red: rgb(0xE35D5B),
    orange: rgb(0xE06A3A),
    green: rgb(0x859900),
    cyan: rgb(0x2AA198),
    blue: rgb(0x268BD2),
    purple: rgb(0x8A8FE0),
    text: rgb(0x93A1A1),
    comment: rgb(0x7E969C),
    ui_red: rgb(0xE0514E),
    ui_orange: rgb(0xE06A3A),
    ui_yellow: rgb(0xC99A00),
    ui_green: rgb(0x859900),
    ui_cyan: rgb(0x2AA198),
    ui_purple: rgb(0x8A8FE0),
    accent: rgb(0x268BD2),
    base: rgb(0x002B36),
    panel: rgb(0x00252F),
    float: rgb(0x001F27),
    deep: rgb(0x001A21),
    label_text: rgb(0x002B36),
    border: rgb(0x15505C),
    current_line: rgb(0x073642),
    selection: rgb(0x0D4A56),
    highlight: rgb(0x073642),
    prompt: rgb(0x073642),
    ui_text: rgb(0x93A1A1),
    inactive: rgb(0x586E75),
    winbar: rgb(0x839496),
    strong: rgb(0xFDF6E3),
    search: rgb(0x0C5566),
};

/// The Solarized Light palette. Darker accents keep the light base readable.
pub const SOLARIZED_LIGHT: Scheme = Scheme {
    red: rgb(0xC22926),
    orange: rgb(0xB44A10),
    green: rgb(0x6E8200),
    cyan: rgb(0x1B7F78),
    blue: rgb(0x1F6FA8),
    purple: rgb(0x5A5FB8),
    text: rgb(0x4F656C),
    comment: rgb(0x667A80),
    ui_red: rgb(0xC22926),
    ui_orange: rgb(0xB44A10),
    ui_yellow: rgb(0x8A6500),
    ui_green: rgb(0x6E8200),
    ui_cyan: rgb(0x1B7F78),
    ui_purple: rgb(0x5A5FB8),
    accent: rgb(0x1F6FA8),
    base: rgb(0xFDF6E3),
    panel: rgb(0xEEE8D5),
    float: rgb(0xEEE8D5),
    deep: rgb(0xE6DFC9),
    label_text: rgb(0xFDF6E3),
    border: rgb(0xC9C2AE),
    current_line: rgb(0xF0EAD8),
    selection: rgb(0xD8D2BE),
    highlight: rgb(0xE9E2CF),
    prompt: rgb(0xE9E2CF),
    ui_text: rgb(0x4A5F66),
    inactive: rgb(0x93A1A1),
    winbar: rgb(0x586E75),
    strong: rgb(0x002B36),
    search: rgb(0xFFE9A8),
};

/// The GitHub Dark palette, with the bright syntax colors of the site.
pub const GITHUB_DARK: Scheme = Scheme {
    red: rgb(0xFF7B72),
    orange: rgb(0xFFA657),
    green: rgb(0x7EE787),
    cyan: rgb(0x76E3EA),
    blue: rgb(0x79C0FF),
    purple: rgb(0xD2A8FF),
    text: rgb(0xC9D1D9),
    comment: rgb(0x8B949E),
    ui_red: rgb(0xF85149),
    ui_orange: rgb(0xDB6D28),
    ui_yellow: rgb(0xD29922),
    ui_green: rgb(0x3FB950),
    ui_cyan: rgb(0x39C5CF),
    ui_purple: rgb(0xBC8CFF),
    accent: rgb(0x58A6FF),
    base: rgb(0x0D1117),
    panel: rgb(0x161B22),
    float: rgb(0x161B22),
    deep: rgb(0x010409),
    label_text: rgb(0x0D1117),
    border: rgb(0x30363D),
    current_line: rgb(0x161B22),
    selection: rgb(0x264F78),
    highlight: rgb(0x21262D),
    prompt: rgb(0x21262D),
    ui_text: rgb(0xC9D1D9),
    inactive: rgb(0x6E7681),
    winbar: rgb(0x8B949E),
    strong: rgb(0xF0F6FC),
    search: rgb(0x1F4A75),
};

/// The Rose Pine palette, with its muted backgrounds and soft colors.
pub const ROSE_PINE: Scheme = Scheme {
    red: rgb(0xEB6F92),
    orange: rgb(0xF0A36B),
    green: rgb(0x9DD4A6),
    cyan: rgb(0x9CCFD8),
    blue: rgb(0x7AA2D6),
    purple: rgb(0xC4A7E7),
    text: rgb(0xE0DEF4),
    comment: rgb(0x908CAA),
    ui_red: rgb(0xEB6F92),
    ui_orange: rgb(0xF0A36B),
    ui_yellow: rgb(0xF6C177),
    ui_green: rgb(0x9DD4A6),
    ui_cyan: rgb(0x9CCFD8),
    ui_purple: rgb(0xC4A7E7),
    accent: rgb(0xC4A7E7),
    base: rgb(0x191724),
    panel: rgb(0x1F1D2E),
    float: rgb(0x1F1D2E),
    deep: rgb(0x14121E),
    label_text: rgb(0x191724),
    border: rgb(0x403D52),
    current_line: rgb(0x26233A),
    selection: rgb(0x403D52),
    highlight: rgb(0x26233A),
    prompt: rgb(0x26233A),
    ui_text: rgb(0xE0DEF4),
    inactive: rgb(0x6E6A86),
    winbar: rgb(0x908CAA),
    strong: rgb(0xE0DEF4),
    search: rgb(0x3A3552),
};

/// A high-contrast dark palette: a black base and bright marks.
pub const HIGH_CONTRAST: Scheme = Scheme {
    red: rgb(0xFF6B6B),
    orange: rgb(0xFFA94D),
    green: rgb(0x69DB7C),
    cyan: rgb(0x4DD4E8),
    blue: rgb(0x74C0FC),
    purple: rgb(0xDA77F2),
    text: rgb(0xFFFFFF),
    comment: rgb(0xC8C8C8),
    ui_red: rgb(0xFF6B6B),
    ui_orange: rgb(0xFFA94D),
    ui_yellow: rgb(0xFFD43B),
    ui_green: rgb(0x69DB7C),
    ui_cyan: rgb(0x4DD4E8),
    ui_purple: rgb(0xDA77F2),
    accent: rgb(0x4DABF7),
    base: rgb(0x000000),
    panel: rgb(0x0A0A0A),
    float: rgb(0x000000),
    deep: rgb(0x000000),
    label_text: rgb(0x000000),
    border: rgb(0x5C5C5C),
    current_line: rgb(0x141414),
    selection: rgb(0x1A4A6B),
    highlight: rgb(0x1A1A1A),
    prompt: rgb(0x1A1A1A),
    ui_text: rgb(0xFFFFFF),
    inactive: rgb(0x9A9A9A),
    winbar: rgb(0xD0D0D0),
    strong: rgb(0xFFFFFF),
    search: rgb(0x6B4A00),
};

/// A high-contrast light palette: a white base and dark marks.
pub const HIGH_CONTRAST_LIGHT: Scheme = Scheme {
    red: rgb(0xC92A2A),
    orange: rgb(0xB35C00),
    green: rgb(0x2B7A0B),
    cyan: rgb(0x0B7285),
    blue: rgb(0x1864AB),
    purple: rgb(0x7048E8),
    text: rgb(0x000000),
    comment: rgb(0x6C757D),
    ui_red: rgb(0xC92A2A),
    ui_orange: rgb(0xB35C00),
    ui_yellow: rgb(0x8A6D00),
    ui_green: rgb(0x2B7A0B),
    ui_cyan: rgb(0x0B7285),
    ui_purple: rgb(0x7048E8),
    accent: rgb(0x1864AB),
    base: rgb(0xFFFFFF),
    panel: rgb(0xF5F5F5),
    float: rgb(0xFFFFFF),
    deep: rgb(0xE8E8E8),
    label_text: rgb(0x000000),
    border: rgb(0xADB5BD),
    current_line: rgb(0xF1F3F5),
    selection: rgb(0xD0E2FF),
    highlight: rgb(0xE9ECEF),
    prompt: rgb(0xF1F3F5),
    ui_text: rgb(0x000000),
    inactive: rgb(0x6C757D),
    winbar: rgb(0x495057),
    strong: rgb(0x000000),
    search: rgb(0xFFE066),
};

/// The Everforest dark palette, with a green-tinted base.
pub const EVERFOREST: Scheme = Scheme {
    red: rgb(0xE67E80),
    orange: rgb(0xE69875),
    green: rgb(0xA7C080),
    cyan: rgb(0x83C092),
    blue: rgb(0x7FBBB3),
    purple: rgb(0xD699B6),
    text: rgb(0xD3C6AA),
    comment: rgb(0x859289),
    ui_red: rgb(0xE67E80),
    ui_orange: rgb(0xE69875),
    ui_yellow: rgb(0xDBBC7F),
    ui_green: rgb(0xA7C080),
    ui_cyan: rgb(0x83C092),
    ui_purple: rgb(0xD699B6),
    accent: rgb(0xA7C080),
    base: rgb(0x232A2E),
    panel: rgb(0x1E2428),
    float: rgb(0x1B2125),
    deep: rgb(0x171C20),
    label_text: rgb(0x232A2E),
    border: rgb(0x475258),
    current_line: rgb(0x2D353B),
    selection: rgb(0x3D484D),
    highlight: rgb(0x343F44),
    prompt: rgb(0x343F44),
    ui_text: rgb(0xD3C6AA),
    inactive: rgb(0x7A8478),
    winbar: rgb(0x9DA9A0),
    strong: rgb(0xEFEBD4),
    search: rgb(0x4C5A44),
};

/// The Kanagawa Wave palette, with an ink-blue base.
pub const KANAGAWA: Scheme = Scheme {
    red: rgb(0xE46876),
    orange: rgb(0xFFA066),
    green: rgb(0x98BB6C),
    cyan: rgb(0x7AA89F),
    blue: rgb(0x7E9CD8),
    purple: rgb(0x957FB8),
    text: rgb(0xDCD7BA),
    comment: rgb(0x8A8980),
    ui_red: rgb(0xE46876),
    ui_orange: rgb(0xFFA066),
    ui_yellow: rgb(0xE6C384),
    ui_green: rgb(0x98BB6C),
    ui_cyan: rgb(0x7AA89F),
    ui_purple: rgb(0x957FB8),
    accent: rgb(0x7E9CD8),
    base: rgb(0x1F1F28),
    panel: rgb(0x181820),
    float: rgb(0x16161D),
    deep: rgb(0x121218),
    label_text: rgb(0x1F1F28),
    border: rgb(0x54546D),
    current_line: rgb(0x2A2A37),
    selection: rgb(0x2D4F67),
    highlight: rgb(0x363646),
    prompt: rgb(0x363646),
    ui_text: rgb(0xDCD7BA),
    inactive: rgb(0x727169),
    winbar: rgb(0xC8C093),
    strong: rgb(0xDCD7BA),
    search: rgb(0x3A4A5A),
};

/// The Night Owl palette, with a deep navy base.
pub const NIGHT_OWL: Scheme = Scheme {
    red: rgb(0xEF5350),
    orange: rgb(0xF78C6C),
    green: rgb(0x22DA6E),
    cyan: rgb(0x21C7A8),
    blue: rgb(0x82AAFF),
    purple: rgb(0xC792EA),
    text: rgb(0xD6DEEB),
    comment: rgb(0x637777),
    ui_red: rgb(0xEF5350),
    ui_orange: rgb(0xF78C6C),
    ui_yellow: rgb(0xFFCB6B),
    ui_green: rgb(0x22DA6E),
    ui_cyan: rgb(0x21C7A8),
    ui_purple: rgb(0xC792EA),
    accent: rgb(0x82AAFF),
    base: rgb(0x011627),
    panel: rgb(0x01111D),
    float: rgb(0x010E17),
    deep: rgb(0x010B12),
    label_text: rgb(0x011627),
    border: rgb(0x1D3B53),
    current_line: rgb(0x0B2942),
    selection: rgb(0x1D3B53),
    highlight: rgb(0x0B2942),
    prompt: rgb(0x0B2942),
    ui_text: rgb(0xD6DEEB),
    inactive: rgb(0x637777),
    winbar: rgb(0x8FA7B3),
    strong: rgb(0xFFFFFF),
    search: rgb(0x255B7A),
};

/// The light Dawn flavor of Rose Pine, with a warm base.
pub const ROSE_PINE_DAWN: Scheme = Scheme {
    red: rgb(0xB4637A),
    orange: rgb(0xA66A00),
    green: rgb(0x4F772D),
    cyan: rgb(0x3A7D8C),
    blue: rgb(0x286983),
    purple: rgb(0x7B6495),
    text: rgb(0x575279),
    comment: rgb(0x6E6A86),
    ui_red: rgb(0xB4637A),
    ui_orange: rgb(0xA66A00),
    ui_yellow: rgb(0x8A6A00),
    ui_green: rgb(0x4F772D),
    ui_cyan: rgb(0x3A7D8C),
    ui_purple: rgb(0x7B6495),
    accent: rgb(0x286983),
    base: rgb(0xFAF4ED),
    panel: rgb(0xF2E9E1),
    float: rgb(0xF2E9E1),
    deep: rgb(0xEAE0D7),
    label_text: rgb(0xFAF4ED),
    border: rgb(0xD6CCC2),
    current_line: rgb(0xF4EDE8),
    selection: rgb(0xEADDD5),
    highlight: rgb(0xEAE0D7),
    prompt: rgb(0xEAE0D7),
    ui_text: rgb(0x575279),
    inactive: rgb(0x9893A5),
    winbar: rgb(0x575279),
    strong: rgb(0x575279),
    search: rgb(0xF5D9A8),
};

/// The Everforest light palette, with a green-tinted base.
pub const EVERFOREST_LIGHT: Scheme = Scheme {
    red: rgb(0xC13A37),
    orange: rgb(0xB35C00),
    green: rgb(0x6E7F00),
    cyan: rgb(0x1F7A5C),
    blue: rgb(0x1F6FA8),
    purple: rgb(0xA0408E),
    text: rgb(0x5C6A72),
    comment: rgb(0x74847C),
    ui_red: rgb(0xC13A37),
    ui_orange: rgb(0xB35C00),
    ui_yellow: rgb(0x9A6F00),
    ui_green: rgb(0x6E7F00),
    ui_cyan: rgb(0x1F7A5C),
    ui_purple: rgb(0xA0408E),
    accent: rgb(0x1F6FA8),
    base: rgb(0xFDF6E3),
    panel: rgb(0xF4F0D9),
    float: rgb(0xF4F0D9),
    deep: rgb(0xEAE6CF),
    label_text: rgb(0xFDF6E3),
    border: rgb(0xD3CFB8),
    current_line: rgb(0xF7F3DE),
    selection: rgb(0xDDE0C8),
    highlight: rgb(0xEFEBD4),
    prompt: rgb(0xEFEBD4),
    ui_text: rgb(0x4F5D64),
    inactive: rgb(0xA6B0A0),
    winbar: rgb(0x5C6A72),
    strong: rgb(0x3A464C),
    search: rgb(0xF5D9A8),
};

/// A red-tinted dark palette that marks a production environment.
pub const EMBER: Scheme = Scheme {
    red: rgb(0xFF7B72),
    orange: rgb(0xFFA657),
    green: rgb(0x7EE787),
    cyan: rgb(0x76E3EA),
    blue: rgb(0x79C0FF),
    purple: rgb(0xD2A8FF),
    text: rgb(0xE8D8D5),
    comment: rgb(0xA88F8C),
    ui_red: rgb(0xF85149),
    ui_orange: rgb(0xDB6D28),
    ui_yellow: rgb(0xD29922),
    ui_green: rgb(0x3FB950),
    ui_cyan: rgb(0x39C5CF),
    ui_purple: rgb(0xBC8CFF),
    accent: rgb(0xFF7B72),
    base: rgb(0x221416),
    panel: rgb(0x1B0F11),
    float: rgb(0x180D0F),
    deep: rgb(0x140B0C),
    label_text: rgb(0x221416),
    border: rgb(0x5E3A3E),
    current_line: rgb(0x2E1C1F),
    selection: rgb(0x4A2A2E),
    highlight: rgb(0x332024),
    prompt: rgb(0x332024),
    ui_text: rgb(0xD8C4C1),
    inactive: rgb(0x8A6D6A),
    winbar: rgb(0xC0A6A2),
    strong: rgb(0xFFF0EE),
    search: rgb(0x5A2A2A),
};
