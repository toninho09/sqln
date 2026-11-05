//! Assistant behavior through the private IPC, input events, and the existing executor.
mod common;

use crossterm::event::{Event, KeyCode};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};
use sqln::agent::ipc::{Client, Instance};
use sqln::agent::{self, Edit, EditMode, Operation, Position, Reply};
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::editor::KeyboardMode;
use sqln::input::{Action, Keymap};
use sqln::sql::Dialect;
use sqln::ui::theme::Theme;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

struct Harness {
    app: App,
    runtime: tokio::runtime::Runtime,
    client: Option<Client>,
    directory: PathBuf,
    instance: Instance,
}
impl Harness {
    fn new(name: &str) -> Self {
        Self::mode(name, KeyboardMode::Conventional)
    }
    fn mode(name: &str, mode: KeyboardMode) -> Self {
        let path = common::temp_db(&format!("agent-{name}"));
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE things(id INTEGER PRIMARY KEY, name TEXT, data BLOB); INSERT INTO things VALUES(1, 'one', X'00ff'),(2, 'two', NULL);").unwrap();
        drop(db);
        Self::options(
            Options {
                database: Some(path),
                keyboard: mode,
                ..Default::default()
            },
            mode,
        )
    }
    fn options(mut options: Options, mode: KeyboardMode) -> Self {
        let overrides = HashMap::from([
            ("agentSessions".into(), vec!["f8".into()]),
            ("agentReview".into(), vec!["f9".into()]),
            ("agentDirect".into(), vec!["f10".into()]),
        ]);
        options
            .keymap
            .get_or_insert_with(|| Keymap::with_overrides(mode, &overrides).unwrap());
        let mut app = App::new(
            options,
            Rc::new(RefCell::new(MemoryClipboard::default())),
            Theme::plain(),
        );
        let directory = std::env::temp_dir().join(format!(
            "sn-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..12]
        ));
        let id = app.start_agent_at(directory.clone()).unwrap();
        let instance: Instance =
            serde_json::from_slice(&std::fs::read(directory.join(format!("{id}.json"))).unwrap())
                .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let client = runtime
            .block_on(Client::connect(&instance, "test agent\u{1b}[31m"))
            .unwrap();
        Self {
            app,
            runtime,
            client: Some(client),
            directory,
            instance,
        }
    }
    fn call(&mut self, op: Operation) -> Reply {
        let client = self.client.as_mut().unwrap();
        let app = &mut self.app;
        self.runtime.block_on(async {
            let request=client.call(&op);tokio::pin!(request);
            let limit=tokio::time::sleep(Duration::from_secs(15));tokio::pin!(limit);
            loop{tokio::select!{result=&mut request=>return result,()=&mut limit=>panic!("assistant request timed out"),()=tokio::time::sleep(Duration::from_millis(1))=>{app.drain_events();}}}
        })
    }
    fn key(&mut self, code: KeyCode) {
        self.app.on_event(Event::Key(common::key(code)));
    }
    fn authorize(&mut self) {
        assert_eq!(
            self.call(Operation::RequestAccess {}).unwrap()["status"],
            "pending"
        );
        self.key(KeyCode::F(8));
        self.key(KeyCode::Enter);
        self.key(KeyCode::Right);
        self.key(KeyCode::Enter);
        assert_eq!(
            self.call(Operation::GetAccess {}).unwrap()["status"],
            "authorized"
        );
    }
    fn connection(&self) -> String {
        self.app.connections()[0].id.to_string()
    }
    fn own(&mut self) -> String {
        self.call(Operation::OpenAgentConsole {
            connection_id: self.connection(),
            schema: None,
        })
        .unwrap()["console_id"]
            .as_str()
            .unwrap()
            .into()
    }
    fn query(&mut self, console: &str, sql: &str) -> String {
        self.call(Operation::RunReadQuery {
            console_id: console.into(),
            sql: sql.into(),
            parameters: vec![],
        })
        .unwrap()["job_id"]
            .as_str()
            .unwrap()
            .into()
    }
    fn wait_job(&mut self, id: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let job = self.call(Operation::GetJob { job_id: id.into() }).unwrap();
            if ["completed", "failed", "cancelled"].contains(&job["status"].as_str().unwrap()) {
                return job;
            }
            assert!(Instant::now() < deadline, "unfinished job: {job:?}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn draw(&mut self, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn click_label(&mut self, label: &str) {
        let screen = self.draw(80, 24);
        let (x, y) = screen
            .lines()
            .enumerate()
            .find_map(|(y, line)| {
                line.find(label)
                    .map(|byte| (unicode_width::UnicodeWidthStr::width(&line[..byte]) + 1, y))
            })
            .unwrap_or_else(|| panic!("Missing {label}:\n{screen}"));
        self.app
            .on_event(Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: x as u16,
                row: y as u16,
                modifiers: crossterm::event::KeyModifiers::NONE,
            }));
    }
    fn user_sql(&mut self, sql: &str) {
        self.app
            .console_mut()
            .editor
            .on_command(Action::EditorSelectAll, None);
        self.app.console_mut().editor.on_paste(sql);
        self.key(KeyCode::F(5));
        let deadline = Instant::now() + Duration::from_secs(20);
        while self.app.console().is_busy() {
            self.app.drain_events();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn proposal(&mut self, text: &str) -> String {
        let console = self.app.console();
        let id = console.id.to_string();
        let revision = console.editor.revision();
        self.call(Operation::EditConsole {
            console_id: id,
            revision,
            edits: vec![Edit {
                start: Position { line: 0, column: 0 },
                end: Position { line: 0, column: 0 },
                text: text.into(),
            }],
            mode: EditMode::Propose,
        })
        .unwrap()["proposal_id"]
            .as_str()
            .unwrap()
            .into()
    }
    fn wait_proposal(&mut self, id: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let proposal = self
                .call(Operation::GetProposal {
                    proposal_id: id.into(),
                })
                .unwrap();
            if proposal["status"] != "building" {
                return proposal;
            }
            assert!(Instant::now() < deadline);
        }
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.client.take();
        self.app.shutdown();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn pending_access_notification_survives_typing_without_changing_focus() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut h = Harness::mode("access-notice", mode);
        if mode == KeyboardMode::Vim {
            h.key(KeyCode::Char('i'));
        }
        let active = h.app.console().id;
        let cursor = h.app.console().editor.cursor();
        h.call(Operation::RequestAccess {}).unwrap();
        assert_eq!(h.app.console().id, active);
        assert_eq!(h.app.console().editor.cursor(), cursor);
        h.key(KeyCode::Char('x'));
        let screen = h.draw(80, 24);
        assert!(screen.contains("MCP: 1 access request pending"), "{screen}");
        assert!(screen.contains("F8: review"), "{screen}");
        assert!(!screen.contains("Allow agent access"));
        assert_eq!(h.app.console().editor.text(), "x");
        assert_eq!(h.app.console().id, active);
        h.app.on_event(Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('s'),
            crossterm::event::KeyModifiers::CONTROL,
        )));
        assert!(h.draw(80, 24).contains("MCP: 1 access request pending"));
        h.app.on_event(Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('w'),
            crossterm::event::KeyModifiers::CONTROL,
        )));
        h.click_label("[Review]");
        h.key(KeyCode::Esc);
        h.key(KeyCode::Char('x'));
        assert_eq!(h.app.console().editor.text(), "xx");
        assert_eq!(
            h.call(Operation::GetContext {}).unwrap_err().code,
            "access_required"
        );
        h.authorize();
        assert!(!h.draw(80, 24).contains("access request pending"));
    }
}

#[test]
fn pending_access_notification_supports_home_mouse_resize_and_custom_shortcuts() {
    for shortcut in ["ctrl+w :", "f6"] {
        let keymap = Keymap::with_overrides(
            KeyboardMode::Conventional,
            &HashMap::from([(Action::OpenPalette.name().into(), vec![shortcut.into()])]),
        )
        .unwrap();
        let mut h = Harness::options(
            Options {
                start_home: true,
                keymap: Some(keymap),
                ..Default::default()
            },
            KeyboardMode::Conventional,
        );
        h.key(KeyCode::F(1));
        h.call(Operation::RequestAccess {}).unwrap();
        h.key(KeyCode::Esc);
        h.draw(120, 40);
        let screen = h.draw(80, 24);
        assert!(screen.contains("MCP: 1 access request pending"), "{screen}");
        let label = sqln::ui::text::key_label(shortcut);
        assert!(
            screen.contains(&format!("{label} > Agent sessions")),
            "{screen}"
        );
        h.click_label("[Review]");
        assert!(h.draw(80, 24).contains("Agent sessions"));
        h.key(KeyCode::Enter);
        assert!(h.draw(80, 24).contains("Allow agent access"));
        h.key(KeyCode::Enter);
        assert_eq!(h.call(Operation::GetAccess {}).unwrap()["status"], "denied");
        assert!(!h.draw(80, 24).contains("access request pending"));
    }
}

#[test]
fn pending_access_notification_counts_clients_and_clears_on_disconnect() {
    let mut h = Harness::new("access-notice-clients");
    h.call(Operation::RequestAccess {}).unwrap();
    let first = h.client.take();
    h.client = Some(
        h.runtime
            .block_on(Client::connect(&h.instance, "second client"))
            .unwrap(),
    );
    h.call(Operation::RequestAccess {}).unwrap();
    let second = h.client.replace(first.unwrap()).unwrap();
    assert!(h.draw(80, 24).contains("MCP: 2 access requests pending"));
    drop(second);
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.draw(80, 24).contains("MCP: 2 access requests pending") {
        h.app.drain_events();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(h.draw(80, 24).contains("MCP: 1 access request pending"));
    h.client.take();
    while h.draw(80, 24).contains("access request pending") {
        h.app.drain_events();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(h.app.console().editor.text().is_empty());
}

#[test]
fn a_session_requires_user_approval_and_never_changes_focus() {
    let mut h = Harness::new("access");
    assert_eq!(
        h.call(Operation::ListConnections {
            offset: 0,
            limit: 100
        })
        .unwrap_err()
        .code,
        "access_required"
    );
    let active = h.app.console().id;
    h.authorize();
    let own = h.own();
    let same = h
        .call(Operation::OpenAgentConsole {
            connection_id: h.connection(),
            schema: Some("main".into()),
        })
        .unwrap();
    assert_eq!(same["console_id"], own);
    assert_eq!(h.app.consoles().len(), 2);
    assert_ne!(own, active.to_string());
    assert_eq!(h.app.console().id, active);
    assert!(
        h.app
            .consoles()
            .iter()
            .find(|c| c.id.to_string() == own)
            .unwrap()
            .name
            .contains("Agent:")
    );
    let found = h
        .runtime
        .block_on(agent::ipc::discover(&h.directory))
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].instance_id, h.instance.instance_id);
    let job = h.query(&own, "SELECT id, name, data FROM things ORDER BY id");
    assert_eq!(h.wait_job(&job)["status"], "completed");
    assert_eq!(h.app.console().id, active);
    let result = h
        .call(Operation::ReadResult {
            tab_id: own,
            result: 0,
            offset: 0,
            limit: 100,
            generation: None,
        })
        .unwrap();
    assert_eq!(
        result["rows"][0]["cells"][0],
        json!({"kind":"number","value":"1"})
    );
    assert_eq!(
        result["rows"][0]["cells"][2],
        json!({"kind":"binary","base64":"AP8="})
    );
    assert_eq!(
        result["rows"][1]["cells"][2],
        json!({"kind":"null","value":null})
    );
}

#[test]
fn a_user_console_executes_only_its_current_text_and_rejects_writes() {
    let mut h = Harness::new("current");
    h.authorize();
    let id = h.app.console().id.to_string();
    assert_eq!(
        h.call(Operation::RunReadQuery {
            console_id: id.clone(),
            sql: "SELECT 9".into(),
            parameters: vec![]
        })
        .unwrap_err()
        .code,
        "own_console_required"
    );
    h.app
        .on_event(Event::Paste("SELECT name FROM things ORDER BY id;".into()));
    let revision = h.app.console().editor.revision();
    let job = h
        .call(Operation::RunConsole {
            console_id: id.clone(),
            revision,
            range: None,
        })
        .unwrap()["job_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(h.wait_job(&job)["status"], "completed");
    assert_eq!(
        h.app.console().results[0].table.rows[0].text(0).as_deref(),
        Some("one")
    );
    assert_eq!(
        h.call(Operation::RunConsole {
            console_id: id.clone(),
            revision: revision + 1,
            range: None
        })
        .unwrap_err()
        .code,
        "revision_conflict"
    );
    let own = h.own();
    let job = h.query(&own, "SELECT 1; DELETE FROM things;");
    assert_eq!(h.wait_job(&job)["status"], "failed");
    let job = h.query(&own, "SELECT count(*) FROM things");
    assert_eq!(h.wait_job(&job)["status"], "completed");
    let page = h
        .call(Operation::ReadResult {
            tab_id: own.clone(),
            result: 0,
            offset: 0,
            limit: 10,
            generation: None,
        })
        .unwrap();
    assert_eq!(page["rows"][0]["cells"][0]["value"], "2");
    // The validation worker splits the raw text into one result per statement.
    let job = h.query(&own, "SELECT 1; SELECT 2;");
    assert_eq!(h.wait_job(&job)["status"], "completed");
    let console = h
        .app
        .consoles()
        .iter()
        .find(|console| console.id.to_string() == own)
        .unwrap();
    assert_eq!(console.results.len(), 2);
}

#[test]
fn proposals_need_review_and_direct_edits_need_a_user_option() {
    let mut h = Harness::new("proposal");
    h.authorize();
    let proposal = h.proposal("SELECT '猫';");
    assert_eq!(h.wait_proposal(&proposal)["status"], "pending");
    assert!(h.app.console().editor.text().is_empty());
    h.key(KeyCode::F(9));
    h.key(KeyCode::Enter);
    let screen = h.draw(80, 24);
    assert!(screen.contains("Agent edit"));
    assert!(screen.contains("[Reject]"));
    h.key(KeyCode::Right);
    h.key(KeyCode::Enter);
    assert_eq!(h.app.console().editor.text(), "SELECT '猫';");
    assert_eq!(
        h.call(Operation::GetProposal {
            proposal_id: proposal
        })
        .unwrap()["status"],
        "applied"
    );
    h.app.on_event(Event::Key(common::ctrl('z')));
    assert!(h.app.console().editor.text().is_empty());
    let edit = Edit {
        start: Position { line: 0, column: 0 },
        end: Position { line: 0, column: 0 },
        text: "SELECT 2;".into(),
    };
    let console = h.app.console().id.to_string();
    let revision = h.app.console().editor.revision();
    assert_eq!(
        h.call(Operation::EditConsole {
            console_id: console.clone(),
            revision,
            edits: vec![edit.clone()],
            mode: EditMode::Direct
        })
        .unwrap_err()
        .code,
        "direct_edit_not_allowed"
    );
    h.key(KeyCode::F(10));
    h.key(KeyCode::Right);
    h.key(KeyCode::Enter);
    assert_eq!(
        h.call(Operation::EditConsole {
            console_id: console,
            revision,
            edits: vec![edit],
            mode: EditMode::Direct
        })
        .unwrap()["status"],
        "applied"
    );
    assert_eq!(h.app.console().editor.text(), "SELECT 2;");
    assert!(h.app.console().results.is_empty());
}

#[test]
fn revisions_invalidate_proposals_without_replacing_user_text() {
    let mut h = Harness::new("conflict");
    h.authorize();
    let proposal = h.proposal("SELECT 1;");
    h.wait_proposal(&proposal);
    h.app.on_event(Event::Paste("user text".into()));
    h.app.drain_events();
    assert_eq!(
        h.call(Operation::GetProposal {
            proposal_id: proposal
        })
        .unwrap()["status"],
        "revision_conflict"
    );
    assert_eq!(h.app.console().editor.text(), "user text");
    assert!(h.draw(120, 24).contains("Agent edit discarded"));
}

#[test]
fn disconnect_cancels_its_query_and_not_the_next_user_query() {
    let mut h = Harness::new("disconnect");
    h.authorize();
    let own = h.own();
    let _job=h.query(&own,"WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<1000000000) SELECT count(*) FROM n");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !h
        .app
        .consoles()
        .iter()
        .find(|c| c.id.to_string() == own)
        .unwrap()
        .is_busy()
    {
        h.app.drain_events();
        assert!(Instant::now() < deadline);
    }
    h.client.take();
    while h
        .app
        .consoles()
        .iter()
        .find(|c| c.id.to_string() == own)
        .unwrap()
        .is_busy()
    {
        h.app.drain_events();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    h.app.on_event(Event::Paste("SELECT 42".into()));
    h.key(KeyCode::F(5));
    while h.app.console().is_busy() {
        h.app.drain_events();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        h.app.console().results[0].table.rows[0].text(0).as_deref(),
        Some("42")
    );
}

#[test]
fn ddl_diagrams_and_comparisons_reuse_the_existing_models() {
    let mut h = Harness::new("analysis");
    h.authorize();
    let connection = h.connection();
    let active = h.app.console().id;
    let reply = h
        .call(Operation::GetObjectDdl {
            connection_id: connection.clone(),
            schema: "main".into(),
            name: "things".into(),
            kind: agent::DdlKind::Table,
            identity: None,
        })
        .unwrap();
    let ddl = reply["job_id"].as_str().unwrap().to_owned();
    let tab = reply["tab_ids"][0].as_str().unwrap().to_owned();
    assert_eq!(h.wait_job(&ddl)["status"], "completed");
    // The DDL text reads the rows in place, and the grid keeps them.
    let console = h
        .app
        .consoles()
        .iter()
        .find(|console| console.id.to_string() == tab)
        .unwrap();
    assert!(
        console
            .results
            .iter()
            .any(|grid| !grid.table.rows.is_empty())
    );
    let text = h
        .call(Operation::ReadArtifact {
            job_id: ddl,
            offset: 0,
        })
        .unwrap();
    assert!(text["text"].as_str().unwrap().contains("CREATE TABLE"));
    let diagram = h
        .call(Operation::GetErDiagram {
            connection_id: connection.clone(),
            schema: "main".into(),
            table: None,
        })
        .unwrap()["job_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(h.wait_job(&diagram)["status"], "completed");
    assert!(
        h.call(Operation::ReadArtifact {
            job_id: diagram,
            offset: 0
        })
        .unwrap()["text"]
            .as_str()
            .unwrap()
            .contains("erDiagram")
    );
    let side = agent::Side {
        connection_id: connection,
        schema: "main".into(),
        table: Some("things".into()),
    };
    for op in [
        Operation::CompareSchema {
            source: side.clone(),
            target: side.clone(),
            removals: false,
        },
        Operation::CompareData {
            source: side.clone(),
            target: side,
            removals: false,
        },
    ] {
        let job = h.call(op).unwrap()["job_id"].as_str().unwrap().to_owned();
        let result = h.wait_job(&job);
        assert_eq!(result["status"], "completed", "{result:?}");
    }
    assert_eq!(h.app.console().id, active);
}

#[test]
fn transactions_refuse_agent_execution_without_changing_the_user_session() {
    let mut h = Harness::new("transaction");
    h.authorize();
    h.user_sql("BEGIN;");
    assert_eq!(
        h.app.console().transaction,
        sqln::connector::TransactionState::Open
    );
    let console = h.app.console().id.to_string();
    let revision = h.app.console().editor.revision();
    assert_eq!(
        h.call(Operation::RunConsole {
            console_id: console.clone(),
            revision,
            range: None
        })
        .unwrap_err()
        .code,
        "transaction_active"
    );
    let own = h.own();
    let job = h.query(&own, "SELECT 8");
    assert_eq!(h.wait_job(&job)["status"], "completed");
    assert_eq!(
        h.app.console().transaction,
        sqln::connector::TransactionState::Open
    );
    h.user_sql("ROLLBACK;");
    assert_eq!(
        h.app.console().transaction,
        sqln::connector::TransactionState::None
    );
    for state in [
        sqln::connector::TransactionState::Failed,
        sqln::connector::TransactionState::Unknown,
    ] {
        h.app.console_mut().set_transaction(state);
        assert_eq!(
            h.call(Operation::RunConsole {
                console_id: console.clone(),
                revision: h.app.console().editor.revision(),
                range: None
            })
            .unwrap_err()
            .code,
            "transaction_active"
        );
    }
    h.app
        .console_mut()
        .set_transaction(sqln::connector::TransactionState::None);
}

#[test]
fn result_generations_and_large_cells_keep_values_exact() {
    let mut h = Harness::new("pages");
    h.authorize();
    let own = h.own();
    let job = h.query(&own, "SELECT id, name, data FROM things ORDER BY id");
    let completed = h.wait_job(&job);
    assert_eq!(completed["status"], "completed");
    let generation = completed["results"][0]["generation"]
        .as_str()
        .unwrap()
        .to_owned();
    let first = h
        .call(Operation::ReadResult {
            tab_id: own.clone(),
            result: 0,
            offset: 0,
            limit: 1,
            generation: Some(generation.clone()),
        })
        .unwrap();
    assert_eq!(first["next_offset"], 1);
    let second = h
        .call(Operation::ReadResult {
            tab_id: own.clone(),
            result: 0,
            offset: 1,
            limit: 1,
            generation: Some(generation.clone()),
        })
        .unwrap();
    assert_eq!(second["rows"][0]["cells"][1]["value"], "two");
    let job = h.query(&own, "SELECT 'replacement'");
    h.wait_job(&job);
    assert_eq!(
        h.call(Operation::ReadResult {
            tab_id: own.clone(),
            result: 0,
            offset: 0,
            limit: 1,
            generation: Some(generation)
        })
        .unwrap_err()
        .code,
        "result_changed"
    );
    let text = "猫\u{0}\n".repeat(10000);
    let job = h
        .call(Operation::RunReadQuery {
            console_id: own.clone(),
            sql: "SELECT :value".into(),
            parameters: vec![agent::Binding {
                name: "value".into(),
                kind: agent::BindingKind::Text,
                value: text.clone(),
            }],
        })
        .unwrap()["job_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let completed = h.wait_job(&job);
    assert_eq!(completed["status"], "completed");
    let generation = completed["results"][0]["generation"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut actual = String::new();
    let mut offset = 0;
    loop {
        let page = h
            .call(Operation::ReadResultCell {
                tab_id: own.clone(),
                result: 0,
                row: 0,
                column: 0,
                generation: generation.clone(),
                offset,
            })
            .unwrap();
        actual.push_str(page["text"].as_str().unwrap());
        match page["next_offset"].as_u64() {
            Some(next) => offset = next as usize,
            None => break,
        }
    }
    assert_eq!(actual, text);
    h.user_sql("INSERT INTO things(id,name) VALUES(3,'user write after agent read');");
    let job = h.query(&own, "SELECT count(*) FROM things");
    h.wait_job(&job);
    assert_eq!(
        h.call(Operation::ReadResult {
            tab_id: own,
            result: 0,
            offset: 0,
            limit: 1,
            generation: None
        })
        .unwrap()["rows"][0]["cells"][0]["value"],
        "3"
    );
}

#[test]
fn mouse_acceptance_applies_a_proposal_without_executing_sql() {
    let mut h = Harness::new("mouse-review");
    h.authorize();
    let proposal = h.proposal("SELECT 99;");
    h.wait_proposal(&proposal);
    h.key(KeyCode::F(9));
    h.key(KeyCode::Enter);
    let screen = h.draw(80, 24);
    let (x, y) = screen
        .lines()
        .enumerate()
        .find_map(|(y, line)| {
            line.find("[Accept]")
                .map(|byte| (line[..byte].chars().count() + 1, y))
        })
        .unwrap();
    h.app.on_event(Event::Mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: x as u16,
        row: y as u16,
        modifiers: crossterm::event::KeyModifiers::NONE,
    }));
    assert_eq!(h.app.console().editor.text(), "SELECT 99;");
    assert!(h.app.console().results.is_empty());
    assert_eq!(
        h.call(Operation::GetProposal {
            proposal_id: proposal
        })
        .unwrap()["status"],
        "applied"
    );
}

#[test]
fn revocation_cancels_the_sessions_work_and_blocks_further_reads() {
    let mut h = Harness::new("revoke");
    h.authorize();
    let own = h.own();
    h.query(&own,"WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<1000000000) SELECT count(*) FROM n");
    h.key(KeyCode::F(8));
    h.key(KeyCode::Enter);
    h.key(KeyCode::Right);
    h.key(KeyCode::Enter);
    assert_eq!(
        h.call(Operation::GetAccess {}).unwrap()["status"],
        "revoked"
    );
    assert_eq!(
        h.call(Operation::GetContext {}).unwrap_err().code,
        "access_required"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while h
        .app
        .consoles()
        .iter()
        .find(|console| console.id.to_string() == own)
        .unwrap()
        .is_busy()
    {
        h.app.drain_events();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(h.draw(80, 24).contains("Agent inactive"));
}

#[test]
fn both_editor_modes_review_proposals_and_keep_default_rejection() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut h = Harness::mode(&format!("review-{mode:?}"), mode);
        h.authorize();
        let proposal = h.proposal("SELECT 11;");
        h.wait_proposal(&proposal);
        h.key(KeyCode::F(9));
        h.key(KeyCode::Enter);
        assert!(h.draw(80, 24).contains("[Reject]"));
        h.draw(100, 30);
        h.draw(80, 24);
        h.key(KeyCode::Enter);
        assert_eq!(
            h.call(Operation::GetProposal {
                proposal_id: proposal
            })
            .unwrap()["status"],
            "rejected"
        );
        assert!(h.app.console().editor.text().is_empty());
    }
}

#[test]
fn a_new_session_cannot_read_or_cancel_another_sessions_jobs() {
    let mut h = Harness::new("sessions");
    h.authorize();
    let own = h.own();
    let job = h.query(&own, "SELECT 17");
    h.wait_job(&job);
    let first = h.client.take().unwrap();
    h.client = Some(
        h.runtime
            .block_on(Client::connect(&h.instance, "second client"))
            .unwrap(),
    );
    assert_eq!(
        h.call(Operation::GetJob {
            job_id: job.clone()
        })
        .unwrap_err()
        .code,
        "access_required"
    );
    let requested = h.call(Operation::RequestAccess {}).unwrap();
    // The first session is authorized and the second is pending. Select its row by its reported label.
    h.key(KeyCode::F(8));
    h.key(KeyCode::Enter);
    h.key(KeyCode::Right);
    h.key(KeyCode::Enter);
    assert_eq!(
        h.call(Operation::GetAccess {}).unwrap()["status"],
        "authorized",
        "{requested}"
    );
    assert_eq!(
        h.call(Operation::GetJob {
            job_id: job.clone()
        })
        .unwrap_err()
        .code,
        "invalid_target"
    );
    assert_eq!(
        h.call(Operation::CancelJob { job_id: job })
            .unwrap_err()
            .code,
        "invalid_target"
    );
    drop(first);
}

#[test]
fn network_connectors_apply_and_clean_the_agent_read_only_context() {
    for variable in [
        "SQLN_TEST_POSTGRES",
        "SQLN_TEST_MYSQL",
        "SQLN_TEST_REDIS",
        "SQLN_TEST_VALKEY",
    ] {
        let Ok(url) = std::env::var(variable) else {
            eprintln!("SKIP {variable}: database integration environment is absent");
            continue;
        };
        let sqln::connector::target::OpenTarget::Network(target) =
            sqln::connector::target::open_target(&url).unwrap()
        else {
            panic!("network target expected")
        };
        let mut options = Options {
            connections: vec![sqln::connection::Definition::new(
                variable,
                sqln::connection::Source::Network(target),
            )],
            ..Default::default()
        };
        options.settings.max_connections = 1;
        let mut h = Harness::options(options, KeyboardMode::Conventional);
        h.authorize();
        let own = h.own();
        let dialect = h.app.connections()[0].dialect;
        let read = match dialect {
            Dialect::Postgres => "SHOW default_transaction_read_only",
            Dialect::MySql => "SELECT @@transaction_read_only",
            Dialect::Redis => "GET sqln_agent_missing",
            _ => unreachable!(),
        };
        let job = h.query(&own, read);
        let completed = h.wait_job(&job);
        assert_eq!(completed["status"], "completed", "{variable}: {completed}");
        if dialect != Dialect::Redis {
            let console = h
                .app
                .consoles()
                .iter()
                .find(|console| console.id.to_string() == own)
                .unwrap();
            let value = console.results[0].table.rows[0].text(0).unwrap();
            assert!(matches!(value.as_str(), "on" | "1"), "{variable}: {value}");
            h.user_sql(read);
            let value = h.app.console().results[0].table.rows[0].text(0).unwrap();
            assert!(
                matches!(value.as_str(), "off" | "0"),
                "{variable}: leaked read-only context: {value}"
            );
            let table = format!("sqln_agent_{}", uuid::Uuid::new_v4().simple());
            h.user_sql(&format!("CREATE TABLE {table}(id INT);"));
            h.user_sql(&format!("INSERT INTO {table} VALUES (1);"));
            let denied = h.query(&own, &format!("DELETE FROM {table}"));
            assert_eq!(h.wait_job(&denied)["status"], "failed");
            let job = h.query(&own, &format!("SELECT count(*) FROM {table}"));
            assert_eq!(h.wait_job(&job)["status"], "completed");
            assert_eq!(
                h.app
                    .consoles()
                    .iter()
                    .find(|console| console.id.to_string() == own)
                    .unwrap()
                    .results[0]
                    .table
                    .rows[0]
                    .text(0)
                    .as_deref(),
                Some("1")
            );
            h.user_sql(&format!("DROP TABLE {table};"));
        } else {
            let key = format!("sqln_agent_{}", uuid::Uuid::new_v4().simple());
            h.user_sql(&format!("SET {key} user_value"));
            let job = h.query(&own, &format!("GET {key}"));
            assert_eq!(h.wait_job(&job)["status"], "completed");
            let denied = h.query(&own, &format!("SET {key} agent_value"));
            assert_eq!(h.wait_job(&denied)["status"], "failed");
            h.user_sql(&format!("DEL {key}"));
        }
    }
}

#[test]
fn the_policy_rejects_session_changes_hidden_writes_and_unknown_calls() {
    let bad = [
        "DELETE FROM things",
        "SELECT nextval('x')",
        "WITH changed AS (DELETE FROM things RETURNING *) SELECT * FROM changed",
        "SELECT * INTO new_table FROM things",
        "SELECT * FROM things FOR UPDATE",
        "SET default_transaction_read_only=off",
        "SELECT 1; DELETE FROM things",
        "SELECT * FROM evil()",
        "SELECT load_extension('x')",
        "SELECT 1 /*! INTO OUTFILE '/tmp/evil' */",
    ];
    for dialect in [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite] {
        for sql in bad {
            assert!(
                agent::policy::validate(&[sql.into()], dialect).is_err(),
                "{dialect:?}: {sql}"
            );
        }
    }
    for dialect in [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite] {
        for sql in [
            "SELECT 1",
            "SELECT count(*) FROM things",
            "WITH x AS (SELECT id FROM things) SELECT * FROM x",
            "SELECT :name",
        ] {
            assert!(
                agent::policy::validate(&[sql.into()], dialect).is_ok(),
                "{dialect:?}: {sql}"
            );
        }
    }
    for command in ["SET a b", "MULTI", "EXEC", "SELECT 2", "EVAL_RO x 0"] {
        assert!(
            agent::policy::validate(&[command.into()], Dialect::Redis).is_err(),
            "{command}"
        );
    }
}

#[test]
fn character_edits_preserve_cursor_and_undo_in_both_modes() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut f = common::Fixture::new("猫abc\nSELECT 1;", mode);
        f.editor.set_cursor(edtui::Index2::new(1, 3));
        let revision = f.editor.revision();
        f.editor
            .apply_agent_edits(
                revision,
                &[Edit {
                    start: Position { line: 0, column: 1 },
                    end: Position { line: 0, column: 2 },
                    text: "XYZ".into(),
                }],
            )
            .unwrap();
        assert_eq!(f.editor.text(), "猫XYZbc\nSELECT 1;");
        assert_eq!(f.editor.cursor(), edtui::Index2::new(1, 3));
        f.editor.on_command(Action::EditorUndo, None);
        assert_eq!(f.editor.text(), "猫abc\nSELECT 1;");
    }
}
