//! Helpers that drive the whole application. The helpers of the lower crates come
//! from `sqln-testkit`.

#![allow(dead_code, unused_imports)]

pub use sqln_testkit::*;

use crossterm::event::{Event, KeyCode};
use sqln::app::App;
use sqln::ui::text;
use sqln::ui::which_key::KeyGroup;

/// Returns true when the palette window of a drawn screen shows a command row.
/// A help page row and a field row are not commands.
pub fn palette_offers_command(screen: &str) -> bool {
    let top = format!("╭ {} ", text::TITLE_PALETTE);
    let help = text::help_command("");
    screen
        .lines()
        .skip_while(|line| !line.contains(&top))
        .skip(1)
        .take_while(|line| !line.contains('╰'))
        .filter_map(|line| line.split('│').nth(1))
        .map(str::trim)
        .any(|row| {
            !row.is_empty()
                && !row.starts_with('>')
                && !row.starts_with(text::NO_COMMANDS)
                && !row.starts_with(&help)
                && !row.starts_with(text::CONNECTION_FIELD_ROW)
                && !row.starts_with(text::SETTING_ROW)
        })
}

/// Lists the rows of a page of the `Ctrl+W` menu, as `key label`.
fn menu_rows(groups: Vec<KeyGroup>) -> Vec<String> {
    let groups = groups.into_iter();
    groups
        .flat_map(|group| group.keys)
        .map(|(key, label)| format!("{key} {label}"))
        .collect()
}

/// Lists the rows of the explorer page of the `Ctrl+W` menu, as `key label`.
pub fn explorer_page(app: &App) -> Vec<String> {
    menu_rows(app.explorer_menu())
}

/// Lists the rows of the first page of the `Ctrl+W` menu, as `key label`.
pub fn prefix_page(app: &App) -> Vec<String> {
    menu_rows(app.prefix_menu())
}

/// Opens a table with the keys of the navigator. `Enter` on its row does it.
/// The call waits for the catalog, and it does not wait for the first page.
pub fn open_table(app: &mut App, connection: usize, name: &str) {
    use sqln::navigator::RowKind;

    select_row(app, connection, &[RowKind::Table, RowKind::View], name);
    app.on_event(Event::Key(key(KeyCode::Enter)));
}

/// Moves the cursor of the navigator to a row, with the keys of the navigator. It opens
/// the rows above the target and waits for the catalog.
pub fn select_row(
    app: &mut App,
    connection: usize,
    kinds: &[sqln::navigator::RowKind],
    name: &str,
) {
    use sqln::navigator::RowKind;

    use sqln::connection::CatalogState;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    // A catalog in a new read has no new table yet.
    while app.connections()[connection].catalog_state == CatalogState::Loading {
        app.drain_events();
        assert!(
            std::time::Instant::now() < deadline,
            "the catalog did not arrive"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let press = |app: &mut App, code| app.on_event(Event::Key(key(code)));
    app.on_event(Event::Key(ctrl('w')));
    press(app, KeyCode::Char('h'));
    assert!(app.navigator_focused());
    // The search goes down from the first row.
    for _ in 0..50 {
        press(app, KeyCode::Up);
    }
    for _ in 0..500 {
        let row = app.navigator().current(app.connections()).unwrap();
        let is_target = row.connection == connection
            && kinds.contains(&row.kind)
            && row.name(app.connections()) == name;
        if is_target {
            return;
        }
        let branch = matches!(row.kind, RowKind::Connection | RowKind::Schema);
        let closed = !app.navigator().is_open(&row, app.connections());
        if row.connection == connection && branch && closed {
            press(app, KeyCode::Right);
            // A connection waits for its catalog, and a schema opens at once.
            while row.kind == RowKind::Connection && app.connections()[connection].catalog.is_none()
            {
                app.drain_events();
                assert!(
                    std::time::Instant::now() < deadline,
                    "the catalog did not arrive"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        } else {
            press(app, KeyCode::Down);
        }
    }
    panic!("{name} is not in the navigator");
}

/// Moves the focus of the connection form to a field with the keys of a user. A field of
/// another section first opens that section from the sidebar.
pub fn focus_form_field(app: &mut App, field: sqln::form::Field) {
    use sqln::form::{Field, FieldStyle};
    let press = |app: &mut App, code| {
        app.on_event(Event::Key(key(code)));
    };
    let form = |app: &App| {
        let form = app.form().expect("the form is open");
        (form.focus, form.shown_section())
    };
    if field.style() != FieldStyle::Button && form(app).1 != field.group() {
        for _ in 0..40 {
            if form(app).0 == Field::Sections {
                break;
            }
            press(app, KeyCode::BackTab);
        }
        for _ in 0..8 {
            if form(app).1 == field.group() {
                break;
            }
            press(app, KeyCode::Down);
        }
    }
    for _ in 0..40 {
        if form(app).0 == field {
            return;
        }
        press(app, KeyCode::Tab);
    }
    panic!("{field:?} is not in the form");
}
