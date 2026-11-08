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
