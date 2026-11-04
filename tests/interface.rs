// The look of the application: themes, symbols, the status line, the menu of `Ctrl+W`,
// the command palette, and the first screen.

mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{ctrl, key, shift, temp_db, temp_dir};
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::console::Focus;
use sqln::editor::KeyboardMode;
use sqln::form::{ConnectionForm, Field, FormNote, Group};
use sqln::input::Keymap;
use sqln::store::config::{ConnectionIcon, IconName, ThemeName};
use sqln::store::{Root, Store};
use sqln::ui::icons::{Icons, connection_symbol};
use sqln::ui::schemes;
use sqln::ui::theme::{ColorEnv, Theme};
use sqln::ui::which_key::{KeyGroup, draw_which_key, widest_row};
use sqln::ui::{MainTab, TabView, draw_main_tabs, text};
use unicode_width::UnicodeWidthStr;

const SETUP: &str =
    "CREATE TABLE users(id INTEGER, name TEXT); INSERT INTO users VALUES (1, 'Ana');";

struct Screen {
    app: App,
}

impl Screen {
    fn new(name: &str, theme: Theme, keyboard: KeyboardMode) -> Self {
        let path = temp_db(&format!("interface-{name}"));
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(SETUP)
            .unwrap();
        let options = Options {
            database: Some(path),
            keyboard,
            ..Options::default()
        };
        Self {
            app: App::new(
                options,
                Rc::new(RefCell::new(MemoryClipboard::default())),
                theme,
            ),
        }
    }

    fn empty(theme: Theme) -> Self {
        Self {
            app: App::new(
                Options::default(),
                Rc::new(RefCell::new(MemoryClipboard::default())),
                theme,
            ),
        }
    }

    fn buffer(&mut self, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn screen(&mut self, width: u16, height: u16) -> String {
        let buffer = self.buffer(width, height);
        rows(&buffer).join("\n")
    }

    fn code(&mut self, code: KeyCode) {
        self.app.on_event(Event::Key(key(code)));
    }

    fn write(&mut self, text: &str) {
        self.app.on_event(Event::Paste(text.to_owned()));
    }

    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.code(KeyCode::Char(c));
        }
    }

    /// Opens the `Ctrl+W` menu and leaves it open.
    fn code_ctrl_w(&mut self) {
        self.app.on_event(Event::Key(ctrl('w')));
    }

    fn prefix(&mut self, letter: char) {
        self.app.on_event(Event::Key(ctrl('w')));
        self.code(KeyCode::Char(letter));
    }

    fn palette(&mut self) {
        self.app.on_event(Event::Key(ctrl('p')));
    }

    fn click(&mut self, column: u16, row: u16) {
        let kind = MouseEventKind::Down(MouseButton::Left);
        self.app.on_event(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
    }

    fn locate(&mut self, text: &str) -> (u16, u16) {
        let buffer = self.buffer(100, 30);
        for (y, row) in rows(&buffer).iter().enumerate() {
            if let Some(byte) = row.find(text) {
                return (row[..byte].chars().count() as u16, y as u16);
            }
        }
        panic!("{text:?} is not on the screen:\n{}", self.screen(100, 30));
    }

    fn pump_until(&mut self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.screen(100, 30).contains(text) {
            self.app.drain_events();
            assert!(
                Instant::now() < deadline,
                "{text:?} did not appear:\n{}",
                self.screen(100, 30)
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn run(&mut self, sql: &str) {
        self.write(sql);
        self.code(KeyCode::F(5));
        self.pump_until(" row");
    }

    /// Opens each kind of screen, and calls `check` with each one.
    fn visit_all(&mut self, mut check: impl FnMut(&Buffer)) {
        let (width, height) = (100, 30);
        check(&self.buffer(width, height));
        self.run("SELECT id, name FROM users;");
        check(&self.buffer(width, height));
        self.prefix('j');
        check(&self.buffer(width, height));
        self.app.on_event(Event::Key(ctrl('w')));
        check(&self.buffer(width, height));
        self.code(KeyCode::Esc);
        self.palette();
        check(&self.buffer(width, height));
        self.code(KeyCode::Esc);
        self.code(KeyCode::F(1));
        check(&self.buffer(width, height));
        self.code(KeyCode::Esc);
        self.prefix(',');
        check(&self.buffer(width, height));
        self.code(KeyCode::Esc);
        self.prefix('h');
        self.code(KeyCode::Char('a'));
        check(&self.buffer(width, height));
        self.code(KeyCode::Esc);
        check(&self.buffer(80, 24));
    }
}

fn rows(buffer: &Buffer) -> Vec<String> {
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect()
        })
        .collect()
}

fn is_rgb(color: Color) -> bool {
    matches!(color, Color::Rgb(..))
}

#[test]
fn the_plain_theme_has_no_color_on_any_screen() {
    let mut screen = Screen::new("plain", Theme::plain(), KeyboardMode::Conventional);
    screen.visit_all(|buffer| {
        for cell in buffer.content() {
            assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset), "{cell:?}");
        }
    });
}

#[test]
fn the_ansi_theme_has_no_rgb_color_on_any_screen() {
    let mut screen = Screen::new("ansi", Theme::ansi(), KeyboardMode::Conventional);
    screen.visit_all(|buffer| {
        for cell in buffer.content() {
            let ansi = |color| !matches!(color, Color::Rgb(..) | Color::Indexed(..));
            assert!(ansi(cell.fg) && ansi(cell.bg), "{cell:?}");
        }
    });
}

#[test]
fn the_astrodark_theme_paints_each_cell_of_each_screen() {
    let mut screen = Screen::new("astrodark", Theme::astrodark(), KeyboardMode::Conventional);
    screen.visit_all(|buffer| {
        for cell in buffer.content() {
            assert!(
                is_rgb(cell.bg),
                "this cell shows the terminal background: {cell:?}"
            );
            assert!(
                is_rgb(cell.fg),
                "this cell shows the terminal text color: {cell:?}"
            );
        }
    });
}

#[test]
fn the_environment_chooses_the_theme() {
    let env = |no_color, truecolor| ColorEnv {
        no_color,
        truecolor,
    };
    assert_eq!(Theme::automatic(env(false, true)), Theme::astrodark());
    assert_eq!(Theme::automatic(env(false, false)), Theme::ansi());
    assert_eq!(Theme::automatic(env(true, true)), Theme::plain());
    assert_eq!(Theme::automatic(env(true, false)), Theme::plain());
}

#[test]
fn the_configuration_chooses_the_theme_and_no_color_has_priority() {
    let color = ColorEnv {
        no_color: false,
        truecolor: true,
    };
    let no_color = ColorEnv {
        no_color: true,
        truecolor: true,
    };
    let start = Theme::ansi();
    assert_eq!(start.with_choices(None, None, color), start);
    assert_eq!(
        start.with_choices(Some(ThemeName::Astrodark), None, color),
        Theme::astrodark()
    );
    assert_eq!(
        Theme::astrodark().with_choices(Some(ThemeName::Ansi), None, color),
        Theme::ansi()
    );
    assert_eq!(
        start.with_choices(Some(ThemeName::Auto), None, color),
        Theme::astrodark()
    );
    assert_eq!(
        start.with_choices(Some(ThemeName::Astrodark), None, no_color),
        Theme::plain()
    );
    assert_eq!(
        start.with_choices(None, Some(IconName::Nerd), color).icons,
        Icons::nerd()
    );
    assert_eq!(
        Theme::plain()
            .with_choices(None, Some(IconName::Nerd), color)
            .icons,
        Icons::plain()
    );
}

#[test]
fn each_symbol_of_each_set_fills_one_column_per_character() {
    for icons in [Icons::plain(), Icons::unicode(), Icons::nerd()] {
        for symbol in icons.all() {
            assert_eq!(symbol.width(), symbol.chars().count(), "{symbol:?}");
        }
    }
}

#[test]
fn each_connection_icon_has_its_own_symbol_of_one_column() {
    let symbols: Vec<&str> = ConnectionIcon::ALL.map(connection_symbol).to_vec();
    for (index, symbol) in symbols.iter().enumerate() {
        assert_eq!(symbol.width(), 1, "{symbol:?}");
        assert_eq!(symbol.chars().count(), 1, "{symbol:?}");
        assert!(!symbols[index + 1..].contains(symbol), "{symbol:?}");
    }
}

#[test]
fn each_name_gives_its_own_theme_and_a_place_in_the_file() {
    let env = ColorEnv {
        no_color: false,
        truecolor: true,
    };
    let themes: Vec<Theme> = ThemeName::ALL
        .iter()
        .filter(|name| **name != ThemeName::Auto)
        .map(|name| Theme::named(*name, env))
        .collect();
    for (index, theme) in themes.iter().enumerate() {
        assert!(!themes[index + 1..].contains(theme), "{index}");
    }
    for name in ThemeName::ALL {
        let value = serde_json::to_string(&name).unwrap();
        let (store, _) = themed_store("theme-name", &format!(r#"{{"theme":{value}}}"#));
        let config = sqln::store::config::load(&store).config.unwrap();
        assert_eq!(config.theme, Some(name));
    }
}

/// Returns the contrast of two RGB colors, with the formula of WCAG.
fn contrast(first: Color, second: Color) -> f64 {
    let luminance = |color: Color| {
        let Color::Rgb(red, green, blue) = color else {
            panic!("{color:?}");
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

#[test]
fn each_scheme_keeps_its_text_readable() {
    const BODY: f64 = 4.5;
    const SHORT: f64 = 3.0;
    let schemes = [
        schemes::ASTRODARK,
        schemes::TOKYO_NIGHT,
        schemes::CATPPUCCIN_MOCHA,
        schemes::CATPPUCCIN_LATTE,
        schemes::GRUVBOX,
        schemes::NORD,
    ];
    for scheme in schemes {
        let theme = Theme::from_scheme(&scheme);
        let filled = |style: Style| contrast(style.fg.unwrap(), style.bg.unwrap());
        let on_base = |style: Style| contrast(style.fg.unwrap(), scheme.base);
        for style in [
            theme.base,
            theme.panel,
            theme.dialog,
            theme.list,
            theme.list_selected,
            theme.tree_selected,
            theme.field_focus,
            theme.statusline,
            theme.search_match,
        ] {
            assert!(filled(style) >= BODY, "{scheme:?} {style:?}");
        }
        for style in [
            theme.list_detail,
            theme.status_detail,
            theme.status_warning,
            theme.status_error,
            theme.tab,
            theme.tab_active,
            theme.run_active,
            theme.run_done,
            theme.run_failed,
            theme.run_stopped,
            theme.label_editor,
            theme.label_insert,
            theme.label_visual,
            theme.label_results,
            theme.label_explorer,
            theme.dialog_focus,
            theme.search_current,
            theme.header,
        ] {
            assert!(filled(style) >= SHORT, "{scheme:?} {style:?}");
        }
        for style in [
            theme.keyword,
            theme.string,
            theme.number,
            theme.identifier,
            theme.comment,
            theme.hint,
            theme.status,
            theme.error,
            theme.mark_good,
            theme.mark_bad,
            theme.mark_neutral,
            theme.control,
            theme.key,
            theme.panel_title_focused,
        ] {
            assert!(on_base(style) >= SHORT, "{scheme:?} {style:?}");
        }
    }
}

/// Draws one SQLite connection with a short name, and returns the screen.
fn screen_with_connection(name: &str, theme: Theme) -> String {
    let path = temp_db(&format!("interface-{name}"));
    let connector = Arc::new(sqln::connector::sqlite::SqliteConnector::new(
        sqln::connector::sqlite::SqliteTarget::new(path),
    ));
    let options = Options {
        connector: Some(connector),
        label: Some("orders".to_owned()),
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let mut screen = Screen {
        app: App::new(options, clipboard, theme),
    };
    screen.screen(100, 30)
}

#[test]
fn a_set_without_logos_keeps_the_engine_text_after_the_name() {
    let shown = screen_with_connection("unicode-row", Theme::astrodark());
    assert!(shown.contains("⛁ orders  sq"), "{shown}");

    let shown = screen_with_connection("plain-row", Theme::plain());
    assert!(shown.contains("orders  sq"), "{shown}");
    assert!(!shown.contains("⛁"), "{shown}");
}

#[test]
fn a_set_with_logos_shows_only_the_logo_before_the_name() {
    let icons = Icons::nerd();
    let theme = Theme {
        icons,
        ..Theme::astrodark()
    };
    let shown = screen_with_connection("nerd-row", theme);
    let logo = icons.sqlite;
    assert!(shown.contains(&format!("{logo} orders")), "{shown}");
    assert!(!shown.contains(&format!("orders  {logo}")), "{shown}");
}

/// The columns of the **Quit** button at the right end of the row: one column before it,
/// the label, and one column after it.
const QUIT: u16 = 8;

/// Draws the main tabs on one row, and returns the part of the row with the tabs and the
/// places of the tabs. `width` is the part of the tabs, without the button.
fn main_tabs(width: u16, labels: &[&str], active: usize, theme: &Theme) -> (String, Vec<MainTab>) {
    let views = labels
        .iter()
        .map(|label| TabView {
            label: (*label).to_owned(),
            badge: None,
        })
        .collect();
    draw_views(width, views, active, theme)
}

fn draw_views(
    width: u16,
    views: Vec<TabView>,
    active: usize,
    theme: &Theme,
) -> (String, Vec<MainTab>) {
    let mut terminal = Terminal::new(TestBackend::new(width + QUIT, 1)).unwrap();
    let mut tabs = Vec::new();
    terminal
        .draw(|frame| (tabs, _) = draw_main_tabs(frame, frame.area(), views, active, theme))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row: String = rows(buffer)
        .remove(0)
        .chars()
        .take(usize::from(width))
        .collect();
    (row, tabs)
}

#[test]
fn the_quit_button_stands_at_the_right_end_and_the_tabs_stop_before_it() {
    let theme = Theme::plain();
    let views: Vec<TabView> = (0..9)
        .map(|index| TabView {
            label: format!("console-{index}"),
            badge: None,
        })
        .collect();
    let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
    let mut drawn = (Vec::new(), ratatui::layout::Rect::default());
    terminal
        .draw(|frame| drawn = draw_main_tabs(frame, frame.area(), views, 8, &theme))
        .unwrap();
    let (tabs, quit) = drawn;
    let row = rows(terminal.backend().buffer()).remove(0);
    assert!(row.ends_with(" [Quit] "), "{row}");
    assert_eq!((quit.x, quit.width), (73, 6));
    assert!(tabs.iter().all(|tab| tab.end <= 72), "{tabs:?}");
    // The active tab stays on the row.
    assert_eq!(tabs.last().unwrap().index, 8);
}

#[test]
fn the_close_mark_stands_at_the_end_of_each_tab_in_each_set() {
    for (theme, mark) in [
        (Theme::plain(), "X"),
        (Theme::ansi(), "×"),
        (Theme::astrodark(), "×"),
    ] {
        let (row, tabs) = main_tabs(40, &["one", "second"], 1, &theme);
        let cell = |column: u16| row.chars().nth(usize::from(column)).unwrap().to_string();
        assert_eq!(
            tabs[0],
            MainTab {
                start: 0,
                end: 7,
                close: Some(5),
                index: 0
            }
        );
        assert_eq!(
            tabs[1],
            MainTab {
                start: 7,
                end: 17,
                close: Some(15),
                index: 1
            }
        );
        assert_eq!(cell(5), mark, "{row}");
        assert_eq!(cell(15), mark, "{row}");
    }
}

#[test]
fn a_tab_that_the_row_cuts_has_no_close_mark() {
    let theme = Theme::plain();
    // The second tab needs ten columns, and the row keeps six of them.
    let (row, tabs) = main_tabs(13, &["one", "second"], 0, &theme);
    assert_eq!(
        tabs[1],
        MainTab {
            start: 7,
            end: 13,
            close: None,
            index: 1
        }
    );
    assert_eq!(row, " one X  secon");
    // With the column of the mark on the row, the tab keeps its mark.
    let (row, tabs) = main_tabs(16, &["one", "second"], 0, &theme);
    assert_eq!(tabs[1].close, Some(15));
    assert!(row.ends_with('X'), "{row}");
}

/// Returns the cell at the start of a text in the last row.
fn hint_cell(buffer: &Buffer, text: &str) -> ratatui::buffer::Cell {
    let last = buffer.area.height - 1;
    let row = rows(buffer).pop().unwrap();
    let byte = row
        .find(text)
        .unwrap_or_else(|| panic!("{text:?} is not in {row:?}"));
    buffer[(row[..byte].chars().count() as u16, last)].clone()
}

#[test]
fn the_hint_bar_gives_the_keys_and_their_effects_two_styles() {
    let theme = Theme::astrodark();
    let mut screen = Screen::new("hint-styles", theme, KeyboardMode::Conventional);
    screen.prefix('h');
    let buffer = screen.buffer(100, 30);
    for key in ["Enter", "n new", "Right", "Ctrl+P"] {
        assert_eq!(hint_cell(&buffer, key).fg, theme.key.fg.unwrap(), "{key}");
    }
    for effect in ["console", "new", "expand", "commands"] {
        assert_eq!(
            hint_cell(&buffer, effect).fg,
            theme.hint.fg.unwrap(),
            "{effect}"
        );
    }
}

#[test]
fn the_hint_bar_separates_the_keys_without_color() {
    let mut screen = Screen::new("hint-plain", Theme::plain(), KeyboardMode::Conventional);
    screen.prefix('h');
    let buffer = screen.buffer(100, 30);
    assert!(
        hint_cell(&buffer, "Enter")
            .modifier
            .contains(Modifier::BOLD)
    );
    assert!(
        hint_cell(&buffer, "console")
            .modifier
            .contains(Modifier::DIM)
    );
}

#[test]
fn a_note_in_the_hint_bar_has_no_key_style() {
    let theme = Theme::astrodark();
    let mut screen = Screen::new("hint-note", theme, KeyboardMode::Conventional);
    screen.palette();
    let buffer = screen.buffer(100, 30);
    let row = rows(&buffer).pop().unwrap();
    assert!(row.starts_with(" Write to filter  Up/Down move"), "{row}");
    assert_eq!(
        hint_cell(&buffer, "Write to filter").fg,
        theme.hint.fg.unwrap()
    );
    assert_eq!(hint_cell(&buffer, "Enter").fg, theme.key.fg.unwrap());
}

#[test]
fn the_explorer_shows_the_first_commands_of_a_connection_at_the_minimum_width() {
    for keyboard in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut screen = Screen::new("hint-width", Theme::plain(), keyboard);
        screen.prefix('h');
        let row = rows(&screen.buffer(80, 24)).pop().unwrap();
        assert_eq!(row.chars().count(), 80);
        assert!(
            row.contains(
                "Enter console  n new console  Right expand  r refresh  e edit  Ctrl+P commands"
            ),
            "{row}"
        );
    }
}

#[test]
fn a_badge_stands_at_the_start_of_the_tab_and_the_close_mark_stays_at_the_end() {
    let theme = Theme::plain();
    let badge = Some((" ok ".to_owned(), theme.run_done));
    let views = vec![
        TabView {
            label: "one".to_owned(),
            badge,
        },
        TabView {
            label: "two".to_owned(),
            badge: None,
        },
    ];
    let (row, tabs) = draw_views(40, views, 0, &theme);
    assert!(row.starts_with("  ok  one X  two X "), "{row}");
    assert_eq!(
        tabs[0],
        MainTab {
            start: 0,
            end: 12,
            close: Some(10),
            index: 0
        }
    );
    assert_eq!(tabs[1].start, 12);
}

#[test]
fn a_tab_with_a_badge_that_the_row_cuts_has_no_close_mark() {
    let theme = Theme::plain();
    let badge = Some((" | 1m 05s ".to_owned(), theme.run_active));
    let views = vec![
        TabView {
            label: "one".to_owned(),
            badge: None,
        },
        TabView {
            label: "two".to_owned(),
            badge,
        },
    ];
    let (_, tabs) = draw_views(20, views, 0, &theme);
    assert_eq!(tabs[1].close, None);
}

#[test]
fn each_state_of_a_run_has_its_own_color() {
    let theme = Theme::astrodark();
    let styles = [
        theme.run_active,
        theme.run_done,
        theme.run_failed,
        theme.run_stopped,
    ];
    for (index, style) in styles.iter().enumerate() {
        assert!(is_rgb(style.bg.unwrap()), "{style:?}");
        assert!(
            !styles[..index].iter().any(|other| other.bg == style.bg),
            "{style:?}"
        );
    }
    assert_ne!(Icons::plain().failed, Icons::plain().close);
}

#[test]
fn the_time_of_a_run_has_three_decimals_in_each_size() {
    let millis = Duration::from_millis;
    for (time, shown) in [
        (0, "0.000s"),
        (7, "0.007s"),
        (2_440, "2.440s"),
        (59_999, "59.999s"),
        (60_000, "1m 00.000s"),
        (65_123, "1m 05.123s"),
        (3_599_500, "59m 59.500s"),
        (3_600_000, "1h 00m 00.000s"),
        (3_661_007, "1h 01m 01.007s"),
    ] {
        assert_eq!(text::elapsed(millis(time)), shown);
    }
    assert_eq!(text::last_run(millis(2_440)), "(last 2.440s)");
}

#[test]
fn a_button_has_the_same_width_in_each_theme() {
    let plain = Theme::plain().button("Save");
    let dark = Theme::astrodark().button("Save");
    assert_eq!(plain, "[Save]");
    assert_eq!(dark, " Save ");
    assert_eq!(plain.width(), dark.width());
}

#[test]
fn external_names_send_no_sequence_through_the_new_rows() {
    let name = "bad\x1b[31mname\x07";
    let path = temp_db("interface-control-name");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(SETUP)
        .unwrap();
    let connector = Arc::new(sqln::connector::sqlite::SqliteConnector::new(
        sqln::connector::sqlite::SqliteTarget::new(path),
    ));
    let options = Options {
        connector: Some(connector),
        label: Some(name.to_owned()),
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let mut screen = Screen {
        app: App::new(options, clipboard, Theme::astrodark()),
    };
    for buffer in [screen.buffer(100, 30), screen.buffer(80, 24)] {
        for cell in buffer.content() {
            assert!(!cell.symbol().chars().any(char::is_control), "{cell:?}");
        }
    }
}

#[test]
fn a_tall_screen_has_a_title_and_a_short_screen_keeps_the_rows_of_the_panels() {
    let mut screen = Screen::new("rows", Theme::plain(), KeyboardMode::Conventional);
    assert!(
        screen.screen(100, 30).contains("›"),
        "the tall screen has the title"
    );
    assert!(
        !screen.screen(100, 29).contains("›"),
        "the short screen has no title"
    );
    let short = screen.buffer(80, 24);
    assert_eq!(
        rows(&short)[2].chars().nth(5 + 24 + 1),
        Some(' '),
        "the editor starts on row 2"
    );
    assert!(rows(&short)[2].contains('1'), "{}", rows(&short).join("\n"));
    assert!(
        rows(&short)[1].contains("[Run]"),
        "{}",
        rows(&short).join("\n")
    );
}

#[test]
fn the_status_line_names_the_place_of_the_keys() {
    let mut screen = Screen::new("status", Theme::plain(), KeyboardMode::Vim);
    let status = |screen: &mut Screen| screen.screen(100, 30).lines().nth(28).unwrap().to_owned();
    assert!(
        status(&mut screen).contains("NORMAL"),
        "{}",
        status(&mut screen)
    );
    screen.code(KeyCode::Char('i'));
    assert!(
        status(&mut screen).contains("INSERT"),
        "{}",
        status(&mut screen)
    );
    screen.code(KeyCode::Esc);
    screen.code(KeyCode::Char('v'));
    assert!(
        status(&mut screen).contains("VISUAL"),
        "{}",
        status(&mut screen)
    );
    screen.code(KeyCode::Esc);
    screen.prefix('h');
    assert!(
        status(&mut screen).contains("EXPLORER"),
        "{}",
        status(&mut screen)
    );
    screen.prefix('l');
    screen.prefix('j');
    assert!(
        status(&mut screen).contains("RESULTS"),
        "{}",
        status(&mut screen)
    );

    let mut conventional = Screen::new("status-editor", Theme::plain(), KeyboardMode::Conventional);
    assert!(
        status(&mut conventional).contains("EDITOR"),
        "{}",
        status(&mut conventional)
    );
}

#[test]
fn the_state_stays_and_the_other_parts_go_on_a_narrow_row() {
    let long = "a-very-long-connection-name-that-cannot-fit-next-to-the-state-of-the-console.db";
    let mut screen = Screen::new(long, Theme::plain(), KeyboardMode::Conventional);
    let row = screen.screen(80, 24).lines().nth(22).unwrap().to_owned();
    assert!(row.contains("Auto · No transaction · Idle"), "{row}");
    assert!(row.contains('…'), "{row}");
    assert!(!row.contains("1:1"), "the position goes first: {row}");
}

#[test]
fn an_open_transaction_uses_the_warning_style() {
    let mut screen = Screen::new("warning", Theme::astrodark(), KeyboardMode::Conventional);
    screen.write("BEGIN;");
    screen.code(KeyCode::F(5));
    screen.pump_until("Transaction open");
    let theme = Theme::astrodark();
    let (x, y) = screen.locate("Transaction open");
    assert_eq!(
        screen.buffer(100, 30)[(x, y)].fg,
        theme.status_warning.fg.unwrap()
    );
}

#[test]
fn the_grid_has_row_numbers_and_rules() {
    let mut screen = Screen::new("numbers", Theme::plain(), KeyboardMode::Conventional);
    screen.run("SELECT id, name FROM users;");
    let shown = screen.screen(100, 30);
    assert!(shown.contains("1 │    1 │ Ana"), "{shown}");
}

#[test]
fn a_scrollbar_appears_only_when_the_rows_do_not_fit() {
    let mut short = Screen::new("bar-short", Theme::plain(), KeyboardMode::Conventional);
    short.run("SELECT id FROM users;");
    assert!(
        !short.screen(100, 30).contains('▐'),
        "{}",
        short.screen(100, 30)
    );

    let mut long = Screen::new("bar-long", Theme::plain(), KeyboardMode::Conventional);
    long.run("WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 90) SELECT x FROM c;");
    assert!(
        long.screen(100, 30).contains('▐'),
        "{}",
        long.screen(100, 30)
    );
}

#[test]
fn a_click_on_a_result_tab_and_on_the_explorer_title_moves_the_focus() {
    let mut screen = Screen::new("clicks", Theme::plain(), KeyboardMode::Conventional);
    screen.run("SELECT 1 AS a;");
    let (column, row) = screen.locate("Messages");
    screen.click(column, row);
    assert!(screen.app.console().showing_messages());
    let (column, row) = screen.locate("Result 1");
    screen.click(column, row);
    assert!(!screen.app.console().showing_messages());
    let (column, row) = screen.locate("EXPLORER");
    screen.click(column, row);
    assert!(screen.app.navigator_focused());
}

#[test]
fn a_click_on_a_tab_works_with_more_results_than_columns() {
    let mut screen = Screen::new("many-tabs", Theme::plain(), KeyboardMode::Conventional);
    let script = "SELECT 1 AS a;\n".repeat(400);
    screen.app.on_event(Event::Paste(script));
    screen.app.on_event(Event::Key(shift(KeyCode::F(5))));
    let deadline = Instant::now() + Duration::from_secs(20);
    while screen.app.console().is_busy() {
        screen.app.drain_events();
        assert!(Instant::now() < deadline, "the run did not end");
    }
    assert_eq!(screen.app.console().results.len(), 400);
    let (column, row) = screen.locate("Result 2");
    screen.click(column, row);
    assert_eq!(screen.app.console().active, 1);
}

#[test]
fn the_edge_of_a_header_accepts_each_column_of_the_gap() {
    for offset in 0..3 {
        let mut screen = Screen::new("edge", Theme::plain(), KeyboardMode::Conventional);
        screen.prefix('b');
        screen.run("SELECT 'a value' AS one, 'b' AS two;");
        // The text "one" is also in the editor, so the search uses the rule before the header.
        let (rule, row) = screen.locate("│ one");
        let edge = rule + 2 + 7 + offset;
        let event = |kind, column| {
            Event::Mouse(MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            })
        };
        screen
            .app
            .on_event(event(MouseEventKind::Down(MouseButton::Left), edge));
        screen
            .app
            .on_event(event(MouseEventKind::Drag(MouseButton::Left), edge + 2));
        assert_eq!(
            screen.app.console().results[0].table.width(0),
            9,
            "offset {offset}"
        );
    }
}

#[test]
fn the_menu_of_the_prefix_replaces_the_compressed_hint() {
    let mut screen = Screen::new("menu", Theme::plain(), KeyboardMode::Conventional);
    screen.app.on_event(Event::Key(ctrl('w')));
    let shown = screen.screen(80, 24);
    for text in [
        "Panels",
        "Transaction",
        "Auto or manual",
        "Quit",
        "Ctrl+W …",
    ] {
        assert!(shown.contains(text), "{text}:\n{shown}");
    }
    screen.code(KeyCode::Esc);
    assert!(!screen.screen(80, 24).contains("Panels"));
}

#[test]
fn the_menu_omits_a_removed_key_and_fits_the_minimum_terminal() {
    let overrides = HashMap::from([("quit".to_owned(), Vec::new())]);
    let keymap = Keymap::with_overrides(KeyboardMode::Conventional, &overrides).unwrap();
    let options = Options {
        keymap: Some(keymap),
        ..Options::default()
    };
    let app = App::new(
        options,
        Rc::new(RefCell::new(MemoryClipboard::default())),
        Theme::plain(),
    );
    let groups = app.prefix_menu();
    let keys: Vec<&str> = groups
        .iter()
        .flat_map(|group| &group.keys)
        .map(|(key, _)| key.as_str())
        .collect();
    assert!(!keys.contains(&"q"), "{keys:?}");
    // The menu has three columns inside 76 columns, each with a space at its start.
    assert!(widest_row(&groups) <= 24, "{}", widest_row(&groups));
    let page = app.explorer_menu();
    assert!(widest_row(&page) <= 24, "{}", widest_row(&page));
}

#[test]
fn the_menu_names_the_explorer_page_at_the_minimum_terminal() {
    let mut screen = Screen::new("menu-new-rows", Theme::plain(), KeyboardMode::Conventional);
    screen.code_ctrl_w();
    let shown = screen.screen(80, 24);
    for text in ["Tab  Explorer commands", "Quit", "Tab explorer"] {
        assert!(shown.contains(text), "{text}:\n{shown}");
    }
}

#[test]
fn an_empty_group_does_not_change_the_menu() {
    let group = |title, rows| KeyGroup {
        title,
        keys: vec![("k".to_owned(), "Label"); rows],
    };
    let shown: Vec<KeyGroup> = ["A", "B", "C", "D", "E", "F"]
        .map(|title| group(title, 1))
        .to_vec();
    let mut with_empty = shown.clone();
    with_empty.extend(["G", "H", "I"].map(|title| group(title, 0)));
    let draw = |groups: &[KeyGroup]| {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| {
                draw_which_key(frame, frame.area(), "Menu", groups, &Theme::plain());
            })
            .unwrap();
        terminal.backend().buffer().clone()
    };
    assert_eq!(draw(&shown), draw(&with_empty));
}

#[test]
fn the_explorer_page_fits_the_minimum_terminal_and_names_its_row() {
    let mut screen = Screen::new("menu-page", Theme::plain(), KeyboardMode::Conventional);
    screen.prefix('h');
    screen.code_ctrl_w();
    screen.code(KeyCode::Tab);
    let shown = screen.screen(80, 24);
    for text in [
        "Ctrl+W · Explorer · ",
        "n    New console",
        "/    Filter the tables",
    ] {
        assert!(shown.contains(text), "{text}:\n{shown}");
    }
    screen.code(KeyCode::Esc);
    assert!(!screen.screen(80, 24).contains("Ctrl+W · Explorer"));
}

#[test]
fn the_palette_filters_and_runs_a_command() {
    let mut screen = Screen::new("palette-run", Theme::plain(), KeyboardMode::Conventional);
    screen.write("SELECT 7 AS seven;");
    screen.palette();
    assert!(screen.screen(100, 30).contains("Commands"));
    screen.type_text("run script");
    let shown = screen.screen(100, 30);
    assert!(shown.contains("Run the script"), "{shown}");
    assert!(!shown.contains("Close the tab"), "{shown}");
    screen.code(KeyCode::Enter);
    assert!(
        !screen.screen(100, 30).contains("Commands"),
        "Enter closes the window"
    );
    screen.pump_until("seven");
}

#[test]
fn escape_closes_the_palette_and_its_text_does_not_reach_the_editor() {
    let mut screen = Screen::new("palette-esc", Theme::plain(), KeyboardMode::Conventional);
    screen.palette();
    screen.type_text("abc");
    screen.code(KeyCode::Esc);
    assert!(!screen.screen(100, 30).contains("Commands"));
    assert_eq!(screen.app.console_mut().editor.text(), "");
}

#[test]
fn the_second_key_of_the_palette_works() {
    let mut screen = Screen::new("palette-prefix", Theme::plain(), KeyboardMode::Vim);
    screen.prefix(':');
    assert!(screen.screen(100, 30).contains("Commands"));
}

#[test]
fn a_paste_goes_to_the_filter_of_the_palette() {
    let mut screen = Screen::new("palette-paste", Theme::plain(), KeyboardMode::Conventional);
    screen.palette();
    screen.write("settings");
    assert!(screen.screen(100, 30).contains("Open the settings"));
    assert_eq!(screen.app.console_mut().editor.text(), "");
}

#[test]
fn the_palette_works_with_the_mouse() {
    let mut screen = Screen::new("palette-mouse", Theme::plain(), KeyboardMode::Conventional);
    screen.palette();
    screen.type_text("settings");
    let (column, row) = screen.locate("Open the settings");
    screen.click(column, row);
    let shown = screen.screen(100, 30);
    assert!(!shown.contains("Commands"), "{shown}");
    assert!(
        shown.contains("no configuration files"),
        "the command ran:\n{shown}"
    );
}

#[test]
fn the_wheel_moves_the_cursor_of_the_palette() {
    let wheel = |kind| {
        Event::Mouse(MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        })
    };
    let mut screen = Screen::new("palette-wheel", Theme::plain(), KeyboardMode::Conventional);
    screen.palette();
    let first = screen.buffer(100, 30);

    screen.app.on_event(wheel(MouseEventKind::ScrollDown));
    let second = screen.buffer(100, 30);
    assert_ne!(first, second);
    screen.app.on_event(wheel(MouseEventKind::ScrollUp));
    assert_eq!(first, screen.buffer(100, 30));
    screen.code(KeyCode::Down);
    assert_eq!(
        second,
        screen.buffer(100, 30),
        "the wheel acts as the arrow"
    );
}

#[test]
fn the_palette_shows_only_commands_that_have_an_effect() {
    let mut screen = Screen::new("palette-state", Theme::plain(), KeyboardMode::Conventional);
    screen.palette();
    screen.write("rollback");
    assert!(screen.screen(100, 30).contains("No command matches"));
    screen.code(KeyCode::Esc);

    screen.write("BEGIN;");
    screen.code(KeyCode::F(5));
    // The state of the transaction arrives before the end of the run.
    screen.pump_until("Transaction open · Idle");
    screen.palette();
    screen.write("rollback");
    assert!(!screen.screen(100, 30).contains("No command matches"));
    screen.code(KeyCode::Enter);
    screen.pump_until("No transaction");
}

#[test]
fn the_palette_hides_the_console_commands_in_a_table() {
    let mut screen = Screen::new("palette-table", Theme::plain(), KeyboardMode::Conventional);
    screen.prefix('h');
    screen.code(KeyCode::Right);
    screen.pump_until("main");
    screen.code(KeyCode::Down);
    screen.code(KeyCode::Down);
    screen.code(KeyCode::Right);
    screen.pump_until("users");
    screen.code(KeyCode::Down);
    screen.code(KeyCode::Enter);
    screen.pump_until("users (table)");
    screen.palette();
    screen.write("run the");
    assert!(screen.screen(100, 30).contains("No command matches"));
}

#[test]
fn the_palette_shows_a_panel_key_only_in_its_panel() {
    let key_of_row = |screen: &mut Screen| {
        let (column, row) = screen.locate("Add a connection");
        let buffer = screen.buffer(100, 30);
        let line = &rows(&buffer)[usize::from(row)];
        let after: String = line
            .chars()
            .skip(usize::from(column) + "Add a connection".len())
            .collect();
        after.split_whitespace().next().unwrap_or("").to_owned()
    };
    let mut screen = Screen::new("palette-key", Theme::plain(), KeyboardMode::Conventional);
    screen.palette();
    screen.write("add a connection");
    assert_eq!(
        key_of_row(&mut screen),
        "│",
        "the key of a panel means nothing in the editor"
    );
    screen.code(KeyCode::Esc);
    screen.prefix('h');
    screen.palette();
    screen.write("add a connection");
    assert_eq!(key_of_row(&mut screen), "a");
}

#[test]
fn the_first_screen_shows_the_keys_and_the_first_character_ends_it() {
    let mut screen = Screen::new("welcome", Theme::plain(), KeyboardMode::Conventional);
    let shown = screen.screen(100, 30);
    for text in ["SQLN", "Write SQL here", "Ctrl+P", "Run the script"] {
        assert!(shown.contains(text), "{text}:\n{shown}");
    }
    screen.code(KeyCode::Char('s'));
    let shown = screen.screen(100, 30);
    assert!(!shown.contains("Write SQL here"), "{shown}");
    assert_eq!(screen.app.console_mut().editor.text(), "s");
}

#[test]
fn the_first_screen_without_a_connection_gives_the_way_to_add_one() {
    let mut screen = Screen::empty(Theme::plain());
    let shown = screen.screen(100, 30);
    assert!(shown.contains("No connection yet"), "{shown}");
    assert!(shown.contains("Ctrl+W h a"), "{shown}");
    assert!(shown.contains("Add a connection"), "{shown}");
}

#[test]
fn the_first_screen_leaves_the_first_row_of_the_editor_free() {
    let mut screen = Screen::new("welcome-row", Theme::plain(), KeyboardMode::Conventional);
    let buffer = screen.buffer(80, 24);
    assert!(
        !rows(&buffer)[2].contains("SQLN"),
        "{}",
        rows(&buffer).join("\n")
    );
    assert!(rows(&buffer).iter().any(|row| row.contains("SQLN")));
}

/// Opens the connection form. The focus starts on the first field.
fn form_screen(name: &str, theme: Theme) -> Screen {
    let mut screen = Screen::new(name, theme, KeyboardMode::Conventional);
    screen.prefix('h');
    screen.code(KeyCode::Char('a'));
    screen
}

const FORM_LABELS: [&str; 20] = [
    "Type",
    "Name",
    "Theme",
    "Icon",
    "Entry",
    "Host or socket",
    "Port",
    "Database",
    "User",
    "Password",
    "Credential",
    "TLS",
    "CA file",
    "Client certificate",
    "Client key",
    "Pool limit",
    "Read only",
    "Production",
    "Connect limit",
    "SQL limit",
];

#[test]
fn a_text_field_with_the_focus_shows_its_cursor_in_each_theme() {
    for (index, theme) in [Theme::astrodark(), Theme::ansi(), Theme::plain()]
        .into_iter()
        .enumerate()
    {
        let mut screen = form_screen(&format!("form-cursor-{index}"), theme);
        screen.code(KeyCode::Tab);
        screen.type_text("app");
        let (x, y) = {
            let buffer = screen.buffer(100, 30);
            let row = rows(&buffer)
                .iter()
                .position(|row| row.contains("Name"))
                .unwrap();
            let byte = rows(&buffer)[row].find("app").unwrap();
            let column = rows(&buffer)[row][..byte].chars().count();
            ((column + 3) as u16, row as u16)
        };
        let cell = screen.buffer(100, 30)[(x, y)].clone();
        let bg_matches = theme.cursor.bg.is_none_or(|bg| cell.bg == bg);
        let fg_matches = theme.cursor.fg.is_none_or(|fg| cell.fg == fg);
        assert!(bg_matches && fg_matches, "{cell:?}");
        assert!(
            cell.modifier.contains(theme.cursor.add_modifier),
            "{cell:?}"
        );
    }
}

#[test]
fn the_focus_mark_moves_with_the_focus() {
    let mut screen = form_screen("form-mark", Theme::plain());
    let row_of = |screen: &mut Screen, label: &str| {
        let shown = screen.screen(100, 30);
        shown
            .lines()
            .find(|line| line.contains(label))
            .unwrap()
            .to_owned()
    };
    assert!(row_of(&mut screen, "Type").contains("> Type"));
    screen.code(KeyCode::Tab);
    assert!(!row_of(&mut screen, "Type").contains("> Type"));
    assert!(row_of(&mut screen, "Name").contains("> Name"));
    focus_field(&mut screen, Field::Password);
    assert!(row_of(&mut screen, "Password").contains("> Password"));
}

#[test]
fn the_row_with_the_focus_has_its_own_colors() {
    let theme = Theme::astrodark();
    let mut screen = form_screen("form-colors", theme);
    screen.code(KeyCode::Tab);
    let (column, row) = screen.locate("Name");
    let buffer = screen.buffer(100, 30);
    assert_eq!(
        buffer[(column, row)].fg,
        theme.panel_title_focused.fg.unwrap()
    );
    let (other, other_row) = screen.locate("Entry");
    assert_ne!(
        buffer[(other, other_row)].fg,
        theme.panel_title_focused.fg.unwrap()
    );
    let value = column + 24 + 12;
    assert_eq!(buffer[(value, row)].bg, theme.field_focus.bg.unwrap());
    assert_eq!(buffer[(value, other_row)].bg, theme.field.bg.unwrap());
}

#[test]
fn each_section_shows_its_fields_and_the_buttons_in_each_screen() {
    let mut screen = form_screen("form-room", Theme::plain());
    for (width, height) in [(100, 40), (130, 26), (100, 30), (80, 24)] {
        let mut seen = Vec::new();
        for title in ["General", "Address", "Credentials", "Security", "Options"] {
            open_section(&mut screen, (width, height), title);
            let shown = screen.screen(width, height);
            assert!(shown.contains(&title.to_uppercase()), "{title}:\n{shown}");
            for button in ["[Test connection]", "[Save]", "[Cancel]"] {
                assert!(
                    shown.contains(button),
                    "{button} at {width} x {height}:\n{shown}"
                );
            }
            seen.push(shown);
        }
        for label in FORM_LABELS {
            assert!(
                seen.iter().any(|shown| shown.contains(label)),
                "{label} at {width} x {height}"
            );
        }
    }
}

#[test]
fn the_sidebar_shows_only_the_sections_with_fields() {
    let mut screen = form_screen("form-sections", Theme::plain());
    let shown = screen.screen(100, 30);
    assert!(
        shown.contains("Credentials") && shown.contains("Security"),
        "{shown}"
    );
    screen.code(KeyCode::Left);
    screen.code(KeyCode::Left);
    assert_eq!(screen.app.form().unwrap().kind, sqln::form::Kind::Sqlite);
    let shown = screen.screen(100, 30);
    assert!(!shown.contains("Credentials"), "{shown}");
    assert!(!shown.contains("Security"), "{shown}");
}

#[test]
fn the_keys_of_the_sidebar_open_a_section_and_tab_stays_in_it() {
    let mut screen = form_screen("form-sidebar-keys", Theme::plain());
    screen.code(KeyCode::BackTab);
    assert_eq!(screen.app.form().unwrap().focus, Field::Sections);
    screen.code(KeyCode::Down);
    assert_eq!(screen.app.form().unwrap().shown_section(), Group::Address);
    screen.code(KeyCode::Tab);
    assert_eq!(screen.app.form().unwrap().focus, Field::Host);
    // Tab passes the fields of the section and the buttons, then returns to the sidebar.
    for _ in 0..6 {
        screen.code(KeyCode::Tab);
    }
    assert_eq!(screen.app.form().unwrap().focus, Field::Sections);
}

#[test]
fn the_form_of_a_connected_connection_has_its_own_sections() {
    let target = sqln::connector::sqlite::SqliteTarget::new("shop.db");
    let definition = sqln::connection::Definition::sqlite(target);
    let mut form = ConnectionForm::editing(1, &definition, true);
    assert_eq!(form.sections(), [Group::General, Group::Options]);
    assert_eq!(form.section_fields(), [Field::Name]);
    form.select_section(Group::Options);
    assert_eq!(
        form.section_fields(),
        [Field::Theme, Field::Icon, Field::Production]
    );
    assert_eq!(
        form.buttons(),
        [Field::Disconnect, Field::Save, Field::Cancel]
    );
}

#[test]
fn an_error_opens_the_section_of_its_field() {
    let mut screen = form_screen("form-error-section", Theme::plain());
    focus_field(&mut screen, Field::Name);
    screen.type_text("remote");
    focus_field(&mut screen, Field::Save);
    screen.code(KeyCode::Enter);
    let form = screen.app.form().unwrap();
    assert_eq!(form.focus, Field::Host);
    assert_eq!(form.shown_section(), Group::Address);
    assert!(screen.screen(100, 30).contains("ADDRESS"));
}

/// The form with the most fields: a network connection with the choice of a scope.
#[test]
fn the_largest_form_keeps_its_fields_and_buttons_in_the_smallest_screen() {
    let mut form = ConnectionForm::new_in(true);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let mut seen = String::new();
    for section in form.sections() {
        form.select_section(section);
        terminal
            .draw(|frame| {
                sqln::ui::form::draw_form(frame, &form, &Theme::plain());
            })
            .unwrap();
        let shown = rows(terminal.backend().buffer()).join("\n");
        for button in ["Save", "Cancel"] {
            assert!(shown.contains(button), "{button}:\n{shown}");
        }
        seen.push_str(&shown);
    }
    for label in FORM_LABELS.iter().chain(&["Scope", "Idle limit"]) {
        assert!(seen.contains(label), "{label}:\n{seen}");
    }
}

#[test]
fn a_click_selects_a_field_in_each_placement() {
    for (width, height) in [(100, 40), (130, 26), (100, 30), (80, 24)] {
        let mut screen = form_screen("form-click", Theme::plain());
        open_section(&mut screen, (width, height), "Security");
        assert_eq!(screen.app.form().unwrap().shown_section(), Group::Security);
        let buffer = screen.buffer(width, height);
        let lines = rows(&buffer);
        let row = lines
            .iter()
            .position(|line| line.contains("CA file"))
            .unwrap();
        let byte = lines[row].find("CA file").unwrap();
        let column = lines[row][..byte].chars().count() as u16;
        screen.click(column, row as u16);
        assert_eq!(
            screen.app.form().unwrap().focus,
            Field::CaFile,
            "{width} x {height}"
        );
    }
}

/// Opens the form of a new SQLite connection, with the focus on its file.
fn sqlite_form_screen(name: &str) -> Screen {
    let mut screen = form_screen(name, Theme::plain());
    screen.code(KeyCode::Right);
    screen.code(KeyCode::Right);
    focus_field(&mut screen, Field::File);
    screen
}

fn focus_field(screen: &mut Screen, field: Field) {
    common::focus_form_field(&mut screen.app, field);
}

/// Clicks a section of the sidebar of the form.
fn open_section(screen: &mut Screen, (width, height): (u16, u16), title: &str) {
    let lines = rows(&screen.buffer(width, height));
    let row = lines.iter().position(|line| line.contains(title)).unwrap();
    let byte = lines[row].find(title).unwrap();
    screen.click(lines[row][..byte].chars().count() as u16, row as u16);
}

#[test]
fn the_file_browser_fills_the_file_of_a_sqlite_connection() {
    let dir = temp_dir("form-browse");
    std::fs::write(dir.join("shop.db"), "").unwrap();
    let dir = dir.canonicalize().unwrap();
    let mut screen = sqlite_form_screen("form-browse");
    assert!(screen.app.hint().contains("Ctrl+O browse"));
    screen.write(&format!("{}/", dir.display()));

    screen.app.on_event(Event::Key(ctrl('o')));
    assert!(screen.screen(100, 40).contains("Use this folder"));
    screen.type_text("shop");
    screen.code(KeyCode::Enter);
    assert!(!screen.screen(100, 40).contains("Choose a file"));
    let form = screen.app.form().unwrap();
    let path = dir.join("shop.db");
    assert_eq!(form.text(Field::File), path.display().to_string());
    assert_eq!(form.cursor, form.text(Field::File).chars().count());
}

#[test]
fn use_this_folder_leaves_the_file_of_a_connection_ready_for_a_name() {
    let dir = temp_dir("form-browse-folder").canonicalize().unwrap();
    let mut screen = sqlite_form_screen("form-browse-folder");
    screen.write(&format!("{}/", dir.display()));
    screen.app.on_event(Event::Key(ctrl('o')));
    screen.code(KeyCode::Enter);
    screen.type_text("new.db");
    assert_eq!(
        screen.app.form().unwrap().text(Field::File),
        dir.join("new.db").display().to_string()
    );
}

#[test]
fn the_button_of_a_field_opens_the_file_browser_for_that_field() {
    for (width, height) in [(100, 40), (130, 26), (100, 30), (80, 24)] {
        let mut screen = form_screen("form-browse-tls", Theme::plain());
        open_section(&mut screen, (width, height), "Security");
        let shown = screen.screen(width, height);
        assert_eq!(shown.matches("[...]").count(), 3, "{width} x {height}");
        let (y, row) = shown
            .lines()
            .enumerate()
            .find(|(_, row)| row.contains("Client key"))
            .unwrap();
        let label = row.find("Client key").unwrap();
        let button = label + row[label..].find("[...]").unwrap();
        screen.click(row[..button].chars().count() as u16, y as u16);
        assert_eq!(screen.app.form().unwrap().focus, Field::ClientKey);
        let shown = screen.screen(width, height);
        assert!(shown.contains("Choose a file"), "{shown}");
        assert!(!shown.contains("Use this folder"), "{shown}");

        screen.code(KeyCode::Esc);
        assert!(!screen.screen(width, height).contains("Choose a file"));
        assert_eq!(screen.app.form().unwrap().text(Field::ClientKey), "");
    }
}

#[test]
fn another_field_does_not_open_the_file_browser() {
    let mut screen = form_screen("form-browse-name", Theme::plain());
    focus_field(&mut screen, Field::Name);
    assert!(!screen.app.hint().contains("Ctrl+O"));
    screen.app.on_event(Event::Key(ctrl('o')));
    assert!(!screen.screen(100, 40).contains("Choose a file"));
}

/// Returns the columns of the top border of a window.
fn window_width(screen: &str, title: &str) -> usize {
    let row = screen.lines().find(|row| row.contains(title)).unwrap();
    let first = row.chars().position(|c| c == '╭').unwrap();
    let last = row.chars().position(|c| c == '╮').unwrap();
    last - first + 1
}

#[test]
fn the_form_grows_with_the_screen_and_keeps_its_placement() {
    let mut screen = form_screen("form-grow", Theme::plain());
    for (width, height, expected) in [(80, 24, 76), (100, 40, 92), (160, 40, 140), (200, 26, 140)] {
        let shown = screen.screen(width, height);
        let title = "New connection";
        assert_eq!(window_width(&shown, title), expected, "{width} x {height}");
    }
}

#[test]
fn a_click_selects_a_field_of_a_wide_form() {
    let mut screen = form_screen("form-grow-click", Theme::plain());
    open_section(&mut screen, (200, 50), "Security");
    let lines = rows(&screen.buffer(200, 50));
    let row = lines
        .iter()
        .position(|line| line.contains("CA file"))
        .unwrap();
    let button = lines[row].find("[...]").unwrap();
    screen.click(lines[row][..button].chars().count() as u16, row as u16);
    assert_eq!(screen.app.form().unwrap().focus, Field::CaFile);
    assert!(screen.screen(200, 50).contains("Choose a file"));
}

#[test]
fn a_long_path_stays_away_from_the_button() {
    let mut screen = sqlite_form_screen("form-browse-long");
    screen.write(&"x".repeat(200));
    let shown = screen.screen(100, 40);
    // The cursor takes the column after the text, and a gap comes before the button.
    assert!(shown.contains("xx  [...]"), "{shown}");
}

#[test]
fn the_form_keeps_a_note_visible_in_each_placement() {
    for (width, height) in [(100, 40), (130, 26), (100, 30), (80, 24)] {
        let mut screen = form_screen("form-note", Theme::plain());
        for _ in 0..32 {
            if screen.app.form().unwrap().focus == Field::Save {
                break;
            }
            screen.code(KeyCode::Tab);
        }
        screen.code(KeyCode::Enter);
        let shown = screen.screen(width, height);
        assert!(
            shown.contains("name"),
            "the note of the missing name:\n{shown}"
        );
    }
}

/// Draws the form with a note. It returns the screen and the rows of the note and the buttons.
fn form_with_note(note: FormNote, theme: &Theme) -> (Buffer, Vec<String>, usize) {
    let mut form = ConnectionForm::new_in(false);
    form.note = Some(note);
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    terminal
        .draw(|frame| {
            sqln::ui::form::draw_form(frame, &form, theme);
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let lines = rows(&buffer);
    let buttons = lines
        .iter()
        .position(|line| line.contains("Test connection"))
        .unwrap();
    (buffer, lines, buttons)
}

#[test]
fn a_short_note_stands_on_the_row_above_the_buttons() {
    let note = FormNote::Success("The connection works.".to_owned());
    let (_, lines, buttons) = form_with_note(note, &Theme::plain());
    assert!(
        lines[buttons - 1].contains("The connection works."),
        "{}",
        lines.join("\n")
    );
    assert!(
        lines[buttons - 2].chars().all(|c| c == ' ' || c == '│'),
        "{}",
        lines.join("\n")
    );
}

#[test]
fn a_long_note_uses_both_rows_and_ends_above_the_buttons() {
    let message = "start ".repeat(20) + "last";
    let (_, lines, buttons) = form_with_note(FormNote::Error(message), &Theme::plain());
    assert!(lines[buttons - 2].contains("start"), "{}", lines.join("\n"));
    assert!(lines[buttons - 1].contains("last"), "{}", lines.join("\n"));
}

#[test]
fn each_kind_of_note_has_its_own_color() {
    let theme = Theme::astrodark();
    let color_of = |note: FormNote| {
        let (buffer, lines, buttons) = form_with_note(note, &theme);
        let column = lines[buttons - 1].find("message").unwrap();
        let column = lines[buttons - 1][..column].chars().count() as u16;
        buffer[(column, buttons as u16 - 1)].fg
    };
    assert_eq!(
        color_of(FormNote::Success("message".to_owned())),
        theme.mark_good.fg.unwrap()
    );
    assert_eq!(
        color_of(FormNote::Info("message".to_owned())),
        theme.status.fg.unwrap()
    );
    assert_eq!(
        color_of(FormNote::Error("message".to_owned())),
        theme.error.fg.unwrap()
    );
    assert_ne!(theme.mark_good.fg, theme.status.fg);
}

#[test]
fn a_working_connection_gives_a_success_note_and_a_test_in_progress_does_not() {
    let path = temp_db("form-success");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(SETUP)
        .unwrap();
    let theme = Theme::astrodark();
    let mut screen = form_screen("form-success-start", theme);
    screen.code(KeyCode::Left);
    screen.code(KeyCode::Left);
    focus_field(&mut screen, Field::Name);
    screen.type_text("checked");
    focus_field(&mut screen, Field::File);
    screen.write(path.to_str().unwrap());
    focus_field(&mut screen, Field::Test);
    screen.code(KeyCode::Enter);
    assert!(matches!(
        screen.app.form().unwrap().note,
        Some(FormNote::Info(_))
    ));
    screen.pump_until("The connection works.");
    assert!(matches!(
        screen.app.form().unwrap().note,
        Some(FormNote::Success(_))
    ));
    let (column, row) = screen.locate("The connection works.");
    assert_eq!(
        screen.buffer(100, 30)[(column, row)].fg,
        theme.mark_good.fg.unwrap()
    );
    let (_, buttons) = screen.locate("Test connection");
    assert_eq!(row + 1, buttons);
}

fn themed_store(name: &str, preferences: &str) -> (Store, Root) {
    let base = temp_dir(name);
    let global = Root::global(base.join("global"));
    fs::create_dir_all(&global.dir).unwrap();
    let text = format!(r#"{{"version":1,"preferences":{preferences},"connections":[]}}"#);
    fs::write(global.config_file(), text).unwrap();
    (
        Store {
            global: global.clone(),
            project: None,
        },
        global,
    )
}

#[test]
fn the_configuration_rejects_an_unknown_theme_and_an_unknown_icon_set() {
    for preferences in [r#"{"theme":"neon"}"#, r#"{"icons":"emoji"}"#] {
        let (store, _) = themed_store("theme-invalid", preferences);
        assert!(
            sqln::store::config::load(&store).config.is_err(),
            "{preferences}"
        );
    }
    let (store, _) = themed_store("theme-valid", r#"{"theme":"ansi","icons":"nerd"}"#);
    let config = sqln::store::config::load(&store).config.unwrap();
    assert_eq!(
        (config.theme, config.icons),
        (Some(ThemeName::Ansi), Some(IconName::Nerd))
    );
}

#[test]
fn a_reload_changes_the_theme_at_once() {
    if ColorEnv::read().no_color {
        return;
    }
    let (store, global) = themed_store("theme-reload", "{}");
    let options = Options {
        store: Some(store),
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let mut screen = Screen {
        app: App::new(options, clipboard, Theme::plain()),
    };
    assert!(
        screen
            .buffer(100, 30)
            .content()
            .iter()
            .all(|cell| cell.bg == Color::Reset)
    );

    let text = r#"{"version":1,"preferences":{"theme":"astrodark"},"connections":[]}"#;
    fs::write(global.config_file(), text).unwrap();
    screen.prefix('r');
    assert!(
        screen
            .buffer(100, 30)
            .content()
            .iter()
            .all(|cell| is_rgb(cell.bg))
    );

    let text = r#"{"version":1,"preferences":{},"connections":[]}"#;
    fs::write(global.config_file(), text).unwrap();
    screen.prefix('r');
    assert!(
        screen
            .buffer(100, 30)
            .content()
            .iter()
            .all(|cell| cell.bg == Color::Reset)
    );
}

#[test]
fn the_focus_stays_in_the_console_after_a_command() {
    let mut screen = Screen::new("palette-focus", Theme::plain(), KeyboardMode::Conventional);
    screen.run("SELECT 1;");
    screen.palette();
    screen.write("results");
    screen.code(KeyCode::Enter);
    assert_eq!(screen.app.console().focus, Focus::Results);
}
