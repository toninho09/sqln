// The key map: defaults, sequences, and the rules of the `keymap` configuration.

mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use common::{ctrl, key};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::editor::KeyboardMode;
use sqln::input::{Action, Chord, Keymap, Lookup, Region};
use sqln::ui::theme::Theme;

fn chord(text: &str) -> Chord {
    Chord::parse(text).unwrap()
}

fn overrides(items: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
    items
        .iter()
        .map(|(name, keys)| {
            (
                (*name).to_owned(),
                keys.iter().map(|key| (*key).to_owned()).collect(),
            )
        })
        .collect()
}

#[test]
fn a_key_text_reads_modifiers_and_names() {
    let event = |code, modifiers| Chord::from_event(KeyEvent::new(code, modifiers));
    assert_eq!(
        chord("ctrl+enter"),
        event(KeyCode::Enter, KeyModifiers::CONTROL)
    );
    assert_eq!(chord("Shift+F5"), event(KeyCode::F(5), KeyModifiers::SHIFT));
    assert_eq!(chord("alt+up"), event(KeyCode::Up, KeyModifiers::ALT));
    assert_eq!(chord("<"), event(KeyCode::Char('<'), KeyModifiers::NONE));
    assert_eq!(chord("+"), event(KeyCode::Char('+'), KeyModifiers::NONE));
    assert_eq!(
        chord("ctrl++"),
        event(KeyCode::Char('+'), KeyModifiers::CONTROL)
    );
    assert_eq!(
        chord("space"),
        event(KeyCode::Char(' '), KeyModifiers::NONE)
    );
    assert!(Chord::parse("hyper+x").is_err());
    assert!(Chord::parse("nothing").is_err());
}

#[test]
fn shift_adds_nothing_to_a_character() {
    let upper = Chord::from_event(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
    assert_eq!(upper, chord("G"));
}

#[test]
fn the_default_keys_have_no_conflict_in_both_modes() {
    Keymap::new(KeyboardMode::Conventional);
    Keymap::new(KeyboardMode::Vim);
}

#[test]
fn each_mode_has_its_own_panel_keys() {
    let conventional = Keymap::new(KeyboardMode::Conventional);
    let vim = Keymap::new(KeyboardMode::Vim);
    assert_eq!(
        conventional.lookup(Region::Navigator, chord("j")),
        Lookup::None
    );
    assert_eq!(
        vim.lookup(Region::Navigator, chord("j")),
        Lookup::Action(Action::NavigatorDown)
    );
    assert_eq!(
        conventional.lookup(Region::Navigator, chord("delete")),
        Lookup::Action(Action::NavigatorDelete)
    );
    assert_eq!(vim.lookup(Region::Navigator, chord("delete")), Lookup::None);
    assert_eq!(
        vim.lookup(Region::Navigator, chord("x")),
        Lookup::Action(Action::NavigatorDelete)
    );
    assert_eq!(vim.lookup(Region::Grid, chord("g")), Lookup::Sequence);
    assert_eq!(
        vim.after(Region::Grid, chord("g"), chord("g")),
        Some(Action::GridFirstRow)
    );
}

#[test]
fn the_grid_has_the_keys_of_the_table_view_in_both_modes() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let map = Keymap::new(mode);
        let keys = [
            ("f", Action::GridFocusFilter),
            ("o", Action::GridFocusOrder),
            ("s", Action::GridSortColumn),
            ("m", Action::GridLoadMore),
        ];
        for (text, action) in keys {
            assert_eq!(
                map.lookup(Region::Grid, chord(text)),
                Lookup::Action(action),
                "{text}"
            );
            assert!(action.is_table_only());
            assert!(!action.is_console_only());
        }
    }
    assert!(Action::GridCloseResult.is_console_only());
    assert!(!Action::GridCopy.is_table_only() && !Action::GridCopy.is_console_only());
}

#[test]
fn long_sequences_can_share_partial_prefixes() {
    let items = overrides(&[("run", &["f8 a b c"]), ("stop", &["f8 a d"])]);
    let map = Keymap::with_overrides(KeyboardMode::Conventional, &items).unwrap();
    let keys = |text: &str| text.split_whitespace().map(chord).collect::<Vec<_>>();
    for prefix in ["f8", "f8 a", "f8 a b"] {
        assert_eq!(
            map.sequence(Region::Global, &keys(prefix)),
            Lookup::Sequence
        );
    }
    assert_eq!(
        map.sequence(Region::Global, &keys("f8 a b c")),
        Lookup::Action(Action::Run)
    );
    assert_eq!(
        map.sequence(Region::Global, &keys("f8 a d")),
        Lookup::Action(Action::Stop)
    );
    assert_eq!(map.sequence(Region::Global, &keys("f8 a x")), Lookup::None);
}

#[test]
fn a_complete_sequence_cannot_be_another_sequences_prefix_in_either_order() {
    for (run, stop) in [("f8 a b", "f8 a"), ("f8 a", "f8 a b"), ("f8 a b", "f8 a b")] {
        let items = overrides(&[("run", &[run]), ("stop", &[stop])]);
        let error = Keymap::with_overrides(KeyboardMode::Conventional, &items).unwrap_err();
        assert!(error.contains("run") && error.contains("stop"), "{error}");
    }
}

fn app_with_keys(items: &[(&str, &[&str])]) -> App {
    let map = Keymap::with_overrides(KeyboardMode::Conventional, &overrides(items)).unwrap();
    App::new(
        Options {
            keymap: Some(map),
            ..Options::default()
        },
        Rc::new(RefCell::new(MemoryClipboard::default())),
        Theme::plain(),
    )
}

#[test]
fn the_application_waits_for_every_key_of_a_long_sequence() {
    let mut app = app_with_keys(&[("closeTab", &["f8 a b c"])]);
    for code in [KeyCode::F(8), KeyCode::Char('a'), KeyCode::Char('b')] {
        app.on_event(Event::Key(key(code)));
        assert!(!app.should_quit());
        assert!(app.hint().contains("next key"));
        assert_eq!(app.console().editor.text(), "");
    }
    app.on_event(Event::Key(key(KeyCode::Char('c'))));
    assert!(app.should_quit());
}

#[test]
fn escape_and_an_unmatched_key_cancel_a_sequence_without_entering_text() {
    let mut app = app_with_keys(&[("closeTab", &["f8 a b c"])]);
    for end in [KeyCode::Esc, KeyCode::Char('x')] {
        for code in [KeyCode::F(8), KeyCode::Char('a'), end] {
            app.on_event(Event::Key(key(code)));
        }
        assert!(!app.should_quit());
        assert_eq!(app.console().editor.text(), "");
    }
    app.on_event(Event::Key(key(KeyCode::Char('z'))));
    assert_eq!(app.console().editor.text(), "z");
}

#[test]
fn a_long_global_sequence_still_works_after_a_shared_panel_prefix() {
    let mut app = app_with_keys(&[("newConsole", &["f8 n n"]), ("closeTab", &["f8 c c"])]);
    app.on_event(Event::Key(ctrl('w')));
    app.on_event(Event::Key(key(KeyCode::Char('h'))));
    for code in [KeyCode::F(8), KeyCode::Char('c'), KeyCode::Char('c')] {
        app.on_event(Event::Key(key(code)));
    }
    assert!(app.should_quit());
}

#[test]
fn the_grid_opens_the_export_in_both_modes() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        assert_eq!(
            Keymap::new(mode).lookup(Region::Grid, chord("e")),
            Lookup::Action(Action::GridExport)
        );
    }
}

#[test]
fn the_keys_of_the_table_view_can_change_by_name() {
    let items = overrides(&[("loadMore", &["ctrl+l"]), ("sortColumn", &[])]);
    let map = Keymap::with_overrides(KeyboardMode::Conventional, &items).unwrap();
    assert_eq!(
        map.lookup(Region::Grid, chord("ctrl+l")),
        Lookup::Action(Action::GridLoadMore)
    );
    assert_eq!(map.lookup(Region::Grid, chord("m")), Lookup::None);
    assert_eq!(map.lookup(Region::Grid, chord("s")), Lookup::None);
    // The keys of the table view and the keys of the grid share one region.
    let items = overrides(&[("loadMore", &["x"])]);
    assert!(Keymap::with_overrides(KeyboardMode::Conventional, &items).is_err());
}

#[test]
fn the_execution_keys_stay_the_same_in_both_modes() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let map = Keymap::new(mode);
        assert_eq!(
            map.lookup(Region::Global, chord("f5")),
            Lookup::Action(Action::Run)
        );
        assert_eq!(
            map.lookup(Region::Global, chord("ctrl+enter")),
            Lookup::Action(Action::Run)
        );
        assert_eq!(
            map.lookup(Region::Global, chord("ctrl+w")),
            Lookup::Sequence
        );
        assert_eq!(
            map.after(Region::Global, chord("ctrl+w"), chord("e")),
            Some(Action::Run)
        );
        assert_eq!(
            map.after(Region::Global, chord("ctrl+w"), chord("left")),
            Some(Action::FocusLeft)
        );
    }
}

#[test]
fn a_list_replaces_the_keys_of_its_action() {
    let items = overrides(&[("run", &["f6"])]);
    let map = Keymap::with_overrides(KeyboardMode::Conventional, &items).unwrap();
    assert_eq!(
        map.lookup(Region::Global, chord("f6")),
        Lookup::Action(Action::Run)
    );
    assert_eq!(map.lookup(Region::Global, chord("f5")), Lookup::None);
    assert_eq!(map.keys(Action::Run), ["f6"]);
}

#[test]
fn an_empty_list_removes_the_keys_of_its_action() {
    let items = overrides(&[("stop", &[])]);
    let map = Keymap::with_overrides(KeyboardMode::Conventional, &items).unwrap();
    assert_eq!(map.after(Region::Global, chord("ctrl+w"), chord("x")), None);
    assert!(map.keys(Action::Stop).is_empty());
}

#[test]
fn two_actions_with_one_key_in_a_region_are_refused() {
    let items = overrides(&[("stop", &["f5"])]);
    let error = Keymap::with_overrides(KeyboardMode::Conventional, &items).unwrap_err();
    assert!(error.contains("f5"), "{error}");
}

#[test]
fn two_regions_can_use_one_key() {
    let items = overrides(&[("newConsole", &["f5"])]);
    let map = Keymap::with_overrides(KeyboardMode::Conventional, &items).unwrap();
    assert_eq!(
        map.lookup(Region::Navigator, chord("f5")),
        Lookup::Action(Action::NavigatorNewConsole)
    );
    assert_eq!(
        map.lookup(Region::Global, chord("f5")),
        Lookup::Action(Action::Run)
    );
}

#[test]
fn a_key_and_a_sequence_that_starts_with_it_are_refused() {
    let items = overrides(&[("gridFirstRow", &["g g"]), ("gridLastRow", &["g"])]);
    assert!(Keymap::with_overrides(KeyboardMode::Conventional, &items).is_err());
    let items = overrides(&[("run", &["ctrl+w"])]);
    assert!(Keymap::with_overrides(KeyboardMode::Conventional, &items).is_err());
}

#[test]
fn an_unknown_action_or_key_is_refused() {
    let items = overrides(&[("launch", &["f5"])]);
    assert!(Keymap::with_overrides(KeyboardMode::Conventional, &items).is_err());
    let items = overrides(&[("run", &["hyper+x"])]);
    assert!(Keymap::with_overrides(KeyboardMode::Conventional, &items).is_err());
}

#[test]
fn the_application_uses_the_map() {
    let items = overrides(&[("closeTab", &["f9"])]);
    let map = Keymap::with_overrides(KeyboardMode::Conventional, &items).unwrap();
    let options = Options {
        keymap: Some(map),
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let mut app = App::new(options, clipboard, Theme::plain());
    app.on_event(Event::Key(ctrl('w')));
    app.on_event(Event::Key(key(KeyCode::Char('c'))));
    assert!(!app.should_quit());
    app.on_event(Event::Key(key(KeyCode::F(9))));
    assert!(app.should_quit());
}

#[test]
fn a_panel_key_has_priority_over_a_global_key() {
    let path = common::temp_db("keymap-panel");
    rusqlite::Connection::open(&path).unwrap();
    let items = overrides(&[("newConsole", &["f5"])]);
    let map = Keymap::with_overrides(KeyboardMode::Conventional, &items).unwrap();
    let options = Options {
        keymap: Some(map),
        database: Some(path),
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let mut app = App::new(options, clipboard, Theme::plain());
    app.on_event(Event::Key(ctrl('w')));
    app.on_event(Event::Key(key(KeyCode::Char('h'))));
    app.on_event(Event::Key(key(KeyCode::F(5))));
    assert_eq!(app.consoles().len(), 2, "the navigator uses the key");
    app.on_event(Event::Key(key(KeyCode::F(5))));
    assert_eq!(
        app.consoles().len(),
        2,
        "the editor leaves the key to the global action"
    );
}

#[test]
fn a_character_key_of_a_global_action_stays_text_in_the_editor() {
    let items = overrides(&[("closeTab", &["q"])]);
    let map = Keymap::with_overrides(KeyboardMode::Conventional, &items).unwrap();
    let options = Options {
        keymap: Some(map),
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let mut app = App::new(options, clipboard, Theme::plain());
    app.on_event(Event::Key(key(KeyCode::Char('q'))));
    assert!(!app.should_quit());
    assert_eq!(app.console().editor.text(), "q");
}

#[test]
fn three_keys_reach_the_comment_action_in_each_mode() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let map = Keymap::new(mode);
        for text in ["ctrl+/", "ctrl+7"] {
            assert_eq!(
                map.lookup(Region::Global, chord(text)),
                Lookup::Action(Action::ToggleComment)
            );
        }
        assert_eq!(
            map.after(Region::Global, chord("ctrl+w"), chord("/")),
            Some(Action::ToggleComment)
        );
    }
}

#[test]
fn the_row_and_count_keys_work_on_the_grid_of_each_mode() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let map = Keymap::new(mode);
        assert_eq!(
            map.lookup(Region::Grid, chord("r")),
            Lookup::Action(Action::GridOpenRow)
        );
        assert_eq!(
            map.lookup(Region::Grid, chord("c")),
            Lookup::Action(Action::GridCountRows)
        );
    }
    assert!(Action::GridCountRows.is_table_only());
}
