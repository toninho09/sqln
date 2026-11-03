// The key reference groups, search, scrolling, and event isolation.

mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use common::{ctrl, key};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::editor::KeyboardMode;
use sqln::help::Help;
use sqln::input::Keymap;
use sqln::ui::help::draw_help;
use sqln::ui::text;
use sqln::ui::theme::Theme;
use sqln::ui::which_key::KeyGroup;

fn reference() -> Help {
    Help::new(
        "Keys".to_owned(),
        vec![
            KeyGroup {
                title: "Editor",
                keys: vec![("Ctrl+F".to_owned(), "Find and replace")],
            },
            KeyGroup {
                title: "Empty",
                keys: vec![],
            },
            KeyGroup {
                title: "Run",
                keys: (0..50)
                    .map(|index| (format!("key-{index}"), "Run the statement"))
                    .collect(),
            },
        ],
    )
}

fn screen(width: u16, height: u16, draw: impl FnOnce(&mut ratatui::Frame)) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(draw).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .chunks(usize::from(width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn draw(help: &mut Help, width: u16, height: u16) -> String {
    screen(width, height, |frame| {
        draw_help(frame, help, &Theme::plain())
    })
}

#[test]
fn the_reference_omits_empty_groups_and_cycles_in_both_directions() {
    let mut help = reference();
    assert_eq!(help.groups.len(), 2);
    assert_eq!(help.group, None);
    help.on_key(key(KeyCode::Tab));
    assert_eq!(help.group, Some(0));
    let shown = draw(&mut help, 80, 24);
    assert!(shown.contains("Group: Editor"), "{shown}");
    assert!(!shown.contains("Run the statement"), "{shown}");
    help.on_key(key(KeyCode::Tab));
    assert_eq!(help.group, Some(1));
    help.on_key(key(KeyCode::Tab));
    assert_eq!(help.group, None);
    help.on_key(key(KeyCode::BackTab));
    assert_eq!(help.group, Some(1));
    help.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));
    assert_eq!(help.group, Some(0));
}

#[test]
fn search_words_match_the_group_the_key_and_the_description() {
    let mut help = reference();
    help.paste("EDITOR ctrl+f REPLACE");
    let shown = draw(&mut help, 80, 24);
    assert!(shown.contains("Find and replace"), "{shown}");
    assert!(!shown.contains("Run the statement"), "{shown}");
    assert!(shown.contains("1 command"), "{shown}");
    help.on_key(ctrl('u'));
    help.paste("no-such-command");
    let shown = draw(&mut help, 80, 24);
    assert!(shown.contains(text::NO_COMMANDS), "{shown}");
    assert!(!shown.contains("Find and replace"), "{shown}");
    help.on_key(ctrl('u'));
    assert!(draw(&mut help, 80, 24).contains("Find and replace"));
}

#[test]
fn pages_and_end_keys_use_the_viewport_and_stop_at_the_last_page() {
    let mut help = reference();
    let first = draw(&mut help, 80, 24);
    assert!(first.contains('▐'));
    let page = help.visible_rows;
    help.on_key(key(KeyCode::PageDown));
    draw(&mut help, 80, 24);
    assert_eq!(help.scroll, page);
    help.on_key(key(KeyCode::PageUp));
    assert_eq!(help.scroll, 0);
    help.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL));
    let last = draw(&mut help, 80, 24);
    assert!(last.contains("key-49"), "{last}");
    let end = help.scroll;
    help.on_key(KeyEvent::new(KeyCode::Home, KeyModifiers::CONTROL));
    assert_eq!(help.scroll, 0);
    help.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL));
    draw(&mut help, 80, 24);
    help.on_key(key(KeyCode::Down));
    draw(&mut help, 80, 24);
    assert_eq!(help.scroll, end);
    draw(&mut help, 140, 100);
    assert_eq!(help.scroll, 0);
    help.on_key(key(KeyCode::PageDown));
    help.paste("replace");
    assert_eq!(help.scroll, 0);
}

#[test]
fn the_search_field_keeps_its_cursor_keys_and_group_changes_keep_the_filter() {
    let mut help = reference();
    help.paste("replace");
    help.on_key(key(KeyCode::Home));
    help.on_key(key(KeyCode::Char('x')));
    assert_eq!(help.input.text(), "xreplace");
    help.on_key(key(KeyCode::Backspace));
    help.on_key(key(KeyCode::End));
    assert_eq!(help.input.cursor(), 7);
    help.on_key(key(KeyCode::Tab));
    assert_eq!(help.input.text(), "replace");
    assert_eq!(help.scroll, 0);
}

#[test]
fn long_keys_and_descriptions_wrap_at_the_minimum_width() {
    let mut help = Help::new(
        "Keys".to_owned(),
        vec![KeyGroup {
            title: "Run",
            keys: vec![(
                "ctrl+enter, f5, ctrl+w e, alt+enter, ctrl+shift+r".to_owned(),
                "Run the selection or the statement without cutting the description",
            )],
        }],
    );
    let shown = draw(&mut help, 80, 24);
    for part in ["ctrl+enter", "ctrl+shift+r", "cutting the description"] {
        assert!(shown.contains(part), "{part}:\n{shown}");
    }
    for (width, height) in [(1, 1), (4, 3), (12, 6), (300, 100)] {
        draw(&mut help, width, height);
    }
}

#[test]
fn an_empty_reference_keeps_group_navigation_valid() {
    let mut help = Help::new("Keys".to_owned(), vec![]);
    help.on_key(key(KeyCode::Tab));
    help.on_key(key(KeyCode::BackTab));
    assert_eq!(help.group, None);
    assert!(draw(&mut help, 80, 24).contains(text::NO_COMMANDS));
}

fn app(options: Options) -> App {
    App::new(
        options,
        Rc::new(RefCell::new(MemoryClipboard::default())),
        Theme::plain(),
    )
}

fn app_screen(app: &mut App) -> String {
    screen(100, 30, |frame| app.draw(frame))
}

#[test]
fn f1_filters_effective_keys_and_omits_removed_actions() {
    let overrides = HashMap::from([
        ("run".to_owned(), vec!["alt+r".to_owned()]),
        ("quit".to_owned(), vec![]),
    ]);
    let keymap = Keymap::with_overrides(KeyboardMode::Conventional, &overrides).unwrap();
    let mut app = app(Options {
        keymap: Some(keymap),
        ..Options::default()
    });
    app.on_event(Event::Key(key(KeyCode::F(1))));
    app.on_event(Event::Paste("ALT+R selection".to_owned()));
    let shown = app_screen(&mut app);
    assert!(
        shown.contains("Run the selection or the statement"),
        "{shown}"
    );
    assert!(shown.contains("alt+r"), "{shown}");
    assert!(!shown.contains("ctrl+enter"), "{shown}");
    app.on_event(Event::Key(ctrl('u')));
    app.on_event(Event::Paste("quit".to_owned()));
    assert!(app_screen(&mut app).contains(text::NO_COMMANDS));
}

#[test]
fn help_captures_typing_paste_command_keys_and_mouse_events() {
    let mut app = app(Options::default());
    app.on_event(Event::Paste("SELECT 1;".to_owned()));
    let before = app.console().editor.text();
    app.on_event(Event::Key(key(KeyCode::F(1))));
    for c in "format".chars() {
        app.on_event(Event::Key(key(KeyCode::Char(c))));
    }
    app.on_event(Event::Key(key(KeyCode::F(5))));
    let shown = app_screen(&mut app);
    assert!(shown.contains("Format the SQL"), "{shown}");
    assert!(!app.console().is_busy());
    app.on_event(Event::Key(ctrl('u')));
    let first = app_screen(&mut app);
    app.on_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 10,
        row: 10,
        modifiers: KeyModifiers::NONE,
    }));
    assert_ne!(app_screen(&mut app), first);
    assert_eq!(app.console().editor.text(), before);
    app.on_event(Event::Key(key(KeyCode::Esc)));
    assert!(!app_screen(&mut app).contains("Keys · Conventional"));
    app.on_event(Event::Key(key(KeyCode::F(1))));
    assert!(app_screen(&mut app).contains(text::HELP_SEARCH));
    app.on_event(Event::Key(key(KeyCode::F(1))));
    assert!(!app_screen(&mut app).contains("Keys · Conventional"));
}
