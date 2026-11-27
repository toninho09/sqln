//! The rows of the palette that open a field of a form.

use sqln_store::store::config::Preferences;
use sqln_ui::form::Field;
use sqln_ui::palette::{Command, Palette, PaletteRequest, Target};
use sqln_ui::settings::{self, Field as SettingsField, SECTIONS, SettingsForm};

fn row(target: Target, label: &str, words: &str) -> Command {
    Command {
        target,
        label: label.to_owned(),
        key: None,
        words: words.to_owned(),
    }
}

#[test]
fn a_field_row_asks_for_its_form_and_the_filter_reads_its_section() {
    let mut palette = Palette::new(vec![
        row(
            Target::ConnectionField(7, Field::Pool),
            "Connection: Pool limit = 5 — one",
            "Options",
        ),
        row(
            Target::Setting(SettingsField::UndoLimit),
            "Setting: Undo (MiB) = 64 — Editor",
            "",
        ),
    ]);
    assert_eq!(
        palette.choose(0),
        PaletteRequest::ConnectionField(7, Field::Pool)
    );
    assert_eq!(
        palette.choose(1),
        PaletteRequest::Setting(SettingsField::UndoLimit)
    );
    palette.paste("options");
    assert_eq!(palette.shown().len(), 1);
    assert_eq!(
        palette.choose(0),
        PaletteRequest::ConnectionField(7, Field::Pool)
    );
}

#[test]
fn the_value_fields_skip_the_buttons_and_open_on_their_section() {
    let fields: Vec<SettingsField> = settings::value_fields().map(|(_, field)| field).collect();
    for button in [
        SettingsField::AiTest,
        SettingsField::ThemeNew,
        SettingsField::ShortcutRecord,
        SettingsField::Themes,
    ] {
        assert!(!fields.contains(&button), "{button:?}");
    }
    assert!(fields.contains(&SettingsField::UndoLimit));
    assert!(fields.contains(&SettingsField::ShortcutMode));

    let mut form = SettingsForm::new(Preferences::default(), None);
    form.open_field(SettingsField::McpMax);
    assert_eq!(form.focus, SettingsField::McpMax);
    assert!(
        SECTIONS[form.section]
            .fields
            .contains(&SettingsField::McpMax)
    );
}
