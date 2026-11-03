//! Shortcut recording, scope inheritance, and validation in the settings dialog.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::input::{Action, Chord, Region};
use sqln::settings::shortcuts::Shortcuts;
use sqln::settings::{Field, SECTIONS, SettingsForm, SettingsRequest};
use sqln::store::Scope;
use sqln::store::config::{KeyboardName, Preferences};
use sqln::ui::settings::draw_settings;
use sqln::ui::theme::Theme;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn preferences(json: &str) -> Preferences {
    serde_json::from_str(json).unwrap()
}

fn form() -> SettingsForm {
    SettingsForm::new(Preferences::default(), None)
}

fn press(form: &mut SettingsForm, field: Field) {
    form.focus_on(field);
    form.press();
}

fn start(form: &mut SettingsForm) {
    press(form, Field::ShortcutRecord);
    assert!(form.shortcuts.is_recording());
}

fn record(form: &mut SettingsForm, codes: &[KeyCode]) {
    start(form);
    for code in codes {
        assert!(matches!(
            form.on_key(key(*code), false),
            SettingsRequest::None
        ));
    }
    form.on_key(key(KeyCode::Enter), false);
    assert!(!form.shortcuts.is_recording());
}

#[test]
fn the_settings_record_a_long_sequence_and_keep_the_other_keys() {
    let mut form = form();
    assert!(
        SECTIONS
            .iter()
            .any(|section| section.fields.contains(&Field::ShortcutRecord))
    );
    record(
        &mut form,
        &[
            KeyCode::F(8),
            KeyCode::Char('a'),
            KeyCode::Char('b'),
            KeyCode::Char('c'),
        ],
    );
    assert_eq!(form.shortcut_keys(), ["f8 a b c", "f5", "ctrl+w e"]);
    let saved = form.preferences().unwrap();
    assert_eq!(
        saved.keymap.unwrap().conventional["run"],
        ["f8 a b c", "f5", "ctrl+w e"]
    );
}

#[test]
fn a_recording_can_add_an_alternative_without_replacing_the_other_keys() {
    let mut form = form();
    form.shortcuts.binding = form.shortcut_keys().len();
    record(&mut form, &[KeyCode::F(8)]);
    assert_eq!(form.shortcut_keys(), ["ctrl+enter", "f5", "ctrl+w e", "f8"]);
    assert!(form.preferences().is_ok());
}

#[test]
fn an_empty_recording_stays_open_and_escape_discards_it() {
    let mut form = form();
    let before = form.preferences().unwrap();
    start(&mut form);
    assert!(form.preferences().is_err());
    form.on_key(key(KeyCode::Enter), false);
    assert!(form.shortcuts.is_recording());
    assert!(form.note.as_ref().unwrap().contains("at least one key"));
    form.on_key(key(KeyCode::F(8)), false);
    form.on_key(key(KeyCode::Esc), false);
    assert!(!form.shortcuts.is_recording());
    assert_eq!(form.preferences().unwrap(), before);
}

#[test]
fn mouse_confirmation_can_record_enter_and_tab() {
    let mut form = form();
    press(&mut form, Field::ShortcutFinish);
    assert!(form.shortcuts.mouse_confirm);
    start(&mut form);
    for code in [
        KeyCode::F(8),
        KeyCode::Enter,
        KeyCode::Tab,
        KeyCode::Char('a'),
    ] {
        form.on_key(key(code), false);
    }
    assert!(form.shortcuts.is_recording());
    assert_eq!(form.shortcuts.recorded(), "f8 enter tab a");
    press(&mut form, Field::ShortcutOk);
    assert!(!form.shortcuts.is_recording());
    assert_eq!(form.shortcut_keys()[0], "f8 enter tab a");
    assert!(form.preferences().is_ok());
}

#[test]
fn the_mouse_can_confirm_or_cancel_a_keyboard_recording() {
    let mut form = form();
    start(&mut form);
    form.on_key(key(KeyCode::F(8)), false);
    press(&mut form, Field::ShortcutOk);
    assert_eq!(form.shortcut_keys()[0], "f8");
    start(&mut form);
    form.on_key(key(KeyCode::F(9)), false);
    press(&mut form, Field::ShortcutCancel);
    assert_eq!(form.shortcut_keys()[0], "f8");
}

#[test]
fn recording_does_not_change_sections_or_accept_a_save_click() {
    let mut form = form();
    start(&mut form);
    form.open_section(1);
    assert_eq!(form.section, 0);
    form.focus_on(Field::Save);
    assert_eq!(form.focus, Field::ShortcutOk);
    form.on_key(key(KeyCode::Tab), false);
    form.on_key(key(KeyCode::Down), false);
    assert_eq!(form.shortcuts.recorded(), "tab down");
    assert!(form.preferences().is_err());
}

#[test]
fn recording_ignores_repeats_and_releases_and_reports_unsupported_keys() {
    let mut form = form();
    start(&mut form);
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        let mut event = key(KeyCode::F(8));
        event.kind = kind;
        form.on_key(event, false);
    }
    assert_eq!(form.shortcuts.recorded(), "");
    form.on_key(key(KeyCode::Null), false);
    assert!(form.note.as_ref().unwrap().contains("cannot be recorded"));
    assert_eq!(form.shortcuts.recorded(), "");
    form.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT), false);
    form.on_key(key(KeyCode::Enter), false);
    assert_eq!(form.shortcut_keys()[0], "alt+enter");
}

#[test]
fn a_conflict_names_both_actions_and_blocks_the_save() {
    let mut form = form();
    record(&mut form, &[KeyCode::F(1)]);
    let error = form.preferences().unwrap_err();
    assert!(
        error.contains("run") && error.contains("help") && error.contains("f1"),
        "{error}"
    );
    assert_eq!(form.note.as_deref(), Some(error.as_str()));
    press(&mut form, Field::ShortcutRestore);
    assert!(form.preferences().is_ok());
}

#[test]
fn a_long_sequence_conflict_keeps_both_action_names_visible_on_a_small_screen() {
    let sequence = format!("f8 {}", vec!["a"; 40].join(" "));
    let global: Preferences = serde_json::from_value(serde_json::json!({
        "keymap": { "conventional": { "stop": [sequence] } }
    }))
    .unwrap();
    let mut form = SettingsForm::new(global, None);
    form.open_section(SECTIONS.len() - 1);
    let mut keys = vec![KeyCode::F(8)];
    keys.extend(vec![KeyCode::Char('a'); 40]);
    record(&mut form, &keys);
    assert!(form.preferences().is_err());
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| {
            draw_settings(frame, &form, &Theme::plain());
        })
        .unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(screen.contains("run conflicts with stop"), "{screen}");
}

#[test]
fn a_prefix_conflict_blocks_the_save() {
    let mut form = form();
    start(&mut form);
    form.on_key(
        KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
        false,
    );
    form.on_key(key(KeyCode::Enter), false);
    let error = form.preferences().unwrap_err();
    assert!(error.contains("ctrl+w") && error.contains("run"), "{error}");
}

#[test]
fn removing_the_last_shortcut_disables_the_action_and_restore_removes_the_override() {
    let mut form = form();
    for _ in 0..3 {
        press(&mut form, Field::ShortcutRemove);
    }
    assert!(form.shortcut_keys().is_empty());
    assert!(form.preferences().unwrap().keymap.unwrap().conventional["run"].is_empty());
    press(&mut form, Field::ShortcutRestore);
    assert_eq!(form.shortcut_keys(), ["ctrl+enter", "f5", "ctrl+w e"]);
    assert!(form.preferences().unwrap().keymap.is_none());
}

#[test]
fn project_restore_uses_the_global_keys_without_changing_the_global_file() {
    let global = preferences(r#"{"keymap":{"conventional":{"run":["f8"]}}}"#);
    let project = preferences(r#"{"keymap":{"conventional":{"run":["f9"]}}}"#);
    let mut form = SettingsForm::new(global.clone(), Some(project));
    assert_eq!(form.scope, Scope::Project);
    assert_eq!(form.shortcut_keys(), ["f9"]);
    press(&mut form, Field::ShortcutRestore);
    assert_eq!(form.shortcut_keys(), ["f8"]);
    assert!(form.preferences().unwrap().keymap.is_none());
    press(&mut form, Field::Scope);
    assert_eq!(form.scope, Scope::Global);
    assert_eq!(form.preferences().unwrap(), global);
}

#[test]
fn the_scopes_keep_separate_shortcut_drafts() {
    let mut form = SettingsForm::new(Preferences::default(), Some(Preferences::default()));
    record(&mut form, &[KeyCode::F(8)]);
    press(&mut form, Field::Scope);
    record(&mut form, &[KeyCode::F(9)]);
    assert_eq!(form.shortcut_keys()[0], "f9");
    press(&mut form, Field::Scope);
    assert_eq!(form.shortcut_keys()[0], "f8");
    assert_eq!(
        form.preferences().unwrap().keymap.unwrap().conventional["run"][0],
        "f8"
    );
}

#[test]
fn a_conflict_between_scopes_blocks_a_global_edit() {
    let project = preferences(r#"{"keymap":{"conventional":{"quit":["f8"]}}}"#);
    let mut form = SettingsForm::new(Preferences::default(), Some(project));
    press(&mut form, Field::Scope);
    record(&mut form, &[KeyCode::F(8)]);
    let error = form.preferences().unwrap_err();
    assert!(
        error.contains("Global + Project") && error.contains("quit") && error.contains("run"),
        "{error}"
    );
}

#[test]
fn a_conflict_with_inherited_global_keys_blocks_a_project_edit() {
    let global = preferences(r#"{"keymap":{"conventional":{"quit":["f8"]}}}"#);
    let mut form = SettingsForm::new(global, Some(Preferences::default()));
    record(&mut form, &[KeyCode::F(8)]);
    let error = form.preferences().unwrap_err();
    assert!(error.contains("quit") && error.contains("run"), "{error}");
}

#[test]
fn both_modes_are_editable_and_an_inactive_mode_conflict_blocks_the_save() {
    let mut form = form();
    record(&mut form, &[KeyCode::F(8)]);
    press(&mut form, Field::ShortcutMode);
    assert_eq!(form.shortcuts.mode, KeyboardName::Vim);
    assert_eq!(form.shortcut_keys()[0], "ctrl+enter");
    record(&mut form, &[KeyCode::F(1)]);
    let error = form.preferences().unwrap_err();
    assert!(error.starts_with("Vim"), "{error}");
    press(&mut form, Field::ShortcutMode);
    assert!(form.preferences().is_err());
    press(&mut form, Field::ShortcutMode);
    press(&mut form, Field::ShortcutRestore);
    assert_eq!(
        form.preferences().unwrap().keymap.unwrap().conventional["run"][0],
        "f8"
    );
}

#[test]
fn the_command_list_includes_every_panel_and_commands_without_keys() {
    let global = preferences(r#"{"keymap":{"conventional":{"run":[]}}}"#);
    let mut form = SettingsForm::new(global, None);
    let commands = form.shortcut_commands();
    assert!(
        commands
            .iter()
            .find(|(action, _)| *action == Action::Run)
            .unwrap()
            .1
            .is_empty()
    );
    for region in [Region::Global, Region::Navigator, Region::Grid] {
        for action in Action::in_region(region) {
            assert_eq!(
                commands.iter().filter(|(item, _)| *item == action).count(),
                1
            );
        }
    }
    let first = Action::in_region(Region::Navigator).next().unwrap();
    form.edit_shortcut(first);
    record(&mut form, &[KeyCode::F(5)]);
    assert!(form.preferences().is_ok(), "a panel can use a global key");
}

#[test]
fn the_command_list_moves_and_opens_the_selected_command() {
    let mut form = form();
    form.open_section(SECTIONS.len() - 1);
    form.focus_on(Field::ShortcutCommands);
    let actions: Vec<_> = Shortcuts::actions().collect();
    form.on_key(key(KeyCode::Down), false);
    assert_eq!(form.shortcuts.action, actions[1]);
    form.on_key(key(KeyCode::Up), false);
    assert_eq!(form.shortcuts.action, actions[0]);
    form.on_key(key(KeyCode::PageDown), false);
    assert_eq!(form.shortcuts.action, actions[8]);
    form.on_key(key(KeyCode::End), false);
    assert_eq!(form.shortcuts.action, *actions.last().unwrap());
    form.on_key(key(KeyCode::Down), false);
    assert_eq!(form.shortcuts.action, *actions.last().unwrap());
    form.on_key(key(KeyCode::Home), false);
    form.on_key(key(KeyCode::Up), false);
    assert_eq!(form.shortcuts.action, actions[0]);
    form.on_key(key(KeyCode::Enter), false);
    assert!(form.shortcuts.is_editing());
    assert_eq!(form.focus, Field::ShortcutBinding);
    record(&mut form, &[KeyCode::F(8)]);
    form.on_key(key(KeyCode::Esc), false);
    assert!(!form.shortcuts.is_editing());
    assert_eq!(form.focus, Field::ShortcutCommands);
    assert_eq!(form.shortcut_commands()[0].1[0], "f8");
}

#[test]
fn the_command_list_scrolls_to_the_selection_and_shows_multiple_commands() {
    let mut form = form();
    form.open_section(SECTIONS.len() - 1);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    for last in [false, true] {
        if last {
            form.move_shortcut_command(isize::MAX);
        }
        terminal
            .draw(|frame| {
                let layout = draw_settings(frame, &form, &Theme::plain());
                assert!(layout.command_rows.len() > 1);
                assert!(
                    layout
                        .command_rows
                        .iter()
                        .any(|(action, _)| *action == form.shortcuts.action)
                );
                assert!(
                    !layout
                        .fields
                        .iter()
                        .any(|(field, _)| *field == Field::ShortcutRecord)
                );
                assert!(
                    layout
                        .command_rows
                        .iter()
                        .all(|(_, row)| row.bottom() < frame.area().bottom())
                );
            })
            .unwrap();
    }
}

#[test]
fn the_command_list_uses_the_keys_of_the_selected_mode_and_scope() {
    let global = preferences(r#"{"keymap":{"conventional":{"run":["f8"]},"vim":{"run":["f9"]}}}"#);
    let project = preferences(r#"{"keymap":{"conventional":{"run":["f10"]}}}"#);
    let mut form = SettingsForm::new(global, Some(project));
    assert_eq!(form.shortcut_commands()[0].1, ["f10"]);
    press(&mut form, Field::ShortcutMode);
    assert_eq!(form.shortcut_commands()[0].1, ["f9"]);
    press(&mut form, Field::Scope);
    press(&mut form, Field::ShortcutMode);
    assert_eq!(form.shortcut_commands()[0].1, ["f8"]);
}

#[test]
fn recorded_chords_round_trip_through_the_configuration_parser() {
    for event in [
        key(KeyCode::BackTab),
        key(KeyCode::Char(' ')),
        key(KeyCode::Char('+')),
        KeyEvent::new(KeyCode::Char('+'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::F(8), KeyModifiers::SHIFT | KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT),
    ] {
        let chord = Chord::from_event(event);
        assert_eq!(Chord::parse(&chord.text().unwrap()).unwrap(), chord);
    }
}

#[test]
fn the_recording_view_has_mouse_buttons_and_no_save_target() {
    let mut form = form();
    form.open_section(SECTIONS.len() - 1);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let theme = Theme::plain();
    start(&mut form);
    terminal
        .draw(|frame| {
            let layout = draw_settings(frame, &form, &theme);
            assert_eq!(layout.fields.len(), 2);
            for field in [Field::ShortcutOk, Field::ShortcutCancel] {
                let (_, area) = layout
                    .fields
                    .iter()
                    .find(|(item, _)| *item == field)
                    .unwrap();
                assert!(area.width > 0 && area.y < frame.area().height);
            }
        })
        .unwrap();
}
