// The connection form of Databricks. The connector tests are in `sqln-databricks`.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::connection::Source;
use sqln::connector::target::{Engine, Secret};
use sqln::form::{ConnectionForm, Field, Kind};
use sqln::ui::theme::Theme;

fn databricks_form() -> ConnectionForm {
    let mut form = ConnectionForm::new();
    form.kind = Kind::Databricks;
    for (field, text) in [
        (Field::Name, "lake"),
        (Field::Host, "adb-1.azuredatabricks.net"),
        (Field::Database, "main"),
        (Field::Warehouse, "abc123"),
        (Field::Password, "dapi-token"),
    ] {
        form.focus_on(field);
        form.on_paste(text);
    }
    form
}

fn draw(form: &ConnectionForm) -> String {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| {
            sqln::ui::form::draw_form(frame, form, &Theme::plain());
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    buffer
        .content
        .chunks(usize::from(buffer.area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>() + "\n")
        .collect()
}

#[test]
fn the_form_of_databricks_has_a_warehouse_and_a_token_and_no_user() {
    let form = databricks_form();
    let fields = form.fields();
    assert!(fields.contains(&Field::Warehouse));
    assert!(fields.contains(&Field::Database));
    assert!(fields.contains(&Field::Schemas));
    assert!(!fields.contains(&Field::User));
    assert!(!fields.contains(&Field::Sasl));

    let mut form = form;
    form.select_section(Field::Warehouse.group());
    let shown = draw(&form);
    for label in ["Catalog", "Warehouse", "abc123"] {
        assert!(shown.contains(label), "{label}:\n{shown}");
    }
    form.select_section(Field::Password.group());
    let shown = draw(&form);
    assert!(shown.contains("Token"), "{shown}");
    assert!(!shown.contains("dapi-token"), "{shown}");
}

#[test]
fn the_form_saves_a_databricks_connection_and_reads_it_back() {
    let form = databricks_form();
    let definition = form.definition().unwrap();
    let Source::Network(target) = &definition.source else {
        panic!("not a network source");
    };
    assert_eq!(target.engine, Engine::Databricks);
    assert_eq!(target.databricks.warehouse, "abc123");
    assert_eq!(target.database.as_deref(), Some("main"));
    assert_eq!(target.password, Some(Secret::new("dapi-token")));
    assert_eq!(target.user, "");
    let edited = ConnectionForm::editing(1, &definition, false);
    assert_eq!(edited.definition().unwrap(), definition);
}

#[test]
fn the_form_refuses_a_databricks_connection_without_a_valid_warehouse() {
    for text in ["", "a/b"] {
        let mut form = databricks_form();
        form.focus_on(Field::Warehouse);
        while !form.text(Field::Warehouse).is_empty() {
            form.on_key(
                crate::common::key(crossterm::event::KeyCode::Backspace),
                false,
            );
        }
        form.on_paste(text);
        let error = form.definition().unwrap_err();
        assert_eq!(error.field, Field::Warehouse, "{text:?}");
    }
}

#[test]
fn the_connection_text_of_databricks_fills_the_form() {
    let mut form = ConnectionForm::new();
    form.kind = Kind::Databricks;
    form.use_uri = true;
    form.focus_on(Field::Name);
    form.on_paste("lake");
    form.focus_on(Field::Uri);
    form.on_paste("databricks://:dapi-token@adb-1.net/main?warehouse=abc123");
    let definition = form.definition().unwrap();
    let Source::Network(target) = &definition.source else {
        panic!("not a network source");
    };
    assert_eq!(target.databricks.warehouse, "abc123");
    let shown = draw(&form);
    assert!(!shown.contains("dapi-token"), "{shown}");
}
