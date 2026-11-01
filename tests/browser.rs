// The file browser of the path fields: its rows, its keys, and its window.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{ctrl, key, temp_dir};
use crossterm::event::KeyCode;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::browser::{BrowserRequest, FileBrowser, RowView};
use sqln::ui::browser::draw_browser;
use sqln::ui::console::modal_width;
use sqln::ui::theme::Theme;

/// Creates a folder with these names. A name that ends with `/` is a folder.
fn tree(name: &str, names: &[&str]) -> PathBuf {
    let root = temp_dir(&format!("browser-{name}"));
    for name in names {
        match name.strip_suffix('/') {
            Some(folder) => fs::create_dir_all(root.join(folder)).unwrap(),
            None => fs::write(root.join(name), "").unwrap(),
        }
    }
    root.canonicalize().unwrap()
}

fn text_of(path: &Path) -> String {
    path.display().to_string()
}

fn browse(folder: &Path) -> FileBrowser {
    FileBrowser::new(&format!("{}/", text_of(folder)), false, None)
}

fn labels(browser: &FileBrowser) -> Vec<String> {
    browser
        .rows()
        .map(|row| match row {
            RowView::UseFolder => "(use)".to_owned(),
            RowView::Parent => "../".to_owned(),
            RowView::Folder(name) => format!("{name}/"),
            RowView::File(name) => name.to_owned(),
        })
        .collect()
}

fn press(browser: &mut FileBrowser, code: KeyCode) -> BrowserRequest {
    browser.on_key(key(code))
}

fn type_text(browser: &mut FileBrowser, text: &str) {
    for c in text.chars() {
        press(browser, KeyCode::Char(c));
    }
}

#[test]
fn the_folders_come_first_and_the_order_ignores_case() {
    let root = tree("order", &["c.txt", "B.txt", "b/", "A/"]);
    let browser = browse(&root);
    assert_eq!(labels(&browser), ["../", "A/", "b/", "B.txt", "c.txt"]);
}

#[test]
fn a_filter_with_a_dot_shows_the_hidden_names() {
    let root = tree("hidden", &[".env", ".git/", "app.db"]);
    let mut browser = browse(&root);
    assert_eq!(labels(&browser), ["../", "app.db"]);
    type_text(&mut browser, ".");
    assert_eq!(labels(&browser), [".git/", ".env", "app.db"]);
    type_text(&mut browser, "e");
    assert_eq!(labels(&browser), [".env"]);
}

#[test]
fn the_filter_ignores_case_and_hides_the_fixed_rows() {
    let root = tree("filter", &["Orders.db", "users.db", "notes.txt"]);
    let mut browser = FileBrowser::new(&format!("{}/", text_of(&root)), true, None);
    assert_eq!(
        labels(&browser),
        ["(use)", "../", "notes.txt", "Orders.db", "users.db"]
    );
    type_text(&mut browser, "ORD");
    assert_eq!(labels(&browser), ["Orders.db"]);
    assert_eq!(
        press(&mut browser, KeyCode::Enter),
        BrowserRequest::File(text_of(&root.join("Orders.db")))
    );
}

#[test]
fn a_filter_without_a_match_has_no_rows_and_enter_does_nothing() {
    let root = tree("no-match", &["a.db"]);
    let mut browser = browse(&root);
    type_text(&mut browser, "zzz");
    assert_eq!(browser.row_count(), 0);
    assert_eq!(press(&mut browser, KeyCode::Enter), BrowserRequest::None);
}

#[test]
fn enter_opens_a_folder_and_left_returns_to_that_row() {
    let root = tree("enter", &["a/", "b/", "b/inner.db"]);
    let mut browser = browse(&root);
    browser.point_at(2);
    assert_eq!(press(&mut browser, KeyCode::Enter), BrowserRequest::None);
    assert_eq!(browser.folder(), root.join("b"));
    assert_eq!(labels(&browser), ["../", "inner.db"]);

    press(&mut browser, KeyCode::Left);
    assert_eq!(browser.folder(), root);
    assert_eq!(browser.cursor(), 2);

    press(&mut browser, KeyCode::Right);
    assert_eq!(browser.folder(), root.join("b"));
    press(&mut browser, KeyCode::Backspace);
    assert_eq!(browser.folder(), root);
}

#[test]
fn the_keys_of_a_folder_edit_a_filter_with_text() {
    let root = tree("edit-filter", &["a/", "ab/"]);
    let mut browser = browse(&root);
    type_text(&mut browser, "ab");
    press(&mut browser, KeyCode::Left);
    press(&mut browser, KeyCode::Right);
    press(&mut browser, KeyCode::Backspace);
    assert_eq!(browser.folder(), root);
    assert_eq!(browser.filter().text(), "a");
    assert_eq!(labels(&browser), ["a/", "ab/"]);
}

#[test]
fn entering_a_folder_clears_the_filter() {
    let root = tree("clear", &["docs/", "docs/a.txt", "other/"]);
    let mut browser = browse(&root);
    type_text(&mut browser, "doc");
    press(&mut browser, KeyCode::Enter);
    assert_eq!(browser.folder(), root.join("docs"));
    assert_eq!(browser.filter().text(), "");
    assert_eq!(labels(&browser), ["../", "a.txt"]);
}

#[test]
fn the_root_has_no_row_for_the_parent() {
    let mut browser = FileBrowser::new("/", false, None);
    assert_ne!(browser.rows().next(), Some(RowView::Parent));
    press(&mut browser, KeyCode::Left);
    assert_eq!(browser.folder(), Path::new("/"));
}

#[test]
fn the_right_key_does_nothing_on_a_file() {
    let root = tree("right-file", &["a.db"]);
    let mut browser = browse(&root);
    browser.point_at(1);
    assert_eq!(press(&mut browser, KeyCode::Right), BrowserRequest::None);
    assert_eq!(browser.folder(), root);
}

#[test]
fn a_file_ends_the_choice_and_escape_cancels() {
    let root = tree("file", &["a.db"]);
    let mut browser = browse(&root);
    browser.point_at(1);
    assert_eq!(
        press(&mut browser, KeyCode::Enter),
        BrowserRequest::File(text_of(&root.join("a.db")))
    );
    assert_eq!(press(&mut browser, KeyCode::Esc), BrowserRequest::Cancel);
}

#[test]
fn the_cursor_stays_on_the_rows() {
    let root = tree("cursor", &["a", "b"]);
    let mut browser = browse(&root);
    browser.move_by(-5);
    assert_eq!(browser.cursor(), 0);
    press(&mut browser, KeyCode::PageDown);
    assert_eq!(browser.cursor(), 2);
    browser.point_at(9);
    assert_eq!(browser.cursor(), 2);
    press(&mut browser, KeyCode::PageUp);
    assert_eq!(browser.cursor(), 0);
}

#[test]
fn a_paste_goes_to_the_filter() {
    let root = tree("paste", &["orders.db", "users.db"]);
    let mut browser = browse(&root);
    browser.paste("users\nignored");
    assert_eq!(browser.filter().text(), "users");
    assert_eq!(labels(&browser), ["users.db"]);
}

#[test]
fn the_window_starts_at_the_folder_of_the_field_with_the_cursor_on_its_file() {
    let root = tree("start", &["a.db", "b.db"]);
    let browser = FileBrowser::new(&text_of(&root.join("b.db")), false, None);
    assert_eq!(browser.folder(), root);
    assert_eq!(
        browser.rows().nth(browser.cursor()),
        Some(RowView::File("b.db"))
    );
}

#[test]
fn the_window_starts_at_the_nearest_folder_that_exists() {
    let root = tree("nearest", &["a/"]);
    let path = root.join("a/missing/deep/file.db");
    assert_eq!(
        FileBrowser::new(&text_of(&path), false, None).folder(),
        root.join("a")
    );
}

#[test]
fn an_empty_field_starts_at_the_current_folder() {
    let current = std::env::current_dir().unwrap().canonicalize().unwrap();
    assert_eq!(FileBrowser::new("  ", false, None).folder(), current);
}

#[test]
fn the_window_starts_at_a_folder_with_a_parent_step_in_its_path() {
    let root = tree("dots", &["a/", "b/"]);
    let path = format!("{}/a/../b/file.db", text_of(&root));
    let mut browser = FileBrowser::new(&path, false, None);
    assert_eq!(browser.folder(), root.join("b"));
    press(&mut browser, KeyCode::Left);
    assert_eq!(browser.folder(), root);
}

#[test]
fn a_tilde_in_the_field_starts_at_the_home() {
    let root = tree("tilde-field", &["data/"]);
    let browser = FileBrowser::new("~/data/file.db", false, Some(root.clone()));
    assert_eq!(browser.folder(), root.join("data"));
}

#[test]
fn the_tilde_key_goes_home_with_an_empty_filter() {
    let root = tree("tilde-key", &["home/", "work/", "home/~name"]);
    let home = root.join("home");
    let mut browser = FileBrowser::new(
        &format!("{}/work/", text_of(&root)),
        false,
        Some(home.clone()),
    );
    press(&mut browser, KeyCode::Char('~'));
    assert_eq!(browser.folder(), home);

    // In a filter, the tilde is a character of a name.
    type_text(&mut browser, "x");
    press(&mut browser, KeyCode::Backspace);
    type_text(&mut browser, "n");
    press(&mut browser, KeyCode::Home);
    press(&mut browser, KeyCode::Char('~'));
    assert_eq!(browser.filter().text(), "~n");
    assert_eq!(labels(&browser), ["~name"]);

    browser.on_key(ctrl('~'));
    assert_eq!(browser.filter().text(), "~n");
}

#[test]
fn the_tilde_key_does_nothing_without_a_home() {
    let root = tree("no-home", &[]);
    let mut browser = browse(&root);
    press(&mut browser, KeyCode::Char('~'));
    assert_eq!(browser.folder(), root);
    assert_eq!(browser.filter().text(), "");
}

#[test]
fn use_this_folder_keeps_the_name_of_the_field() {
    let root = tree("use", &["old/", "new/"]);
    let path = text_of(&root.join("old/out.csv"));
    let mut browser = FileBrowser::new(&path, true, None);
    assert_eq!(labels(&browser), ["(use)", "../"]);
    press(&mut browser, KeyCode::Left);
    browser.point_at(2);
    press(&mut browser, KeyCode::Enter);
    assert_eq!(browser.folder(), root.join("new"));
    assert_eq!(browser.cursor(), 0);
    assert_eq!(
        press(&mut browser, KeyCode::Enter),
        BrowserRequest::Folder(text_of(&root.join("new/out.csv")))
    );
}

#[test]
fn use_this_folder_leaves_a_field_without_a_name_ready_for_one() {
    let root = tree("use-empty", &["a/"]);
    for field in [format!("{}/a/", text_of(&root)), text_of(&root.join("a"))] {
        let mut browser = FileBrowser::new(&field, true, None);
        assert_eq!(
            press(&mut browser, KeyCode::Enter),
            BrowserRequest::Folder(format!("{}/a/", text_of(&root))),
            "{field}"
        );
    }
}

#[test]
fn a_field_of_an_existing_file_has_no_use_this_folder() {
    let root = tree("no-use", &["ca.pem"]);
    let browser = browse(&root);
    assert!(!browser.rows().any(|row| row == RowView::UseFolder));
}

#[cfg(unix)]
#[test]
fn a_link_to_a_folder_is_a_folder_and_the_parent_is_the_folder_of_the_link() {
    let root = tree("link", &["place/", "real/", "real/inner.db", "file.db"]);
    std::os::unix::fs::symlink(root.join("real"), root.join("place/to-folder")).unwrap();
    std::os::unix::fs::symlink(root.join("file.db"), root.join("place/to-file")).unwrap();
    std::os::unix::fs::symlink(root.join("gone"), root.join("place/broken")).unwrap();
    let mut browser = browse(&root.join("place"));
    assert_eq!(labels(&browser), ["../", "to-folder/", "broken", "to-file"]);

    browser.point_at(1);
    press(&mut browser, KeyCode::Enter);
    assert_eq!(labels(&browser), ["../", "inner.db"]);
    press(&mut browser, KeyCode::Left);
    assert_eq!(browser.folder(), root.join("place"));
}

#[cfg(unix)]
#[test]
fn a_refused_folder_leaves_a_note_and_the_list() {
    use std::os::unix::fs::PermissionsExt;

    let root = tree("refused", &["closed/"]);
    let closed = root.join("closed");
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();
    let mut browser = browse(&root);
    browser.point_at(1);
    press(&mut browser, KeyCode::Enter);
    let refused = fs::read_dir(&closed).is_err();
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o755)).unwrap();
    if !refused {
        // A superuser reads any folder.
        return;
    }
    assert_eq!(browser.folder(), root);
    assert!(
        browser
            .note()
            .unwrap()
            .starts_with("Cannot read the folder")
    );
    assert_eq!(labels(&browser), ["../", "closed/"]);

    press(&mut browser, KeyCode::Down);
    assert_eq!(browser.note(), None);
}

#[cfg(unix)]
#[test]
fn a_path_that_a_field_would_change_leaves_a_note() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let root = tree("bad-path", &["line\nbreak", "space "]);
    let mut browser = browse(&root);
    for index in [1, 2] {
        browser.point_at(index);
        assert_eq!(press(&mut browser, KeyCode::Enter), BrowserRequest::None);
        assert!(browser.note().unwrap().contains("cannot hold"));
    }

    // macOS refuses a name that is not UTF-8, so this part runs only where it exists.
    if fs::write(root.join(OsStr::from_bytes(b"\xff.db")), "").is_ok() {
        let mut browser = browse(&root);
        let index = browser
            .rows()
            .position(|row| matches!(row, RowView::File(name) if name.ends_with(".db")))
            .unwrap();
        browser.point_at(index);
        assert_eq!(press(&mut browser, KeyCode::Enter), BrowserRequest::None);
        assert!(browser.note().is_some());
    }
}

fn drawn(browser: &FileBrowser, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| {
            draw_browser(frame, browser, &Theme::plain());
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    buffer
        .content()
        .chunks(usize::from(width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_window_shows_the_folder_and_the_rows() {
    let root = tree("window", &["docs/", "app.db"]);
    let browser = FileBrowser::new(&format!("{}/", text_of(&root)), true, None);
    let shown = drawn(&browser, 100, 30);
    assert!(shown.contains("Choose a file"), "{shown}");
    assert!(shown.contains("Use this folder"), "{shown}");
    assert!(shown.contains(" ../"), "{shown}");
    assert!(shown.contains(" docs/"), "{shown}");
    assert!(shown.contains(" app.db"), "{shown}");
    assert!(shown.contains("window"), "{shown}");
}

#[test]
fn the_end_of_a_long_folder_stays_visible() {
    let long = "a".repeat(100);
    let root = tree("long", &[&format!("{long}/last-folder/")]);
    let browser = browse(&root.join(&long).join("last-folder"));
    let shown = drawn(&browser, 80, 24);
    assert!(shown.contains("last-folder"), "{shown}");
}

#[test]
fn the_window_says_when_no_row_matches() {
    let root = tree("window-empty", &["a.db"]);
    let mut browser = browse(&root);
    type_text(&mut browser, "zzz");
    assert!(drawn(&browser, 80, 24).contains("No matching items"));
}

#[test]
fn the_cursor_stays_inside_the_window() {
    let names: Vec<String> = (0..40).map(|index| format!("file-{index:02}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let root = tree("scroll", &names);
    let mut browser = browse(&root);
    browser.point_at(40);
    let shown = drawn(&browser, 80, 24);
    assert!(shown.contains("file-39"), "{shown}");
    assert!(!shown.contains("file-00"), "{shown}");
}

#[test]
fn a_modal_grows_with_the_screen_up_to_a_limit() {
    assert_eq!(modal_width(80, 72), 72);
    assert_eq!(modal_width(60, 72), 72);
    assert_eq!(modal_width(100, 72), 92);
    assert_eq!(modal_width(148, 72), 140);
    assert_eq!(modal_width(300, 72), 140);
    assert_eq!(modal_width(120, 116), 116);
    assert_eq!(modal_width(300, 150), 150);
}

#[test]
fn a_larger_screen_gives_the_window_more_columns_and_rows() {
    let names: Vec<String> = (0..60).map(|index| format!("file-{index:02}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let root = tree("grow", &names);
    let browser = browse(&root);

    let small = drawn(&browser, 80, 24);
    assert!(small.contains("file-13"), "{small}");
    assert!(!small.contains("file-15"), "{small}");

    let tall = drawn(&browser, 120, 50);
    assert!(tall.contains("file-33"), "{tall}");
    assert!(!tall.contains("file-35"), "{tall}");

    let huge = drawn(&browser, 300, 100);
    assert!(huge.contains("file-33"), "the list has a limit:\n{huge}");
    assert!(!huge.contains("file-35"), "the list has a limit:\n{huge}");
}

#[test]
fn a_larger_screen_shows_more_of_a_long_folder() {
    let long = "abcdefghij".repeat(10);
    let root = tree("grow-path", &[&format!("{long}/")]);
    let browser = browse(&root.join(&long));
    assert!(!drawn(&browser, 80, 24).contains(&long[..70]));
    assert!(drawn(&browser, 160, 40).contains(&long));
}

#[cfg(unix)]
#[test]
fn a_name_cannot_send_terminal_sequences() {
    let root = tree("escape", &["a\u{1b}[31mb"]);
    let browser = browse(&root);
    let shown = drawn(&browser, 80, 24);
    assert!(!shown.contains('\u{1b}'), "{shown}");
}

#[test]
fn a_small_area_does_not_break_the_window() {
    let root = tree("small", &["a.db"]);
    let browser = browse(&root);
    for (width, height) in [(1, 1), (10, 3), (20, 4), (72, 5), (80, 24)] {
        drawn(&browser, width, height);
    }
}
