// The guide, the key reference groups, search, scrolling, and event isolation.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::common::{alt, ctrl, key};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::editor::KeyboardMode;
use sqln::help::{GuideTopic, Help, View};
use sqln::input::Keymap;
use sqln::ui::help::draw_help;
use sqln::ui::text;
use sqln::ui::theme::Theme;
use sqln::ui::which_key::KeyGroup;

fn guide() -> Vec<GuideTopic> {
    vec![
        GuideTopic {
            name: "Connect",
            title: "1. Connect",
            intro: "A connection holds the address of a database.",
            steps: vec![("Ctrl+W h a".to_owned(), "Add a connection")],
            examples: &[],
        },
        GuideTopic {
            name: "Kafka",
            title: "2. Kafka",
            intro: "A plain SELECT reads the retained messages.",
            steps: vec![("F5".to_owned(), "Run the statement")],
            examples: &[
                ("List the topics", "TOPICS"),
                (
                    "Watch the new messages",
                    "SELECT * FROM orders EMIT CHANGES;",
                ),
            ],
        },
    ]
}

fn groups() -> Vec<KeyGroup> {
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
    ]
}

/// Returns a help on the page of all keys, as the tests of the keys expect.
fn reference() -> Help {
    let mut help = Help::new("Keys".to_owned(), guide(), groups());
    help.open_page(2);
    assert_eq!(help.view(), View::All);
    help
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
fn the_sidebar_lists_the_pages_and_tab_cycles_in_both_directions() {
    let mut help = Help::new("Keys".to_owned(), guide(), groups());
    assert_eq!(help.groups.len(), 2);
    assert_eq!(help.view(), View::Topic(0));
    assert_eq!(
        help.page_titles(),
        ["Connect", "Kafka", text::HELP_ALL, "Editor", "Run"]
    );
    let shown = draw(&mut help, 80, 24);
    for title in ["Connect", "Kafka", text::HELP_ALL, "Editor"] {
        assert!(shown.contains(title), "{title}:\n{shown}");
    }
    let order = [
        View::Topic(1),
        View::All,
        View::Group(0),
        View::Group(1),
        View::Topic(0),
    ];
    for view in order {
        help.on_key(key(KeyCode::Tab));
        assert_eq!(help.view(), view);
    }
    help.on_key(key(KeyCode::BackTab));
    assert_eq!(help.view(), View::Group(1));
    help.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));
    assert_eq!(help.view(), View::Group(0));
    let shown = draw(&mut help, 80, 24);
    assert!(shown.contains("Find and replace"), "{shown}");
    assert!(!shown.contains("Run the statement"), "{shown}");
}

#[test]
fn a_click_on_the_sidebar_opens_the_page() {
    let mut help = Help::new("Keys".to_owned(), guide(), groups());
    draw(&mut help, 80, 24);
    let (_, row) = help.page_rows[1];
    assert!(help.click(row.x + 2, row.y));
    assert_eq!(help.view(), View::Topic(1));
    assert!(draw(&mut help, 80, 24).contains("2. Kafka"));
    assert!(!help.click(0, 0));
    assert_eq!(help.view(), View::Topic(1));
}

#[test]
fn a_topic_page_shows_its_steps_and_examples_and_the_search_filters_them() {
    let mut help = Help::new("Keys".to_owned(), guide(), groups());
    let shown = draw(&mut help, 80, 24);
    assert!(shown.contains(text::HELP_GUIDE_SEARCH), "{shown}");
    assert!(shown.contains("A connection holds the address"), "{shown}");
    assert!(shown.contains("Ctrl+W h a"), "{shown}");
    assert!(!shown.contains("TOPICS"), "{shown}");
    assert!(!shown.contains("Find and replace"), "{shown}");
    help.on_key(key(KeyCode::Tab));
    let shown = draw(&mut help, 80, 24);
    assert!(
        shown.contains("SELECT * FROM orders EMIT CHANGES;"),
        "{shown}"
    );
    assert!(shown.contains("3 items"), "{shown}");
    help.paste("emit");
    let shown = draw(&mut help, 80, 24);
    assert!(shown.contains("Watch the new messages"), "{shown}");
    assert!(shown.contains("A plain SELECT"), "{shown}");
    assert!(!shown.contains("List the topics"), "{shown}");
    assert!(shown.contains("1 item"), "{shown}");
    help.paste(" no-such-word");
    assert!(draw(&mut help, 80, 24).contains(text::NO_ITEMS));
    // The key list never shows the guide text.
    help.on_key(ctrl('u'));
    help.paste("watch");
    help.on_key(key(KeyCode::Tab));
    assert!(draw(&mut help, 80, 24).contains(text::NO_COMMANDS));
}

#[test]
fn the_sidebar_keeps_the_open_page_visible_on_a_short_screen() {
    let many: Vec<KeyGroup> = (0..30)
        .map(|_| KeyGroup {
            title: "Group",
            keys: vec![("k".to_owned(), "Label")],
        })
        .collect();
    let mut help = Help::new("Keys".to_owned(), guide(), many);
    help.open_page(32);
    draw(&mut help, 80, 24);
    let pages: Vec<usize> = help.page_rows.iter().map(|(page, _)| *page).collect();
    assert!(pages.contains(&32), "{pages:?}");
    let bottom = help.page_rows.last().unwrap().1.y;
    assert!(bottom < 23, "{bottom}");
}

#[test]
fn the_sidebar_scrolls_the_minimum_that_keeps_the_page_visible() {
    let many: Vec<KeyGroup> = (0..30)
        .map(|_| KeyGroup {
            title: "Group",
            keys: vec![("k".to_owned(), "Label")],
        })
        .collect();
    let mut help = Help::new("Keys".to_owned(), guide(), many);
    help.open_page(32);
    draw(&mut help, 80, 24);
    assert_eq!(help.page_rows.first().unwrap().0, 14);
    // Moving up keeps the window: the page moves inside it, not to the bottom.
    help.on_key(key(KeyCode::BackTab));
    help.on_key(key(KeyCode::BackTab));
    draw(&mut help, 80, 24);
    let pages: Vec<usize> = help.page_rows.iter().map(|(page, _)| *page).collect();
    assert_eq!(pages.first(), Some(&14), "{pages:?}");
    assert!(pages.contains(&30), "{pages:?}");
    // Above the window, the window moves one row, and the page shows at the top.
    for _ in 0..17 {
        help.on_key(key(KeyCode::BackTab));
    }
    draw(&mut help, 80, 24);
    let pages: Vec<usize> = help.page_rows.iter().map(|(page, _)| *page).collect();
    assert_eq!(pages.first(), Some(&13), "{pages:?}");
    assert_eq!(pages.last(), Some(&31), "{pages:?}");
    // The cycle wraps to the first page, and the window follows.
    help.open_page(32);
    help.on_key(key(KeyCode::Tab));
    draw(&mut help, 80, 24);
    assert_eq!(help.page_rows.first().unwrap().0, 0);
}

#[test]
fn the_copy_takes_the_visible_page() {
    let mut help = Help::new("Keys".to_owned(), guide(), groups());
    help.open_page(1);
    let topic = help.text();
    assert!(
        topic.contains("SELECT * FROM orders EMIT CHANGES;"),
        "{topic}"
    );
    assert!(!topic.contains("Find and replace"), "{topic}");
    help.open_page(2);
    let keys = help.text();
    assert!(keys.contains("Find and replace"), "{keys}");
    assert!(!keys.contains("EMIT CHANGES"), "{keys}");
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
        vec![],
        vec![KeyGroup {
            title: "Run",
            keys: vec![(
                "ctrl+enter, f5, ctrl+w e, alt+enter, ctrl+shift+r".to_owned(),
                "Run the selection or the statement without cutting the description",
            )],
        }],
    );
    help.open_page(0);
    assert_eq!(help.view(), View::All);
    let shown = draw(&mut help, 80, 24);
    for part in [
        "ctrl+enter",
        "ctrl+shift+r",
        "without cutting the",
        "description",
    ] {
        assert!(shown.contains(part), "{part}:\n{shown}");
    }
    for (width, height) in [(1, 1), (4, 3), (12, 6), (300, 100)] {
        draw(&mut help, width, height);
    }
}

#[test]
fn an_empty_reference_keeps_group_navigation_valid() {
    let mut help = Help::new("Keys".to_owned(), vec![], vec![]);
    assert_eq!(help.view(), View::All);
    assert!(draw(&mut help, 80, 24).contains(text::NO_COMMANDS));
    help.on_key(key(KeyCode::Tab));
    help.on_key(key(KeyCode::BackTab));
    assert_eq!(help.view(), View::All);
}

fn app(options: Options) -> App {
    app_with_clipboard(options).0
}

fn app_with_clipboard(options: Options) -> (App, Rc<RefCell<MemoryClipboard>>) {
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let app = App::new(options, clipboard.clone(), Theme::plain());
    (app, clipboard)
}

/// Types a filter in the open palette.
fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        app.on_event(Event::Key(key(KeyCode::Char(c))));
    }
}

/// Copies the open help page and returns its text.
fn copied_page(app: &mut App, clipboard: &RefCell<MemoryClipboard>) -> String {
    app.on_event(Event::Key(ctrl('c')));
    clipboard.borrow().text.clone()
}

fn kafka_title() -> &'static str {
    text::GUIDE
        .iter()
        .find(|topic| topic.name == "Kafka")
        .unwrap()
        .title
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
    // The guide shows the effective key of a step.
    app.on_event(Event::Key(key(KeyCode::Tab)));
    let shown = app_screen(&mut app);
    assert!(shown.contains("Run SQL"), "{shown}");
    assert!(shown.contains("Alt+R"), "{shown}");
    for _ in 1..text::GUIDE.len() {
        app.on_event(Event::Key(key(KeyCode::Tab)));
    }
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
    for _ in text::GUIDE {
        app.on_event(Event::Key(key(KeyCode::Tab)));
    }
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
    assert!(!app_screen(&mut app).contains("Help · Conventional"));
    app.on_event(Event::Key(key(KeyCode::F(1))));
    assert!(app_screen(&mut app).contains(text::HELP_GUIDE_SEARCH));
    app.on_event(Event::Key(key(KeyCode::F(1))));
    assert!(!app_screen(&mut app).contains("Help · Conventional"));
}

#[test]
fn the_guide_follows_the_vim_mode_and_shows_the_palette_path_without_a_key() {
    let overrides = HashMap::from([("gridExport".to_owned(), vec![])]);
    let keymap = Keymap::with_overrides(KeyboardMode::Vim, &overrides).unwrap();
    let mut app = app(Options {
        keyboard: KeyboardMode::Vim,
        keymap: Some(keymap),
        ..Options::default()
    });
    app.on_event(Event::Key(key(KeyCode::F(1))));
    app.on_event(Event::Key(key(KeyCode::Tab)));
    app.on_event(Event::Paste("export".to_owned()));
    let shown = screen(80, 24, |frame| app.draw(frame));
    assert!(shown.contains("Help · Vim"), "{shown}");
    assert!(shown.contains("→"), "{shown}");
    assert!(shown.contains("Export the result to a file"), "{shown}");
}

#[test]
fn the_kafka_topic_shows_the_select_examples_at_the_minimum_size() {
    let mut app = app(Options::default());
    app.on_event(Event::Key(key(KeyCode::F(1))));
    let kafka = text::GUIDE
        .iter()
        .position(|topic| topic.name == "Kafka")
        .unwrap();
    for _ in 0..kafka {
        app.on_event(Event::Key(key(KeyCode::Tab)));
    }
    app.on_event(Event::Paste("emit".to_owned()));
    let shown = screen(80, 24, |frame| app.draw(frame));
    assert!(shown.contains("Read Kafka with SELECT"), "{shown}");
    assert!(
        shown.contains("SELECT * FROM orders EMIT CHANGES;"),
        "{shown}"
    );
}

#[test]
fn the_page_text_is_the_copy_of_each_page() {
    let mut help = reference();
    for page in 0..help.page_titles().len() {
        help.open_page(page);
        assert_eq!(help.page_text(page), help.text(), "page {page}");
    }
}

#[test]
fn the_palette_opens_a_help_topic_by_its_name_at_the_minimum_size() {
    let (mut app, clipboard) = app_with_clipboard(Options::default());
    app.on_event(Event::Key(ctrl('p')));
    type_text(&mut app, "help kafka");
    let shown = screen(80, 24, |frame| app.draw(frame));
    assert!(shown.contains(&text::help_command("Kafka")), "{shown}");
    app.on_event(Event::Key(key(KeyCode::Enter)));
    let shown = screen(80, 24, |frame| app.draw(frame));
    assert!(shown.contains(kafka_title()), "{shown}");
    assert!(copied_page(&mut app, &clipboard).starts_with(kafka_title()));
}

#[test]
fn the_palette_finds_a_help_topic_by_a_word_of_its_text() {
    let (mut app, clipboard) = app_with_clipboard(Options::default());
    app.on_event(Event::Key(ctrl('p')));
    type_text(&mut app, "emit changes");
    let shown = app_screen(&mut app);
    assert!(shown.contains(&text::help_command("Kafka")), "{shown}");
    app.on_event(Event::Key(key(KeyCode::Enter)));
    assert!(copied_page(&mut app, &clipboard).starts_with(kafka_title()));
    // The help is open, and the editor got no text.
    assert!(app.console().editor.is_blank());
}

#[test]
fn the_palette_opens_a_key_group_page() {
    let filter = "help: transaction";
    let mut found = false;
    for down in 0..8 {
        let (mut app, clipboard) = app_with_clipboard(Options::default());
        app.on_event(Event::Key(ctrl('p')));
        type_text(&mut app, filter);
        assert!(
            app_screen(&mut app).contains(&text::help_command(text::GROUP_TRANSACTION)),
            "{filter}"
        );
        for _ in 0..down {
            app.on_event(Event::Key(key(KeyCode::Down)));
        }
        app.on_event(Event::Key(key(KeyCode::Enter)));
        let page = copied_page(&mut app, &clipboard);
        if page.starts_with(&format!("{}\n", text::GROUP_TRANSACTION)) {
            found = true;
            break;
        }
    }
    assert!(found, "no row opens the {} page", text::GROUP_TRANSACTION);
}

#[test]
fn the_palette_opens_help_in_vim_mode_with_a_custom_palette_key() {
    let overrides = HashMap::from([("openPalette".to_owned(), vec!["alt+p".to_owned()])]);
    let keymap = Keymap::with_overrides(KeyboardMode::Vim, &overrides).unwrap();
    let (mut app, clipboard) = app_with_clipboard(Options {
        keyboard: KeyboardMode::Vim,
        keymap: Some(keymap),
        ..Options::default()
    });
    app.on_event(Event::Key(alt(KeyCode::Char('p'))));
    type_text(&mut app, "help kafka");
    app.on_event(Event::Key(key(KeyCode::Enter)));
    assert!(copied_page(&mut app, &clipboard).starts_with(kafka_title()));
}
