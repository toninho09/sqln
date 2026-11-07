// The size of each modal: it grows with the screen, up to a limit.

use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::{Frame, Terminal};
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::editor::KeyboardMode;
use sqln::input::Action;
use sqln::palette::{Command, Palette};
use sqln::settings::SettingsForm;
use sqln::store::config::Preferences;
use sqln::ui::auth::{PasswordPrompt, draw_password};
use sqln::ui::console::{draw_dialog, draw_overlay, draw_picker};
use sqln::ui::name::{NamePrompt, NameTarget, draw_name};
use sqln::ui::palette::draw_palette;
use sqln::ui::settings::draw_settings;
use sqln::ui::theme::Theme;
use sqln::ui::widgets::short_modal_width;
use sqln::ui::wrap::WrappedText;
use sqln::ui::{ConflictRowView, draw_conflicts, text};

fn drawn(width: u16, height: u16, mut draw: impl FnMut(&mut Frame, &Theme)) -> String {
    let theme = Theme::plain();
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| draw(frame, &theme)).unwrap();
    let buffer = terminal.backend().buffer();
    buffer
        .content()
        .chunks(usize::from(width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Returns the columns of the window on the screen, from its top border.
fn window_width(screen: &str) -> usize {
    let row = screen.lines().find(|row| row.contains('╭')).unwrap();
    let chars: Vec<char> = row.chars().collect();
    let first = chars.iter().position(|c| *c == '╭').unwrap();
    let last = chars.iter().position(|c| *c == '╮').unwrap();
    last - first + 1
}

/// Checks the width on a small, a medium, and a very wide screen.
fn check_widths(limit: usize, mut draw: impl FnMut(&mut Frame, &Theme)) {
    assert_eq!(window_width(&drawn(80, 24, &mut draw)), 72);
    assert_eq!(window_width(&drawn(100, 30, &mut draw)), 92);
    assert_eq!(window_width(&drawn(300, 60, &mut draw)), limit);
}

fn commands(count: usize) -> Palette {
    let command = Command {
        action: Action::Help,
        label: "a-command".to_owned(),
        key: None,
    };
    Palette::new(vec![command; count])
}

fn conflict_rows(names: &[String]) -> Vec<ConflictRowView<'_>> {
    names
        .iter()
        .map(|name| ConflictRowView {
            name,
            removed: false,
            choice: None,
        })
        .collect()
}

#[test]
fn the_settings_grow_like_the_export_dialog() {
    let form = SettingsForm::new(Preferences::default(), None);
    check_widths(140, |frame, theme| {
        draw_settings(frame, &form, theme);
    });
}

#[test]
fn the_palette_grows_to_its_limit() {
    let palette = commands(3);
    check_widths(100, |frame, theme| {
        draw_palette(frame, &palette, theme);
    });
}

#[test]
fn a_list_grows_to_its_limit() {
    check_widths(100, |frame, theme| {
        draw_picker(frame, "Choose", &["one"], 0, false, theme);
    });
}

#[test]
fn the_conflicts_grow_to_their_limit() {
    let names = ["one".to_owned()];
    let rows = conflict_rows(&names);
    check_widths(100, |frame, theme| {
        draw_conflicts(frame, &rows, 0, None, theme);
    });
}

#[test]
fn the_name_prompt_grows_to_its_limit() {
    let prompt = NamePrompt::new(NameTarget::Console(1), "name");
    check_widths(100, |frame, theme| draw_name(frame, &prompt, theme));
}

#[test]
fn the_password_prompt_grows_to_its_limit() {
    let prompt = PasswordPrompt::new("name", "db.example.com".to_owned(), true);
    check_widths(100, |frame, theme| {
        draw_password(frame, &prompt, theme);
    });
}

#[test]
fn a_question_grows_to_its_limit() {
    check_widths(100, |frame, theme| {
        draw_dialog(frame, "Title", "Body", &["Yes"], 0, theme);
    });
}

#[test]
fn the_detail_window_grows_to_its_limit() {
    let mut contents = WrappedText::new("text".to_owned());
    check_widths(140, |frame, theme| {
        draw_overlay(frame, "Title", &mut contents, "", 0, theme);
    });
}

#[test]
fn a_screen_smaller_than_a_window_keeps_the_whole_window_on_it() {
    assert_eq!(short_modal_width(60, 48), 52);
    assert_eq!(short_modal_width(50, 48), 48);
    let palette = commands(3);
    let shown = drawn(40, 20, |frame, theme| {
        draw_palette(frame, &palette, theme);
    });
    assert_eq!(window_width(&shown), 40);
}

#[test]
fn a_tall_screen_shows_more_commands_up_to_the_limit() {
    let palette = commands(60);
    let count = |height| {
        let shown = drawn(100, height, |frame, theme| {
            draw_palette(frame, &palette, theme);
        });
        shown.matches("a-command").count()
    };
    assert_eq!(count(24), 17);
    assert_eq!(count(60), 30);
    assert_eq!(count(120), 30);
}

#[test]
fn the_height_of_a_list_follows_its_items() {
    let items: Vec<String> = (0..60).map(|index| format!("item-{index}")).collect();
    let items: Vec<&str> = items.iter().map(String::as_str).collect();
    let count = |rows: usize, height| {
        let shown = drawn(100, height, |frame, theme| {
            draw_picker(frame, "Choose", &items[..rows], 0, false, theme);
        });
        shown.matches("item-").count()
    };
    assert_eq!(count(60, 24), 17);
    assert_eq!(count(60, 60), 30);
    assert_eq!(count(60, 120), 30);
    assert_eq!(count(3, 60), 3);
}

#[test]
fn a_tall_screen_shows_more_conflicts_and_the_buttons_stay() {
    let names: Vec<String> = (0..60).map(|index| format!("script-{index}")).collect();
    let rows = conflict_rows(&names);
    let shown = |height| {
        drawn(100, height, |frame, theme| {
            draw_conflicts(frame, &rows, 0, None, theme);
        })
    };
    assert_eq!(shown(24).matches("script-").count(), 14);
    let tall = shown(60);
    assert_eq!(tall.matches("script-").count(), 30);
    assert!(tall.contains(text::BUTTON_APPLY), "{tall}");
}

#[test]
fn a_wide_screen_shows_a_long_name_of_a_conflict() {
    let name = "a-long-name-of-a-script-that-another-program-changed-1234";
    let names = [name.to_owned()];
    let rows = conflict_rows(&names);
    let at = |width| {
        drawn(width, 24, |frame, theme| {
            draw_conflicts(frame, &rows, 0, None, theme);
        })
    };
    let small = at(74);
    assert!(!small.contains(name), "{small}");
    let wide = at(120);
    assert!(wide.contains(name), "{wide}");
    assert!(wide.contains(text::STATE_CHANGED), "{wide}");
}

fn app() -> App {
    let clipboard = std::rc::Rc::new(std::cell::RefCell::new(MemoryClipboard::default()));
    let options = Options {
        keyboard: KeyboardMode::Conventional,
        ..Options::default()
    };
    App::new(options, clipboard, Theme::plain())
}

fn app_screen(app: &mut App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    buffer
        .content()
        .chunks(usize::from(width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect()
}

fn click_text(app: &mut App, rows: &[String], label: &str) {
    let (y, row) = rows
        .iter()
        .enumerate()
        .find(|(_, row)| row.contains(label))
        .unwrap_or_else(|| panic!("{label:?} is not on the screen:\n{}", rows.join("\n")));
    let byte = row.find(label).unwrap();
    app.on_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: row[..byte].chars().count() as u16,
        row: y as u16,
        modifiers: KeyModifiers::NONE,
    }));
}

#[test]
fn a_click_runs_a_command_below_the_rows_of_a_small_screen() {
    let mut app = app();
    app.on_event(Event::Key(crate::common::ctrl('p')));
    let rows = app_screen(&mut app, 200, 60);
    let place = rows
        .iter()
        .position(|row| row.contains("Open the settings"))
        .unwrap();
    // The list starts at row 4, and a small screen would show 12 rows.
    assert!(place >= 16, "the command is on row {place}");

    click_text(&mut app, &rows, "Open the settings");
    let shown = app_screen(&mut app, 200, 60).join("\n");
    assert!(!shown.contains(text::TITLE_PALETTE), "{shown}");
    assert!(shown.contains("no configuration files"), "{shown}");
}

#[test]
fn the_mouse_gets_all_rows_of_a_long_list() {
    let items: Vec<String> = (0..30).map(|index| format!("item-{index}")).collect();
    let items: Vec<&str> = items.iter().map(String::as_str).collect();
    let mut terminal = Terminal::new(TestBackend::new(120, 60)).unwrap();
    let mut area = (Rect::default(), usize::MAX);
    terminal
        .draw(|frame| {
            area = draw_picker(frame, "Choose", &items, 29, false, &Theme::plain());
        })
        .unwrap();
    // With a cursor on the last row, a list that needs no scroll starts at its first row.
    assert_eq!(area.1, 0);
    assert!(area.0.height >= 30, "{:?}", area.0);
}
