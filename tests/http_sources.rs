// The connection forms of ClickHouse and libSQL. The connector tests are in
// `sqln-clickhouse` and `sqln-libsql`.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::connection::Source;
use sqln::connector::target::{Engine, Secret};
use sqln::form::{ConnectionForm, Field, Kind};
use sqln::ui::theme::Theme;

fn form(kind: Kind, fields: &[(Field, &str)]) -> ConnectionForm {
    let mut form = ConnectionForm::new();
    form.kind = kind;
    for (field, text) in fields {
        form.focus_on(*field);
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
fn the_form_of_libsql_has_a_token_and_no_user_database_or_schemas() {
    let mut form = form(
        Kind::Libsql,
        &[
            (Field::Name, "edge"),
            (Field::Host, "db-org.turso.io"),
            (Field::Password, "secret-token"),
        ],
    );
    let fields = form.fields();
    for hidden in [
        Field::User,
        Field::Database,
        Field::Schemas,
        Field::Warehouse,
    ] {
        assert!(!fields.contains(&hidden), "{hidden:?}");
    }
    form.select_section(Field::Password.group());
    let shown = draw(&form);
    assert!(shown.contains("Token"), "{shown}");
    assert!(!shown.contains("secret-token"), "{shown}");

    let definition = form.definition().unwrap();
    let Source::Network(target) = &definition.source else {
        panic!("not a network source");
    };
    assert_eq!(target.engine, Engine::Libsql);
    assert_eq!(target.user, "");
    assert_eq!(target.database, None);
    assert_eq!(target.password, Some(Secret::new("secret-token")));
    let edited = ConnectionForm::editing(1, &definition, false);
    assert_eq!(edited.definition().unwrap(), definition);
}

#[test]
fn the_form_of_clickhouse_has_a_user_a_database_and_databases_to_show() {
    let form = form(
        Kind::ClickHouse,
        &[
            (Field::Name, "events"),
            (Field::Host, "ch.example.com"),
            (Field::Database, "sales"),
            (Field::User, "sqln"),
        ],
    );
    let fields = form.fields();
    for shown in [
        Field::User,
        Field::Database,
        Field::Password,
        Field::Schemas,
    ] {
        assert!(fields.contains(&shown), "{shown:?}");
    }
    let definition = form.definition().unwrap();
    let Source::Network(target) = &definition.source else {
        panic!("not a network source");
    };
    assert_eq!(target.engine, Engine::ClickHouse);
    assert_eq!(target.database.as_deref(), Some("sales"));
    assert_eq!(
        target.address,
        sqln::connector::target::Address::Tcp {
            host: "ch.example.com".into(),
            port: 8123
        }
    );
}

#[test]
fn an_http_source_refuses_a_socket() {
    for kind in [Kind::ClickHouse, Kind::Libsql] {
        let form = form(
            kind,
            &[
                (Field::Name, "x"),
                (Field::Host, "/tmp/socket"),
                (Field::User, "u"),
            ],
        );
        let error = form.definition().unwrap_err();
        assert_eq!(error.field, Field::Host, "{kind:?}");
    }
}
