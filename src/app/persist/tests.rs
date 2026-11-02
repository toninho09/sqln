use super::*;
use crate::clipboard::MemoryClipboard;
use crate::connector::TransactionState;
use crate::editor::RunScope;
use crate::input::Action;
use crate::ui::theme::Theme;
use crossterm::event::Event;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender};

struct Release(Option<Sender<()>>);

impl Release {
    fn now(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

impl Drop for Release {
    fn drop(&mut self) {
        self.now();
    }
}

fn app() -> (App, Root) {
    let path = std::env::current_dir()
        .unwrap()
        .join("target/tmp")
        .join(format!("save-worker-{}", uuid::Uuid::new_v4()));
    let root = Root::global(path);
    std::fs::create_dir_all(&root.dir).unwrap();
    std::fs::write(root.config_file(), r#"{"version":1,"connections":[{"id":"test","name":"test","kind":"sqlite","file":"database.db"},{"id":"other","name":"other","kind":"sqlite","file":"other.db"}]}"#).unwrap();
    let options = Options {
        store: Some(Store {
            global: root.clone(),
            project: None,
        }),
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    (App::new(options, clipboard, Theme::plain()), root)
}

#[test]
fn a_move_and_close_keep_the_latest_edits_after_the_console_changes_id() {
    let (mut app, root) = app();
    app.on_event(Event::Paste("SELECT 1;".into()));
    app.save_now();
    pump(&mut app, Instant::now());
    let original = app.console().id;
    let target = app.connections[1].id;
    let (started, mut release) = gate(&mut app);
    app.associate(original, target);
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    app.close_step(original);
    app.on_event(Event::Paste("\nSELECT 2;".into()));
    let expected = app.console().editor.text();
    release.now();
    pump(&mut app, Instant::now());
    assert_ne!(app.console().id, original);
    assert_eq!(app.console().connection, Some(target));
    assert!(app.should_quit());
    assert_eq!(saved_text(&app, &root), expected);
}

fn association_target(app: &mut App, root: &Root, scope: Scope) -> (ConnectionId, Root) {
    let target = app.connections[1].id;
    let destination = match scope {
        Scope::Global => root.clone(),
        Scope::Project => {
            let project = Root::project(&root.dir.join("project"));
            app.persist.as_mut().unwrap().store.project = Some(project.clone());
            app.connections[1].key.as_mut().unwrap().scope = scope;
            project
        }
    };
    (target, destination)
}

fn assert_association_blocks_runs(app: &mut App) {
    for action in [Action::Run, Action::RunScript] {
        app.perform(action, false);
        assert_eq!(app.status.as_deref(), Some(text::CONNECTION_CHANGE_PENDING));
        assert!(!app.console().is_busy());
        assert_eq!(app.console().transaction, TransactionState::None);
        assert!(!app.command_available(action));
    }
    let id = app.console().id;
    for action in [
        DialogAction::RunConfirmed(id, RunScope::Script),
        DialogAction::RunAnyway(id, RunScope::Script),
    ] {
        app.on_dialog_action(action);
        assert_eq!(app.status.as_deref(), Some(text::CONNECTION_CHANGE_PENDING));
        assert!(!app.console().is_busy());
    }
}

#[test]
fn association_blocks_runs_until_the_move_completes_in_either_scope() {
    for scope in [Scope::Global, Scope::Project] {
        let (mut app, root) = app();
        app.on_event(Event::Paste("BEGIN; SELECT 1;".into()));
        app.save_now();
        pump(&mut app, Instant::now());
        let original = app.console().connection;
        let (target, _) = association_target(&mut app, &root, scope);
        let (started, mut release) = gate(&mut app);
        app.associate(app.console().id, target);
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_association_blocks_runs(&mut app);
        assert_eq!(app.console().connection, original);
        release.now();
        pump(&mut app, Instant::now());
        assert_eq!(app.console().connection, Some(target));
        assert!(app.command_available(Action::Run));
        assert!(app.command_available(Action::RunScript));
        app.start_run(0, RunScope::Script);
        assert!(app.console().is_busy());
    }
}

#[test]
fn association_blocks_runs_while_it_saves_the_source_text() {
    let (mut app, _) = app();
    app.on_event(Event::Paste("SELECT 1;".into()));
    app.ensure_file(0);
    let target = app.connections[1].id;
    let (started, mut release) = gate(&mut app);
    app.associate(app.console().id, target);
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        app.persist.as_ref().unwrap().saves[&app.console().id]
            .association_running
            .is_none()
    );
    assert_association_blocks_runs(&mut app);
    release.now();
    pump(&mut app, Instant::now());
    assert_eq!(app.console().connection, Some(target));
}

#[test]
fn association_in_the_queue_blocks_only_its_console() {
    let (mut app, _) = app();
    app.on_event(Event::Paste("SELECT 1;".into()));
    app.save_now();
    pump(&mut app, Instant::now());
    let source = app.console().connection.unwrap();
    let target = app.connections[1].id;
    let (started, mut release) = gate(&mut app);
    app.new_console(source);
    app.on_event(Event::Paste("SELECT 2;".into()));
    app.save_now();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    app.select_console(0);
    app.associate(app.console().id, target);
    assert!(!app.console().file.as_ref().unwrap().is_saving());
    assert_association_blocks_runs(&mut app);
    app.select_console(1);
    assert!(app.command_available(Action::Run));
    app.start_run(1, RunScope::Script);
    assert!(app.console().is_busy());
    release.now();
    pump(&mut app, Instant::now());
    assert_eq!(app.consoles[0].connection, Some(target));
    assert_eq!(app.consoles[1].connection, Some(source));
}

#[test]
fn association_failure_allows_runs_on_the_source_connection_again() {
    let (mut app, root) = app();
    app.on_event(Event::Paste("SELECT 1;".into()));
    app.save_now();
    pump(&mut app, Instant::now());
    let original = app.console().connection;
    let script = app.console().file.as_ref().unwrap().meta.id.clone();
    let (target, destination) = association_target(&mut app, &root, Scope::Project);
    let _lock = scripts::lock(&destination, &script).unwrap().unwrap();
    let (started, mut release) = gate(&mut app);
    app.associate(app.console().id, target);
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_association_blocks_runs(&mut app);
    release.now();
    pump(&mut app, Instant::now());
    assert_eq!(app.console().connection, original);
    assert_eq!(app.console().file.as_ref().unwrap().scope, Scope::Global);
    assert!(matches!(
        app.console().file.as_ref().unwrap().trouble,
        Some(Trouble::Failed(_))
    ));
    assert!(app.command_available(Action::Run));
    app.start_run(0, RunScope::Script);
    assert!(app.console().is_busy());
}

#[test]
fn association_blocks_export_reads_before_they_create_a_file() {
    let (mut app, root) = app();
    app.on_event(Event::Paste("SELECT 1;".into()));
    app.save_now();
    pump(&mut app, Instant::now());
    let original = app.console().connection.unwrap();
    let target = app.connections[1].id;
    let (started, mut release) = gate(&mut app);
    app.associate(app.console().id, target);
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let destination = root.dir.join("export.csv");
    app.authorize(
        original,
        super::super::auth::Use::Export {
            console: app.console().id,
            read: super::super::export::ExportRead {
                sql: "SELECT 1;".into(),
                schema: None,
                destination: destination.clone(),
                options: crate::export::ExportOptions::default(),
            },
        },
    );
    assert_eq!(app.status.as_deref(), Some(text::CONNECTION_CHANGE_PENDING));
    assert!(!destination.exists());
    assert!(app.exports.is_empty());
    assert!(!app.console().is_busy());
    release.now();
    pump(&mut app, Instant::now());
    assert_eq!(app.console().connection, Some(target));
}

#[test]
fn a_failed_first_write_keeps_the_script_lock() {
    let (mut app, root) = app();
    app.on_event(Event::Paste("SELECT 1;".into()));
    app.ensure_file(0);
    let id = app.console().file.as_ref().unwrap().meta.id.clone();
    std::fs::create_dir_all(root.consoles_dir().join(format!("{id}.sql"))).unwrap();
    app.save_now();
    pump(&mut app, Instant::now());
    assert!(matches!(
        app.console().file.as_ref().unwrap().trouble,
        Some(Trouble::Failed(_))
    ));
    assert!(scripts::lock(&root, &id).unwrap().is_none());
}

#[test]
fn discarding_a_close_does_not_write_later_edits_or_wait_in_the_interface() {
    let (mut app, root) = app();
    let (started, mut release) = gate(&mut app);
    app.on_event(Event::Paste("SELECT 1;".into()));
    app.save_now();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    app.on_event(Event::Paste("\nSELECT 2;".into()));
    app.console_mut().discard_text = true;
    app.close_step(app.console().id);
    assert!(!app.should_quit());
    release.now();
    pump(&mut app, Instant::now());
    assert!(app.should_quit());
    assert_eq!(saved_text(&app, &root), "SELECT 1;");
}

fn gate(app: &mut App) -> (Receiver<()>, Release) {
    let (worker, started, release) = SaveWorker::gated();
    app.persist.as_mut().unwrap().worker = worker;
    (started, Release(Some(release)))
}

fn pump(app: &mut App, now: Instant) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.has_pending_saves() {
        assert!(Instant::now() < deadline, "pending writes do not finish");
        app.tick_at(now);
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn saved_text(app: &App, root: &Root) -> String {
    let file = app.console().file.as_ref().unwrap();
    scripts::read_text(root, &file.meta.id).unwrap().0
}

#[test]
fn editing_and_ticks_continue_while_a_write_waits() {
    let (mut app, root) = app();
    let (started, mut release) = gate(&mut app);
    app.on_event(Event::Paste("SELECT 1;".into()));
    app.save_now();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(app.status.as_deref() != Some(text::SAVED));
    app.on_event(Event::Paste("\nSELECT 2;".into()));
    let expected = app.console().editor.text();
    let now = Instant::now();
    app.tick_at(now);
    assert!(!app.should_quit());
    assert!(app.console().file.as_ref().unwrap().is_saving());
    assert_eq!(app.console().editor.text(), expected);
    release.now();
    pump(&mut app, now);
    assert_eq!(saved_text(&app, &root), expected);
    assert!(
        !app.console()
            .file
            .as_ref()
            .unwrap()
            .is_dirty(app.console().editor.revision())
    );
    assert_eq!(app.status.as_deref(), Some(text::SAVED));
}

#[test]
fn closing_waits_for_edits_made_after_the_first_save_request() {
    let (mut app, root) = app();
    let (started, mut release) = gate(&mut app);
    app.on_event(Event::Paste("SELECT 1;".into()));
    app.save_now();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    app.close_step(app.console().id);
    app.on_event(Event::Paste("\nSELECT 2;".into()));
    let expected = app.console().editor.text();
    assert!(!app.should_quit());
    release.now();
    pump(&mut app, Instant::now());
    assert!(app.should_quit());
    assert_eq!(saved_text(&app, &root), expected);
}

#[test]
fn a_delayed_save_keeps_an_external_change_and_the_editor_text() {
    let (mut app, root) = app();
    app.on_event(Event::Paste("SELECT 1;".into()));
    app.save_now();
    pump(&mut app, Instant::now());
    let id = app.console().file.as_ref().unwrap().meta.id.clone();
    let (started, mut release) = gate(&mut app);
    app.on_event(Event::Paste("\nSELECT 2;".into()));
    let expected = app.console().editor.text();
    app.save_now();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    scripts::write_text(&root, &id, "external", file::Expect::Any).unwrap();
    app.poll_files(Instant::now());
    assert_eq!(app.console().file.as_ref().unwrap().trouble, None);
    release.now();
    pump(&mut app, Instant::now());
    assert_eq!(saved_text(&app, &root), "external");
    assert_eq!(app.console().editor.text(), expected);
    assert_eq!(
        app.console().file.as_ref().unwrap().trouble,
        Some(Trouble::Changed)
    );
}

#[test]
fn metadata_changes_wait_without_blocking_the_interface() {
    let (mut app, root) = app();
    let (started, mut release) = gate(&mut app);
    app.on_event(Event::Paste("SELECT 1;".into()));
    app.save_now();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    app.console_mut().schema = Some("other".into());
    app.save_console_meta(0);
    release.now();
    pump(&mut app, Instant::now());
    let file = app.console().file.as_ref().unwrap();
    let entries = scripts::list(&root, Scope::Global);
    let entry = entries
        .iter()
        .find(|entry| entry.meta.id == file.meta.id)
        .unwrap();
    assert_eq!(entry.meta.schema.as_deref(), Some("other"));
}

#[test]
fn emergency_save_applies_the_pending_write_before_the_latest_revision() {
    let (mut app, root) = app();
    let (started, mut release) = gate(&mut app);
    app.on_event(Event::Paste("SELECT 1;".into()));
    app.save_now();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    app.on_event(Event::Paste("\nSELECT 2;".into()));
    let expected = app.console().editor.text();
    release.now();
    app.save_without_questions();
    assert_eq!(saved_text(&app, &root), expected);
    assert_eq!(app.console().file.as_ref().unwrap().trouble, None);
}
