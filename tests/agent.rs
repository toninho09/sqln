//! Assistant behavior through the private IPC, input events, and the existing executor.

use crossterm::event::{Event, KeyCode};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};
use sqln::agent::ipc::{Client, Instance};
use sqln::agent::{self, Edit, Operation, Position, Reply};
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::connection::Definition;
use sqln::connector::sqlite::SqliteTarget;
use sqln::editor::KeyboardMode;
use sqln::input::{Action, Keymap};
use sqln::sql::Dialect;
use sqln::store::config::{AgentAccess, AgentDefault, AgentSettings};
use sqln::ui::theme::Theme;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

fn seeded_db(name: &str) -> PathBuf {
    let path = crate::common::temp_db(&format!("agent-{name}"));
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE things(id INTEGER PRIMARY KEY, name TEXT, data BLOB); INSERT INTO things VALUES(1, 'one', X'00ff'),(2, 'two', NULL);").unwrap();
    path
}

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
        let path = seeded_db(name);
        Self::options(
            Options {
                database: Some(path),
                keyboard: mode,
                ..Default::default()
            },
            mode,
        )
    }
    /// Opens SQLN with one SQLite connection that `edit` changes, such as a production mark.
    fn defined(name: &str, edit: impl FnOnce(&mut Definition)) -> Self {
        let mut definition = Definition::sqlite(SqliteTarget::new(seeded_db(name)));
        edit(&mut definition);
        Self::options(
            Options {
                connections: vec![definition],
                ..Default::default()
            },
            KeyboardMode::Conventional,
        )
    }
    fn options(mut options: Options, mode: KeyboardMode) -> Self {
        let overrides = HashMap::from([
            ("agentSessions".into(), vec!["f8".into()]),
            ("agentReview".into(), vec!["f9".into()]),
            ("agentDirect".into(), vec!["f10".into()]),
            ("agentTasks".into(), vec!["f11".into()]),
            ("agentSession".into(), vec!["f12".into()]),
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
        self.app.on_event(Event::Key(crate::common::key(code)));
    }
    /// Sends a tool request with the same arguments as an MCP client.
    fn tool(&mut self, request: Value) -> Reply {
        self.call(op(request))
    }
    fn access(&mut self) -> Value {
        self.tool(json!({"method":"request_access"})).unwrap()
    }
    fn authorize(&mut self) {
        assert_eq!(self.access()["status"], "pending");
        self.key(KeyCode::F(8));
        self.key(KeyCode::Enter);
        self.key(KeyCode::Right);
        self.key(KeyCode::Enter);
        assert_eq!(self.access()["status"], "authorized");
    }
    fn connection(&self) -> String {
        self.app.connections()[0].id.to_string()
    }
    /// Returns the agent console, which the first query opens.
    fn own(&mut self) -> String {
        let job = self.query("SELECT 1");
        self.wait_job(&job)["tab_ids"][0].as_str().unwrap().into()
    }
    fn query(&mut self, sql: &str) -> String {
        let connection = self.connection();
        let reply = self
            .tool(json!({"method":"query","connection_id":connection,"sql":sql}))
            .unwrap();
        reply["job_id"].as_str().unwrap().into()
    }
    fn wait_job(&mut self, id: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let job = self.tool(json!({"method":"job","job_id":id})).unwrap();
            if !WAITING.contains(&job["status"].as_str().unwrap()) {
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
        self.click_in_row(label, "");
    }
    /// Clicks the first `label` in a row that also contains `row`.
    fn click_in_row(&mut self, label: &str, row: &str) {
        let screen = self.draw(80, 24);
        let (x, y) = screen
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains(row))
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
        let reply = self
            .tool(
                json!({"method":"edit_console","console_id":id,"revision":revision,
                "edits":[insert(text)]}),
            )
            .unwrap();
        assert_eq!(reply["kind"], "proposal");
        reply["job_id"].as_str().unwrap().into()
    }
    fn wait_proposal(&mut self, id: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let proposal = self.tool(json!({"method":"job","job_id":id})).unwrap();
            if proposal["status"] != "building" {
                return proposal;
            }
            assert!(Instant::now() < deadline);
        }
    }
    fn job_status(&mut self, id: &str) -> Value {
        self.tool(json!({"method":"job","job_id":id})).unwrap()["status"].clone()
    }
}
/// States that the bridge waits through.
const WAITING: [&str; 8] = [
    "pending",
    "building",
    "validating",
    "ready",
    "waiting_credentials",
    "waiting_confirmation",
    "running",
    "building_artifact",
];
fn op(value: Value) -> Operation {
    serde_json::from_value(value).unwrap()
}
/// An edit that inserts text at the start of the console.
fn insert(text: &str) -> Value {
    json!({"start":{"line":0,"column":0},"end":{"line":0,"column":0},"text":text})
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
        h.access();
        assert_eq!(h.app.console().id, active);
        assert_eq!(h.app.console().editor.cursor(), cursor);
        h.key(KeyCode::Char('x'));
        let screen = h.draw(80, 24);
        assert!(screen.contains("MCP  1 access request"), "{screen}");
        assert!(screen.contains("F8: sessions"), "{screen}");
        // The card shows the request, and the access dialog stays closed.
        assert!(screen.contains("asks for access to SQLN."), "{screen}");
        assert!(screen.contains("[Later]"), "{screen}");
        assert!(!screen.contains("[Allow]"), "{screen}");
        assert_eq!(h.app.console().editor.text(), "x");
        assert_eq!(h.app.console().id, active);
        h.app.on_event(Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('s'),
            crossterm::event::KeyModifiers::CONTROL,
        )));
        assert!(h.draw(80, 24).contains("MCP  1 access request"));
        h.app.on_event(Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('w'),
            crossterm::event::KeyModifiers::CONTROL,
        )));
        h.click_label("[Review]");
        let screen = h.draw(80, 24);
        assert!(screen.contains("MCP access request"), "{screen}");
        assert!(screen.contains("[Allow all]"), "{screen}");
        assert!(screen.contains("[Full access]"), "{screen}");
        h.key(KeyCode::Char('x'));
        assert_eq!(h.app.console().editor.text(), "x");
        assert_eq!(
            h.tool(json!({"method":"get_context"})).unwrap_err().code,
            "access_required"
        );
        h.key(KeyCode::Right);
        h.key(KeyCode::Enter);
        assert_eq!(h.access()["status"], "authorized");
        assert_eq!(h.access()["access"], "propose");
        h.key(KeyCode::Char('x'));
        assert_eq!(h.app.console().editor.text(), "xx");
        let screen = h.draw(80, 24);
        assert!(!screen.contains("access request"), "{screen}");
        assert!(screen.contains("MCP 1"), "{screen}");
        assert!(!screen.contains("MCP 1 all"), "{screen}");
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
        h.access();
        h.key(KeyCode::Esc);
        h.draw(120, 40);
        let screen = h.draw(80, 24);
        assert!(screen.contains("MCP  1 access request"), "{screen}");
        let label = sqln::ui::text::key_label(shortcut);
        assert!(
            screen.contains(&format!("{label} > Agent sessions")),
            "{screen}"
        );
        h.click_label("[Review]");
        assert!(h.draw(80, 24).contains("MCP access request"));
        h.key(KeyCode::Enter);
        assert_eq!(h.access()["status"], "denied");
        assert!(!h.draw(80, 24).contains("access request"));
    }
}

#[test]
fn pending_access_notification_counts_clients_and_clears_on_disconnect() {
    let mut h = Harness::new("access-notice-clients");
    h.access();
    let first = h.client.take();
    h.client = Some(
        h.runtime
            .block_on(Client::connect(&h.instance, "second client"))
            .unwrap(),
    );
    h.access();
    let second = h.client.replace(first.unwrap()).unwrap();
    assert!(h.draw(80, 24).contains("MCP  2 access requests"));
    drop(second);
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.draw(80, 24).contains("MCP  2 access requests") {
        h.app.drain_events();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(h.draw(80, 24).contains("MCP  1 access request"));
    h.client.take();
    while h.draw(80, 24).contains("access request") {
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
        h.tool(json!({"method":"get_context","limit":100}))
            .unwrap_err()
            .code,
        "access_required"
    );
    let active = h.app.console().id;
    h.authorize();
    let context = h.tool(json!({"method":"get_context"})).unwrap();
    assert_eq!(context["access"], "propose");
    assert_eq!(context["database_access"], "read_only");
    assert_eq!(context["connections"][0]["connection_id"], h.connection());
    assert_eq!(context["tabs"][0]["console_id"], active.to_string());
    let own = h.own();
    let connection = h.connection();
    let same = h
        .tool(json!({"method":"query","connection_id":connection,"schema":"main","sql":"SELECT 2"}))
        .unwrap();
    assert_eq!(same["tab_ids"][0], own);
    h.wait_job(same["job_id"].as_str().unwrap());
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
            .starts_with("MCP · ")
    );
    let found = h
        .runtime
        .block_on(agent::ipc::discover(&h.directory))
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].instance_id, h.instance.instance_id);
    let job = h.query("SELECT id, name, data FROM things ORDER BY id");
    let reply = h.wait_job(&job);
    assert_eq!(reply["status"], "completed");
    assert_eq!(h.app.console().id, active);
    // The task reply holds the first rows, so the agent needs no other read.
    assert_eq!(reply["results"][0]["rows"][1]["cells"][1]["value"], "two");
    assert_eq!(reply["statements"][0]["status"], "success");
    let result = h
        .tool(json!({"method":"read_result","tab_id":own}))
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
    h.app
        .on_event(Event::Paste("SELECT name FROM things ORDER BY id;".into()));
    let revision = h.app.console().editor.revision();
    let job = h
        .tool(json!({"method":"run_console","console_id":id,"revision":revision}))
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
        h.tool(json!({"method":"run_console","console_id":id,"revision":revision + 1}))
            .unwrap_err()
            .code,
        "revision_conflict"
    );
    let own = h.own();
    let job = h.query("SELECT 1; DELETE FROM things;");
    assert_eq!(h.wait_job(&job)["status"], "failed");
    let job = h.query("SELECT count(*) FROM things");
    assert_eq!(h.wait_job(&job)["status"], "completed");
    let page = h
        .tool(json!({"method":"read_result","tab_id":own,"limit":10}))
        .unwrap();
    assert_eq!(page["rows"][0]["cells"][0]["value"], "2");
    // The validation worker splits the raw text into one result per statement.
    let job = h.query("SELECT 1; SELECT 2;");
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
    // The change shows in the editor, with the bar of the review.
    let screen = h.draw(80, 24);
    assert!(screen.contains("+ SELECT '猫"), "{screen}");
    assert!(screen.contains("1/1 change"), "{screen}");
    assert!(screen.contains("[a Accept]"), "{screen}");
    assert!(screen.contains("± console-1"), "{screen}");
    // Enter decides nothing.
    h.key(KeyCode::Enter);
    assert!(h.app.console().editor.text().is_empty());
    h.key(KeyCode::Char('a'));
    assert_eq!(h.app.console().editor.text(), "SELECT '猫';");
    let applied = h.wait_proposal(&proposal);
    assert_eq!(applied["status"], "applied");
    assert_eq!(applied["accepted_changes"], 1);
    assert_eq!(applied["rejected_changes"], 0);
    assert!(!h.draw(80, 24).contains("± console-1"));
    assert_eq!(
        applied["console_revision"],
        h.app.console().editor.revision()
    );
    h.app.on_event(Event::Key(crate::common::ctrl('z')));
    assert!(h.app.console().editor.text().is_empty());
    let console = h.app.console().id.to_string();
    let revision = h.app.console().editor.revision();
    let direct = json!({"method":"edit_console","console_id":console,"revision":revision,
        "edits":[insert("SELECT 2;")],"mode":"direct"});
    assert_eq!(
        h.tool(direct.clone()).unwrap_err().code,
        "direct_edit_not_allowed"
    );
    h.key(KeyCode::F(10));
    h.key(KeyCode::Right);
    h.key(KeyCode::Enter);
    assert_eq!(h.tool(direct).unwrap()["status"], "applied");
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
    assert_eq!(h.job_status(&proposal), "revision_conflict");
    assert_eq!(h.app.console().editor.text(), "user text");
    assert!(h.draw(120, 24).contains("Agent edit discarded"));
}

#[test]
fn disconnect_cancels_its_query_and_not_the_next_user_query() {
    let mut h = Harness::new("disconnect");
    h.authorize();
    let own = h.own();
    let _job=h.query("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<1000000000) SELECT count(*) FROM n");
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
    let catalog = h
        .tool(json!({"method":"describe","connection_id":connection}))
        .unwrap();
    let catalog = if catalog["job_id"].is_string() {
        let job = h.wait_job(catalog["job_id"].as_str().unwrap());
        serde_json::from_str(job["artifact"]["text"].as_str().unwrap()).unwrap()
    } else {
        catalog
    };
    assert!(catalog.to_string().contains("things"), "{catalog}");
    assert_eq!(
        h.tool(json!({"method":"describe","connection_id":connection,
            "object":{"name":"things","kind":"table"}}))
            .unwrap_err()
            .code,
        "schema_required"
    );
    let reply = h
        .tool(
            json!({"method":"describe","connection_id":connection,"schema":"main",
            "object":{"name":"things","kind":"table"}}),
        )
        .unwrap();
    let ddl = reply["job_id"].as_str().unwrap().to_owned();
    let tab = reply["tab_ids"][0].as_str().unwrap().to_owned();
    let done = h.wait_job(&ddl);
    assert_eq!(done["status"], "completed");
    // The DDL reply holds the text, not the internal catalog rows.
    assert!(
        done["artifact"]["text"]
            .as_str()
            .unwrap()
            .contains("CREATE TABLE")
    );
    assert!(done["results"].as_array().unwrap().is_empty());
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
    let artifact = h
        .tool(json!({"method":"job","job_id":ddl,"artifact_offset":0}))
        .unwrap();
    assert!(
        artifact["artifact"]["text"]
            .as_str()
            .unwrap()
            .contains("CREATE TABLE")
    );
    let diagram = h
        .tool(json!({"method":"describe","connection_id":connection,"schema":"main","er_diagram":true}))
        .unwrap()["job_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let done = h.wait_job(&diagram);
    assert_eq!(done["status"], "completed");
    assert!(
        done["artifact"]["text"]
            .as_str()
            .unwrap()
            .contains("erDiagram")
    );
    let side = json!({"connection_id":connection,"schema":"main","table":"things"});
    for kind in ["structure", "data"] {
        let job = h
            .tool(json!({"method":"compare","kind":kind,"source":side,"target":side}))
            .unwrap()["job_id"]
            .as_str()
            .unwrap()
            .to_owned();
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
        h.tool(json!({"method":"run_console","console_id":console,"revision":revision}))
            .unwrap_err()
            .code,
        "transaction_active"
    );
    let job = h.query("SELECT 8");
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
        let revision = h.app.console().editor.revision();
        assert_eq!(
            h.tool(json!({"method":"run_console","console_id":console,"revision":revision}))
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
    let job = h.query("SELECT id, name, data FROM things ORDER BY id");
    let completed = h.wait_job(&job);
    assert_eq!(completed["status"], "completed");
    let generation = completed["results"][0]["generation"]
        .as_str()
        .unwrap()
        .to_owned();
    let first = h
        .tool(json!({"method":"read_result","tab_id":own,"limit":1,"generation":generation}))
        .unwrap();
    assert_eq!(first["next_offset"], 1);
    let second = h
        .tool(json!({"method":"read_result","tab_id":own,"offset":1,"limit":1,"generation":generation}))
        .unwrap();
    assert_eq!(second["rows"][0]["cells"][1]["value"], "two");
    let job = h.query("SELECT 'replacement'");
    h.wait_job(&job);
    assert_eq!(
        h.tool(json!({"method":"read_result","tab_id":own,"limit":1,"generation":generation}))
            .unwrap_err()
            .code,
        "result_changed"
    );
    let text = "猫\u{0}\n".repeat(10000);
    let connection = h.connection();
    let job = h
        .tool(
            json!({"method":"query","connection_id":connection,"sql":"SELECT :value",
            "parameters":[{"name":"value","kind":"text","value":text}]}),
        )
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
    // A large cell stays a reference in the task reply.
    assert_eq!(
        completed["results"][0]["rows"][0]["cells"][0]["needs_cell_read"],
        true
    );
    let mut actual = String::new();
    let mut offset = 0;
    loop {
        let page = h
            .tool(
                json!({"method":"read_result","tab_id":own,"row":0,"column":0,
                "generation":generation,"offset":offset}),
            )
            .unwrap();
        actual.push_str(page["text"].as_str().unwrap());
        match page["next_offset"].as_u64() {
            Some(next) => offset = next as usize,
            None => break,
        }
    }
    assert_eq!(actual, text);
    h.user_sql("INSERT INTO things(id,name) VALUES(3,'user write after agent read');");
    let job = h.query("SELECT count(*) FROM things");
    h.wait_job(&job);
    assert_eq!(
        h.tool(json!({"method":"read_result","tab_id":own,"limit":1}))
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
            line.find("[a Accept]")
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
    assert_eq!(h.job_status(&proposal), "applied");
}

#[test]
fn revocation_cancels_the_sessions_work_and_blocks_further_reads() {
    let mut h = Harness::new("revoke");
    h.authorize();
    let own = h.own();
    h.query("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<1000000000) SELECT count(*) FROM n");
    h.key(KeyCode::F(8));
    h.key(KeyCode::Enter);
    // Revoke is the last option, after the other access levels.
    h.key(KeyCode::Left);
    h.key(KeyCode::Enter);
    assert_eq!(h.access()["status"], "revoked");
    assert_eq!(
        h.tool(json!({"method":"get_context"})).unwrap_err().code,
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
    assert!(h.draw(80, 24).contains("MCP off"));
}

#[test]
fn both_editor_modes_review_proposals_in_the_editor_and_put_them_off() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut h = Harness::mode(&format!("review-{mode:?}"), mode);
        h.authorize();
        let proposal = h.proposal("SELECT 11;");
        h.wait_proposal(&proposal);
        h.key(KeyCode::F(9));
        h.key(KeyCode::Enter);
        assert!(h.draw(80, 24).contains("[r Reject]"));
        h.draw(100, 30);
        assert!(h.draw(80, 24).contains("[r Reject]"));
        // Keys that edit text do not change it. Esc puts the proposal off.
        h.key(KeyCode::Char('x'));
        h.key(KeyCode::Backspace);
        assert!(h.app.console().editor.text().is_empty());
        assert!(h.draw(80, 24).contains("read only during the review"));
        h.key(KeyCode::Esc);
        assert_eq!(h.job_status(&proposal), "pending");
        assert!(h.app.console().editor.review().is_none());
        assert!(h.draw(80, 24).contains("Edit proposal"));
        h.key(KeyCode::F(9));
        h.key(KeyCode::Enter);
        h.key(KeyCode::Char('r'));
        assert_eq!(h.job_status(&proposal), "rejected");
        assert!(h.app.console().editor.text().is_empty());
    }
}

#[test]
fn a_new_session_cannot_read_or_cancel_another_sessions_jobs() {
    let mut h = Harness::new("sessions");
    h.authorize();
    let job = h.query("SELECT 17");
    h.wait_job(&job);
    let first = h.client.take().unwrap();
    h.client = Some(
        h.runtime
            .block_on(Client::connect(&h.instance, "second client"))
            .unwrap(),
    );
    assert_eq!(
        h.tool(json!({"method":"job","job_id":job}))
            .unwrap_err()
            .code,
        "access_required"
    );
    let requested = h.access();
    // The first session is authorized and the second is pending. Select its row by its reported label.
    h.key(KeyCode::F(8));
    h.key(KeyCode::Enter);
    h.key(KeyCode::Right);
    h.key(KeyCode::Enter);
    assert_eq!(h.access()["status"], "authorized", "{requested}");
    assert_eq!(
        h.tool(json!({"method":"job","job_id":job}))
            .unwrap_err()
            .code,
        "invalid_target"
    );
    assert_eq!(
        h.tool(json!({"method":"job","job_id":job,"cancel":true}))
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
        let job = h.query(read);
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
            let denied = h.query(&format!("DELETE FROM {table}"));
            assert_eq!(h.wait_job(&denied)["status"], "failed");
            let job = h.query(&format!("SELECT count(*) FROM {table}"));
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
            let job = h.query(&format!("GET {key}"));
            assert_eq!(h.wait_job(&job)["status"], "completed");
            let denied = h.query(&format!("SET {key} agent_value"));
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
        let mut f = crate::common::Fixture::new("猫abc\nSELECT 1;", mode);
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

impl Harness {
    /// Allows the session with **Allow all**.
    fn authorize_all(&mut self) {
        self.grant(2);
        assert_eq!(self.access()["access"], "direct");
    }
    /// Answers the access request with the option at `index`: 1 Allow, 2 Allow all, 3 Full access.
    fn grant(&mut self, index: usize) {
        self.access();
        self.click_label("[Review]");
        for _ in 0..index {
            self.key(KeyCode::Right);
        }
        self.key(KeyCode::Enter);
        assert_eq!(self.access()["status"], "authorized");
    }
    fn direct(&mut self, text: &str) -> Reply {
        let console = self.app.console().id.to_string();
        let revision = self.app.console().editor.revision();
        self.tool(
            json!({"method":"edit_console","console_id":console,"revision":revision,
            "edits":[insert(text)],"mode":"direct"}),
        )
    }
    fn wait_busy(&mut self, console: &str, busy: bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while h_busy(&self.app, console) != busy {
            self.app.drain_events();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
fn h_busy(app: &App, console: &str) -> bool {
    app.consoles()
        .iter()
        .find(|c| c.id.to_string() == console)
        .is_some_and(|c| c.is_busy())
}

#[test]
fn allow_all_edits_directly_with_a_notice_undo_and_a_way_back() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut h = Harness::mode("allow-all", mode);
        h.authorize_all();
        let screen = h.draw(80, 24);
        assert!(screen.contains("MCP 1 all"), "{screen}");
        assert_eq!(h.direct("SELECT 1;").unwrap()["status"], "applied");
        assert_eq!(h.app.console().editor.text(), "SELECT 1;");
        let screen = h.draw(120, 24);
        assert!(screen.contains("edited"), "{screen}");
        assert!(screen.contains("undoes it"), "{screen}");
        assert!(h.app.console().results.is_empty());
        if mode == KeyboardMode::Vim {
            h.key(KeyCode::Char('u'));
        } else {
            h.app.on_event(Event::Key(crate::common::ctrl('z')));
        }
        assert!(h.app.console().editor.text().is_empty());
        h.key(KeyCode::F(8));
        h.key(KeyCode::Enter);
        let screen = h.draw(80, 24);
        assert!(screen.contains("[Keep access]"), "{screen}");
        assert!(screen.contains("[Allow]"), "{screen}");
        assert!(screen.contains("[Full access]"), "{screen}");
        assert!(screen.contains("[Revoke]"), "{screen}");
        assert!(!screen.contains("[Allow all]"), "{screen}");
        h.key(KeyCode::Right);
        h.key(KeyCode::Enter);
        assert_eq!(
            h.direct("SELECT 2;").unwrap_err().code,
            "direct_edit_not_allowed"
        );
        let screen = h.draw(80, 24);
        assert!(screen.contains("MCP 1"), "{screen}");
        assert!(!screen.contains("MCP 1 all"), "{screen}");
        // The same command raises the access again.
        h.key(KeyCode::F(8));
        h.key(KeyCode::Enter);
        let screen = h.draw(80, 24);
        assert!(screen.contains("[Allow all]"), "{screen}");
        h.key(KeyCode::Right);
        h.key(KeyCode::Enter);
        assert_eq!(h.access()["access"], "direct");
        assert_eq!(h.direct("SELECT 3;").unwrap()["status"], "applied");
        assert!(h.draw(80, 24).contains("MCP 1 all"));
    }
}

#[test]
fn the_stop_button_cancels_every_agent_task_and_keeps_access() {
    let mut h = Harness::new("stop-all");
    h.authorize();
    let own = h.own();
    // Each join doubles the rows. The read policy accepts a plain join.
    let joins = (0..40)
        .map(|index| format!("things t{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let job = h.query(&format!("SELECT count(*) FROM {joins}"));
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.job_status(&job) != "running" {
        assert!(Instant::now() < deadline);
    }
    let screen = h.draw(80, 24);
    assert!(screen.contains("1 task running"), "{screen}");
    assert!(!screen.contains("[Review]"), "{screen}");
    h.click_in_row("[Stop]", " MCP ");
    let reply = h.wait_job(&job);
    assert_eq!(reply["status"], "cancelled", "{reply}");
    assert_eq!(reply["message"], sqln::ui::text::AGENT_STOP_REASON);
    assert!(h.draw(120, 24).contains("1 agent task stopped"));
    assert_eq!(h.access()["status"], "authorized");
    h.wait_busy(&own, false);
    let screen = h.draw(80, 24);
    assert!(
        !screen
            .lines()
            .any(|line| line.contains(" MCP ") && line.contains("[Stop]")),
        "{screen}"
    );
    let job = h.query("SELECT 1");
    assert_eq!(h.wait_job(&job)["status"], "completed");
}

#[test]
fn access_requests_open_one_after_another_with_their_place() {
    let mut h = Harness::new("access-queue");
    h.access();
    let first = h.client.take();
    h.client = Some(
        h.runtime
            .block_on(Client::connect(&h.instance, "second client"))
            .unwrap(),
    );
    h.access();
    h.click_label("[Review]");
    let screen = h.draw(80, 24);
    assert!(screen.contains("MCP access request (1 of 2)"), "{screen}");
    assert!(screen.contains("test agent"), "{screen}");
    assert!(screen.contains("reported, not verified"), "{screen}");
    // Enter keeps the initial choice, Reject. The next request opens.
    h.key(KeyCode::Enter);
    let screen = h.draw(80, 24);
    assert!(screen.contains("MCP access request"), "{screen}");
    assert!(!screen.contains("(1 of 2)"), "{screen}");
    assert!(screen.contains("second client"), "{screen}");
    h.key(KeyCode::Right);
    h.key(KeyCode::Enter);
    assert_eq!(h.access()["status"], "authorized");
    h.client = first;
    assert_eq!(h.access()["status"], "denied");
}

#[test]
fn the_agent_console_shows_messages_and_results_without_an_editor() {
    let mut h = Harness::new("agent-tab");
    h.authorize();
    let own = h.own();
    let job = h.query("SELECT name FROM things ORDER BY id");
    assert_eq!(h.wait_job(&job)["status"], "completed");
    h.click_label("MCP · test agent");
    assert_eq!(h.app.console().id.to_string(), own);
    let screen = h.draw(80, 24);
    assert!(screen.contains("Messages"), "{screen}");
    assert!(!screen.contains("[Run]"), "{screen}");
    h.key(KeyCode::Char('x'));
    h.app.on_event(Event::Paste("SELECT 2".into()));
    assert!(h.app.console().editor.text().is_empty());
    assert!(h.draw(120, 24).contains(sqln::ui::text::AGENT_NO_EDITOR),);
    for size in [(80, 24), (120, 40)] {
        let screen = h.draw(size.0, size.1);
        assert!(screen.contains("Messages"), "{screen}");
    }
}

#[test]
fn the_agent_places_sql_in_its_tab_for_the_user_to_review_and_run() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut h = Harness::mode(&format!("agent-sql-{mode:?}"), mode);
        h.authorize();
        let own = h.own();
        let active = h.app.console().id;
        let tabs = h.app.consoles().len();
        let connection = h.connection();
        let place = |h: &mut Harness, sql: &str| {
            h.tool(json!({"method":"edit_console","connection_id":connection,"sql":sql}))
                .unwrap()
        };
        let reply = place(&mut h, "CREATE TABLE made(id INTEGER);\r\n");
        assert_eq!(reply["status"], "placed");
        assert_eq!(reply["console_id"], own);
        // No new tab opens, and the focus stays.
        assert_eq!(h.app.consoles().len(), tabs);
        assert_eq!(h.app.console().id, active);
        let tab = |h: &Harness| {
            h.app
                .consoles()
                .iter()
                .find(|console| console.id.to_string() == own)
                .unwrap()
                .editor
                .text()
        };
        assert_eq!(tab(&h), "CREATE TABLE made(id INTEGER);\n");
        let screen = h.draw(120, 24);
        assert!(screen.contains("placed SQL in its tab"), "{screen}");
        assert!(screen.contains("SQL · MCP · test agent"), "{screen}");
        // Without full access, the agent cannot run the write that it placed.
        let revision = reply["revision"].as_u64().unwrap();
        let job = h
            .tool(json!({"method":"run_console","console_id":own,"revision":revision}))
            .unwrap()["job_id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(h.wait_job(&job)["status"], "failed");
        // The user opens the tab: the mark goes, and the bar offers the SQL commands.
        h.click_label("SQL · MCP");
        assert_eq!(h.app.console().id.to_string(), own);
        let screen = h.draw(80, 24);
        assert!(!screen.contains("SQL · MCP"), "{screen}");
        assert!(screen.contains("[New console]"), "{screen}");
        assert!(screen.contains("[Clear]"), "{screen}");
        assert!(!screen.contains("[Auto]"), "{screen}");
        assert!(screen.contains("CREATE TABLE made"), "{screen}");
        // New SQL replaces SQL that the user did not change.
        place(&mut h, "CREATE TABLE made(id INTEGER, name TEXT);");
        assert_eq!(tab(&h), "CREATE TABLE made(id INTEGER, name TEXT);");
        // SQL that the user changed stays, and the new SQL waits for a review.
        h.app.on_event(Event::Paste(" ".into()));
        let changed = tab(&h);
        assert_ne!(changed, "CREATE TABLE made(id INTEGER, name TEXT);");
        let reply = place(&mut h, "CREATE TABLE made(id INTEGER, note TEXT);");
        assert_eq!(reply["kind"], "proposal");
        let proposal = reply["job_id"].as_str().unwrap().to_owned();
        assert_eq!(h.wait_proposal(&proposal)["status"], "pending");
        assert_eq!(tab(&h), changed);
        h.key(KeyCode::F(9));
        h.key(KeyCode::Enter);
        h.key(KeyCode::Char('A'));
        assert_eq!(tab(&h), "CREATE TABLE made(id INTEGER, note TEXT);");
        assert!(h.draw(120, 24).contains("applied to MCP"));
        // The user runs it. It is a user run, so the write executes.
        h.key(KeyCode::F(5));
        let deadline = Instant::now() + Duration::from_secs(10);
        while h.sql_value("SELECT count(*) FROM sqlite_master WHERE name = 'made'") == 0 {
            h.app.drain_events();
            assert!(
                Instant::now() < deadline,
                "the user run did not create the table"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        h.wait_busy(&own, false);
        // New console copies the SQL to a user console, which takes the focus.
        h.click_label("[New console]");
        assert_ne!(h.app.console().id.to_string(), own);
        assert_eq!(
            h.app.console().editor.text(),
            "CREATE TABLE made(id INTEGER, note TEXT);"
        );
        assert_eq!(h.app.consoles().len(), tabs + 1);
        // Clear empties the agent SQL, and the tab shows its results only.
        h.click_label("MCP · test agent");
        h.click_label("[Clear]");
        assert!(tab(&h).is_empty());
        let screen = h.draw(80, 24);
        assert!(!screen.contains("[New console]"), "{screen}");
        assert!(screen.contains("Agent SQL cleared"), "{screen}");
        assert_eq!(
            h.tool(json!({"method":"edit_console","connection_id":connection,"sql":"  "}))
                .unwrap_err()
                .code,
            "invalid_request"
        );
    }
}

impl Harness {
    fn count(&mut self) -> i64 {
        self.sql_value("SELECT count(*) FROM things")
    }
    /// Reads one number from the SQLite file, outside SQLN.
    fn sql_value(&self, sql: &str) -> i64 {
        let path = match &self.app.connections()[0]
            .definition
            .as_ref()
            .unwrap()
            .source
        {
            sqln::connection::Source::Sqlite(target) => target.path().to_path_buf(),
            sqln::connection::Source::Network(_) => unreachable!(),
        };
        rusqlite::Connection::open(path)
            .unwrap()
            .query_row(sql, [], |row| row.get(0))
            .unwrap()
    }
    /// Waits until the task asks the run question.
    fn wait_question(&mut self, job: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.job_status(job) != "waiting_confirmation" {
            assert!(Instant::now() < deadline, "no run question");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
fn full_access_writes_to_the_database_and_reports_changed_rows() {
    let mut h = Harness::new("full-write");
    h.access();
    h.click_label("[Review]");
    let screen = h.draw(80, 24);
    assert!(screen.contains("[Full access]"), "{screen}");
    assert!(screen.contains("without WHERE"), "{screen}");
    // Esc keeps the safe choice.
    h.key(KeyCode::Esc);
    assert_eq!(h.access()["status"], "denied");

    let mut h = Harness::new("full-write");
    h.grant(3);
    assert_eq!(h.access()["access"], "full");
    let context = h.tool(json!({"method":"get_context"})).unwrap();
    assert_eq!(context["database_access"], "read_write");
    assert!(h.draw(80, 24).contains("MCP 1 full"));
    let job = h.query("INSERT INTO things(id, name) VALUES (3, 'three'), (4, 'four')");
    let reply = h.wait_job(&job);
    assert_eq!(reply["status"], "completed", "{reply}");
    assert_eq!(reply["statements"][0]["changed_rows"], 2, "{reply}");
    assert_eq!(h.count(), 4);
    // A write with WHERE on a connection without the production mark asks nothing.
    let job = h.query("DELETE FROM things WHERE id = 4");
    assert_eq!(h.wait_job(&job)["status"], "completed");
    assert_eq!(h.count(), 3);
    // The user console runs its writes too.
    h.app.on_event(Event::Paste(
        "UPDATE things SET name = 'uno' WHERE id = 1;".into(),
    ));
    let console = h.app.console().id.to_string();
    let revision = h.app.console().editor.revision();
    let job = h
        .tool(json!({"method":"run_console","console_id":console,"revision":revision}))
        .unwrap()["job_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(h.wait_job(&job)["status"], "completed");
    assert!(h.draw(120, 24).contains("MCP test agent"));
    // Switch to proposals returns the database to read-only.
    h.key(KeyCode::F(8));
    h.key(KeyCode::Enter);
    h.key(KeyCode::Right);
    h.key(KeyCode::Enter);
    assert_eq!(h.access()["access"], "propose");
    let job = h.query("DELETE FROM things");
    assert_eq!(h.wait_job(&job)["status"], "failed");
    assert_eq!(h.count(), 3);
}

#[test]
fn full_access_asks_before_a_write_without_where_and_in_production() {
    for production in [false, true] {
        let mut h = Harness::defined(&format!("full-question-{production}"), |definition| {
            definition.production = production;
        });
        h.grant(3);
        let sql = if production {
            "DELETE FROM things WHERE id = 1"
        } else {
            "DELETE FROM things"
        };
        let job = h.query(sql);
        h.wait_question(&job);
        let screen = h.draw(80, 24);
        assert!(screen.contains("asks to run this SQL"), "{screen}");
        // The first option cancels. Enter keeps it.
        h.key(KeyCode::Enter);
        let reply = h.wait_job(&job);
        assert_eq!(reply["status"], "cancelled");
        assert_eq!(reply["message"], sqln::ui::text::AGENT_RUN_REFUSED);
        assert_eq!(h.count(), 2);
        let job = h.query(sql);
        h.wait_question(&job);
        h.key(KeyCode::Right);
        h.key(KeyCode::Enter);
        assert_eq!(h.wait_job(&job)["status"], "completed");
        assert!(h.count() < 2);
    }
}

#[test]
fn full_access_keeps_a_read_only_connection_read_only() {
    let mut h = Harness::defined("full-read-only", |definition| {
        definition.read_only = true;
    });
    h.grant(3);
    let job = h.query("DELETE FROM things");
    let reply = h.wait_job(&job);
    assert_eq!(reply["status"], "failed", "{reply}");
    assert_eq!(h.count(), 2);
}

#[test]
fn an_agent_transaction_never_stays_open() {
    let mut h = Harness::new("full-transaction");
    h.grant(3);
    let job = h.query("BEGIN; INSERT INTO things(id, name) VALUES (5, 'five');");
    let reply = h.wait_job(&job);
    assert_eq!(reply["status"], "failed", "{reply}");
    assert_eq!(reply["message"], sqln::ui::text::AGENT_ROLLED_BACK);
    assert_eq!(h.count(), 2);
    let own = reply["tab_ids"][0].as_str().unwrap().to_owned();
    let console = h
        .app
        .consoles()
        .iter()
        .find(|console| console.id.to_string() == own)
        .unwrap();
    assert_eq!(console.transaction, sqln::connector::TransactionState::None);
    let job = h.query("BEGIN; INSERT INTO things(id, name) VALUES (5, 'five'); COMMIT;");
    assert_eq!(h.wait_job(&job)["status"], "completed");
    assert_eq!(h.count(), 3);
}

#[test]
fn a_reconnected_client_keeps_its_access_until_the_user_forgets_it() {
    let mut h = Harness::new("remembered");
    h.grant(3);
    h.client = Some(
        h.runtime
            .block_on(Client::connect(&h.instance, "test agent\u{1b}[31m"))
            .unwrap(),
    );
    let reply = h.access();
    assert_eq!(reply["status"], "authorized");
    assert_eq!(reply["access"], "full");
    let screen = h.draw(120, 24);
    assert!(screen.contains("remembered access"), "{screen}");
    assert!(!screen.contains("MCP access request"), "{screen}");
    // Another client still asks.
    let first = h.client.take();
    h.client = Some(
        h.runtime
            .block_on(Client::connect(&h.instance, "other client"))
            .unwrap(),
    );
    assert_eq!(h.access()["status"], "pending");
    h.client.take();
    h.client = first;
    // After a disconnect, the session list keeps the remembered client with Forget.
    h.client.take();
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.draw(80, 24).contains("MCP 1 full") {
        h.app.drain_events();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    h.key(KeyCode::F(8));
    let screen = h.draw(80, 24);
    assert!(screen.contains("remembered"), "{screen}");
    h.key(KeyCode::Down);
    h.key(KeyCode::Enter);
    let screen = h.draw(80, 24);
    assert!(screen.contains("[Forget]"), "{screen}");
    h.key(KeyCode::Right);
    h.key(KeyCode::Enter);
    h.client = Some(
        h.runtime
            .block_on(Client::connect(&h.instance, "test agent\u{1b}[31m"))
            .unwrap(),
    );
    assert_eq!(h.access()["status"], "pending");
}

#[test]
fn a_task_reply_stays_within_the_response_limit() {
    let mut h = Harness::new("reply-limit");
    // The read policy does not know printf, so this test uses full access.
    h.grant(3);
    // Three results of 1,000 rows with 1 KiB each are larger than one reply.
    let sql = "WITH RECURSIVE n(value) AS (SELECT 1 UNION ALL SELECT value + 1 FROM n WHERE value < 1000) \
         SELECT value, printf('%.1000c', 'x') FROM n";
    let connection = h.connection();
    let job = h
        .tool(
            json!({"method":"query","connection_id":connection,"rows":1000,
            "sql":format!("{sql}; {sql}; {sql};")}),
        )
        .unwrap()["job_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let reply = h.wait_job(&job);
    assert_eq!(reply["status"], "completed", "{reply}");
    assert!(serde_json::to_vec(&reply).unwrap().len() < agent::PAGE_BYTES);
    let results = reply["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    assert!(
        results[0]["next_offset"].is_u64(),
        "{}",
        results[0]["next_offset"]
    );
    assert_eq!(results[2]["rows_omitted"], true);
    // rows: 0 returns only the references.
    let job = h
        .tool(json!({"method":"query","connection_id":connection,"rows":0,"sql":"SELECT 1"}))
        .unwrap()["job_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let reply = h.wait_job(&job);
    assert_eq!(reply["results"][0]["rows_omitted"], true);
}

#[test]
fn network_full_access_writes_and_never_leaves_a_transaction() {
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
            connections: vec![Definition::new(
                variable,
                sqln::connection::Source::Network(target),
            )],
            ..Default::default()
        };
        // One session: each run must clean the context of the previous run.
        options.settings.max_connections = 1;
        let mut h = Harness::options(options, KeyboardMode::Conventional);
        h.grant(3);
        let dialect = h.app.connections()[0].dialect;
        let run = |h: &mut Harness, sql: &str| {
            let job = h.query(sql);
            h.wait_job(&job)
        };
        if dialect == Dialect::Redis {
            let key = format!("sqln_agent_{}", uuid::Uuid::new_v4().simple());
            let reply = run(&mut h, &format!("SET {key} agent_value"));
            assert_eq!(reply["status"], "completed", "{variable}: {reply}");
            let reply = run(&mut h, &format!("GET {key}"));
            assert_eq!(
                reply["results"][0]["rows"][0]["cells"][0]["value"], "agent_value",
                "{variable}: {reply}"
            );
            run(&mut h, &format!("DEL {key}"));
            continue;
        }
        let table = format!("sqln_agent_{}", uuid::Uuid::new_v4().simple());
        let reply = run(&mut h, &format!("CREATE TABLE {table}(id INT)"));
        assert_eq!(reply["status"], "completed", "{variable}: {reply}");
        let reply = run(&mut h, &format!("INSERT INTO {table} VALUES (1)"));
        assert_eq!(
            reply["statements"][0]["changed_rows"], 1,
            "{variable}: {reply}"
        );
        let reply = run(&mut h, &format!("BEGIN; INSERT INTO {table} VALUES (2);"));
        assert_eq!(reply["status"], "failed", "{variable}: {reply}");
        assert_eq!(reply["message"], sqln::ui::text::AGENT_ROLLED_BACK);
        let reply = run(&mut h, &format!("SELECT count(*) FROM {table}"));
        assert_eq!(
            reply["results"][0]["rows"][0]["cells"][0]["value"], "1",
            "{variable}: {reply}"
        );
        let read = match dialect {
            Dialect::Postgres => "SHOW default_transaction_read_only",
            _ => "SELECT @@transaction_read_only",
        };
        let reply = run(&mut h, read);
        let value = reply["results"][0]["rows"][0]["cells"][0]["value"].clone();
        assert!(value == "off" || value == "0", "{variable}: {value}");
        let reply = run(&mut h, &format!("DROP TABLE {table}"));
        assert_eq!(reply["status"], "completed", "{variable}: {reply}");
    }
}

#[test]
fn the_access_card_rings_once_and_later_keeps_only_the_mcp_row() {
    let mut h = Harness::new("access-card");
    h.app.on_event(Event::Paste("SELECT 1;".into()));
    let cursor = h.app.console().editor.cursor();
    assert!(!h.app.take_bell());
    h.access();
    assert!(h.app.take_bell());
    // A second read of the same request does not ring again.
    h.access();
    assert!(!h.app.take_bell());
    let screen = h.draw(80, 24);
    assert!(screen.contains("MCP access request"), "{screen}");
    assert!(screen.contains("test agent"), "{screen}");
    // A click inside the card, outside its buttons, does not move the editor cursor.
    h.click_label("asks for access");
    assert_eq!(h.app.console().editor.cursor(), cursor);
    for size in [(120, 40), (80, 24)] {
        assert!(h.draw(size.0, size.1).contains("[Later]"));
    }
    h.click_label("[Later]");
    let screen = h.draw(80, 24);
    assert!(!screen.contains("asks for access"), "{screen}");
    assert!(screen.contains("MCP  1 access request"), "{screen}");
    // A new request shows the card again, with its place in the queue.
    let first = h.client.take();
    h.client = Some(
        h.runtime
            .block_on(Client::connect(&h.instance, "second client"))
            .unwrap(),
    );
    h.access();
    assert!(h.app.take_bell());
    let screen = h.draw(80, 24);
    assert!(
        screen.contains("second client asks for access to SQLN. (2 of 2)"),
        "{screen}"
    );
    // Review opens the request of the card.
    h.click_label("[Review]");
    let screen = h.draw(80, 24);
    assert!(screen.contains("second client"), "{screen}");
    assert!(screen.contains("[Full access]"), "{screen}");
    h.key(KeyCode::Right);
    h.key(KeyCode::Enter);
    assert_eq!(h.access()["status"], "authorized");
    assert!(!h.draw(80, 24).contains("asks for access"));
    // A remembered client does not ring.
    h.client.take();
    h.client = Some(
        h.runtime
            .block_on(Client::connect(&h.instance, "second client"))
            .unwrap(),
    );
    assert_eq!(h.access()["status"], "authorized");
    assert!(!h.app.take_bell());
    drop(first);
}

#[test]
fn a_review_closes_when_its_proposal_is_discarded() {
    let mut h = Harness::new("review-discarded");
    h.authorize();
    let proposal = h.proposal("SELECT 1;");
    h.wait_proposal(&proposal);
    h.key(KeyCode::F(9));
    h.key(KeyCode::Enter);
    assert!(h.draw(80, 24).contains("[a Accept]"));
    // Another change to the console, such as a direct edit of another agent, invalidates it.
    h.app.console_mut().editor.on_paste("user text");
    h.app.drain_events();
    let screen = h.draw(120, 24);
    assert!(!screen.contains("[a Accept]"), "{screen}");
    assert!(screen.contains("Agent edit discarded"), "{screen}");
    assert_eq!(h.job_status(&proposal), "revision_conflict");
}

#[test]
fn proposal_cards_wait_behind_access_cards_and_open_their_review() {
    let mut h = Harness::new("proposal-card");
    h.authorize();
    let proposal = h.proposal("SELECT 7;");
    h.wait_proposal(&proposal);
    let screen = h.draw(80, 24);
    assert!(screen.contains("Edit proposal"), "{screen}");
    assert!(screen.contains("edits console-1: +1 −0 lines."), "{screen}");
    assert!(screen.contains("F9: review"), "{screen}");
    // An access request is more urgent, so its card comes first.
    let first = h.client.take();
    h.client = Some(
        h.runtime
            .block_on(Client::connect(&h.instance, "second client"))
            .unwrap(),
    );
    h.access();
    let screen = h.draw(80, 24);
    assert!(screen.contains("second client asks for access"), "{screen}");
    assert!(!screen.contains("edits console-1"), "{screen}");
    h.click_label("[Later]");
    let screen = h.draw(80, 24);
    assert!(screen.contains("edits console-1"), "{screen}");
    // Later hides the proposal card. The MCP row keeps both items.
    h.click_label("[Later]");
    let screen = h.draw(80, 24);
    assert!(!screen.contains("edits console-1"), "{screen}");
    assert!(
        screen.contains("1 access request · 1 edit proposal"),
        "{screen}"
    );
    h.client.take();
    h.client = first;
    // A new proposal shows a card again. Review opens it.
    h.key(KeyCode::F(9));
    h.key(KeyCode::Enter);
    h.key(KeyCode::Char('A'));
    assert_eq!(h.job_status(&proposal), "applied");
    let proposal = h.proposal("-- next\n");
    h.wait_proposal(&proposal);
    h.click_label("[Review]");
    let screen = h.draw(80, 24);
    assert!(screen.contains("[a Accept]"), "{screen}");
    assert!(screen.contains("+ -- next"), "{screen}");
    // The card of a proposal under review does not show.
    assert!(!screen.contains("edits console-1"), "{screen}");
}

/// Waits for the card of a task that runs long enough to show it.
fn wait_card(h: &mut Harness) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !h.draw(80, 24).contains("Agent task running") {
        h.app.drain_events();
        assert!(Instant::now() < deadline, "no task card");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_long_task_card_stops_its_task() {
    let mut h = Harness::new("task-card-stop");
    h.authorize();
    let job = h.query(&slow_sql());
    // A short task shows no card.
    assert!(!h.draw(80, 24).contains("Agent task running"));
    wait_card(&mut h);
    let screen = h.draw(80, 24);
    assert!(screen.contains("Query"), "{screen}");
    assert!(screen.contains("[Hide]"), "{screen}");
    // Stop in the card stops its task. Access stays.
    h.click_in_row("[Stop]", "[Hide]");
    let reply = h.wait_job(&job);
    assert_eq!(reply["status"], "cancelled", "{reply}");
    assert_eq!(reply["message"], sqln::ui::text::AGENT_STOP_REASON);
    assert_eq!(h.access()["status"], "authorized");
    let own = reply["tab_ids"][0].as_str().unwrap().to_owned();
    h.wait_busy(&own, false);
}

#[test]
fn hiding_a_long_task_card_keeps_its_task() {
    let mut h = Harness::new("task-card-hide");
    h.authorize();
    let own = h.own();
    let job = h.query(&slow_sql());
    wait_card(&mut h);
    // Hide removes the card. The MCP row keeps the task and its Stop button.
    h.click_label("[Hide]");
    let screen = h.draw(80, 24);
    assert!(!screen.contains("Agent task running"), "{screen}");
    assert!(screen.contains("1 task running"), "{screen}");
    h.tool(json!({"method":"job","job_id":job,"cancel":true}))
        .unwrap();
    let reply = h.wait_job(&job);
    assert_eq!(reply["status"], "cancelled", "{reply}");
    h.wait_busy(&own, false);
}

/// Reads the Messages of a console through the assistant protocol.
fn notes(h: &mut Harness, console: &str) -> String {
    h.tool(json!({"method":"read_console","console_id":console,"messages":true}))
        .unwrap()["messages"]
        .to_string()
}

/// A read that runs for some seconds: each join doubles the rows.
fn slow_sql() -> String {
    let joins = (0..40)
        .map(|index| format!("things t{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("SELECT count(*) FROM {joins}")
}

#[test]
fn the_agent_tab_shows_its_session_access_and_changes_it() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut h = Harness::mode(&format!("tab-access-{mode:?}"), mode);
        h.authorize();
        let own = h.own();
        h.click_label("MCP · test agent");
        assert_eq!(h.app.console().id.to_string(), own);
        let screen = h.draw(80, 24);
        assert!(screen.contains("[Access: Allow]"), "{screen}");
        assert!(screen.contains("[Tasks 0]"), "{screen}");
        // Keep access has the initial focus.
        h.click_label("[Access: Allow]");
        let screen = h.draw(80, 24);
        assert!(screen.contains("[Keep access]"), "{screen}");
        assert!(screen.contains("[Revoke]"), "{screen}");
        h.key(KeyCode::Enter);
        assert_eq!(h.access()["access"], "propose");
        // A custom shortcut opens the same dialog. Allow all, then Full access.
        h.key(KeyCode::F(12));
        h.key(KeyCode::Right);
        h.key(KeyCode::Right);
        h.key(KeyCode::Enter);
        assert_eq!(h.access()["access"], "full");
        let screen = h.draw(80, 24);
        assert!(screen.contains("[Access: Full access]"), "{screen}");
        assert!(notes(&mut h, &own).contains("full access allowed"));
        // Revocation keeps the tab and its log, and the button shows that access ended.
        h.key(KeyCode::F(12));
        for _ in 0..3 {
            h.key(KeyCode::Right);
        }
        h.key(KeyCode::Enter);
        assert_eq!(h.access()["status"], "revoked");
        let screen = h.draw(80, 24);
        assert!(screen.contains("[Access: off]"), "{screen}");
        assert!(
            h.app
                .consoles()
                .iter()
                .any(|console| console.id.to_string() == own),
            "the agent tab stays"
        );
    }
}

#[test]
fn the_agent_tab_lists_and_stops_the_tasks_of_its_session() {
    let mut h = Harness::new("tab-tasks");
    h.authorize();
    let own = h.own();
    h.click_label("MCP · test agent");
    let running = |h: &mut Harness, job: &str| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while h.job_status(job) != "running" {
            assert!(Instant::now() < deadline);
        }
    };
    // A task that ends while the list is open cannot be stopped from it.
    let job = h.query(&slow_sql());
    running(&mut h, &job);
    h.click_label("[Tasks 1]");
    let screen = h.draw(80, 24);
    assert!(screen.contains("Agent tasks"), "{screen}");
    assert!(screen.contains("Query"), "{screen}");
    assert!(screen.contains(&job[..8]), "{screen}");
    h.tool(json!({"method":"job","job_id":job,"cancel":true}))
        .unwrap();
    assert_eq!(h.wait_job(&job)["status"], "cancelled");
    h.key(KeyCode::Enter);
    let screen = h.draw(120, 24);
    assert!(screen.contains("This agent task ended"), "{screen}");
    assert!(!screen.contains("[Keep running]"), "{screen}");
    h.wait_busy(&own, false);
    // Keep has the initial focus. Stop cancels that task only, and access stays.
    let job = h.query(&slow_sql());
    running(&mut h, &job);
    h.key(KeyCode::F(11));
    h.key(KeyCode::Enter);
    let screen = h.draw(80, 24);
    assert!(screen.contains("[Keep running]"), "{screen}");
    assert!(screen.contains(&job[..8]), "{screen}");
    h.key(KeyCode::Enter);
    assert_eq!(h.job_status(&job), "running");
    h.key(KeyCode::F(11));
    h.key(KeyCode::Enter);
    h.key(KeyCode::Right);
    h.key(KeyCode::Enter);
    let reply = h.wait_job(&job);
    assert_eq!(reply["status"], "cancelled", "{reply}");
    assert_eq!(reply["message"], sqln::ui::text::AGENT_STOP_REASON);
    assert_eq!(h.access()["status"], "authorized");
    h.wait_busy(&own, false);
    let screen = h.draw(80, 24);
    assert!(screen.contains("[Tasks 0]"), "{screen}");
    // Without tasks, the command explains that no list opens.
    h.key(KeyCode::F(11));
    let screen = h.draw(80, 24);
    assert!(
        screen.contains("Agent tasks: no pending items."),
        "{screen}"
    );
}

#[test]
fn agent_actions_on_user_consoles_go_to_the_session_messages() {
    let mut h = Harness::new("tab-log");
    h.authorize_all();
    // The session tab has no connection yet. The note goes to it, and no other tab opens.
    let tabs = h.app.consoles().len();
    assert_eq!(h.direct("SELECT 1;").unwrap()["status"], "applied");
    assert_eq!(h.app.consoles().len(), tabs);
    assert!(h.draw(120, 24).contains("edited"));
    let own = h.own();
    assert_eq!(
        h.app.consoles().len(),
        tabs,
        "the first query binds the session tab"
    );
    let user = h.app.console().id.to_string();
    assert_ne!(user, own);
    assert_eq!(h.direct("SELECT 2;\n").unwrap()["status"], "applied");
    let log = notes(&mut h, &own);
    assert!(log.contains("edited"), "{log}");
    let revision = h.app.console().editor.revision();
    let job = h
        .tool(json!({"method":"run_console","console_id":user,"revision":revision}))
        .unwrap()["job_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(h.wait_job(&job)["status"], "completed");
    let log = notes(&mut h, &own);
    assert!(log.contains("ran"), "{log}");
    // The proposal and the user's decision also enter the log.
    let proposal = h.proposal("SELECT 3;");
    assert_eq!(h.wait_proposal(&proposal)["status"], "pending");
    assert!(notes(&mut h, &own).contains("proposed an edit"));
    h.key(KeyCode::F(9));
    h.key(KeyCode::Enter);
    h.key(KeyCode::Char('R'));
    assert_eq!(h.wait_proposal(&proposal)["status"], "rejected");
    assert!(notes(&mut h, &own).contains("Agent edit rejected for"));
}

#[test]
fn the_agent_tab_with_sql_keeps_every_button_at_the_minimum_size() {
    let mut h = Harness::new("tab-sql-buttons");
    h.authorize();
    let connection = h.connection();
    let reply = h
        .tool(json!({"method":"edit_console","connection_id":connection,"sql":"SELECT 1;"}))
        .unwrap();
    assert_eq!(reply["status"], "placed");
    h.click_label("MCP · test agent");
    // At 80x24 the explorer leaves 55 columns: the bar drops Access, and the palette keeps it.
    for (width, height) in [(80, 24), (120, 40), (80, 24)] {
        let screen = h.draw(width, height);
        for label in ["[Run]", "[New console]", "[Clear]", "[Stop]", "[Tasks 0]"] {
            assert!(
                screen.contains(label),
                "{label} at {width}x{height}:\n{screen}"
            );
        }
        assert_eq!(screen.contains("[Access: Allow]"), width > 80, "{screen}");
    }
    h.key(KeyCode::F(12));
    assert!(h.draw(80, 24).contains("[Keep access]"));
}

fn close_tab(h: &mut Harness) {
    h.app.on_event(Event::Key(crate::common::ctrl('w')));
    h.key(KeyCode::Char('c'));
}

/// Returns the agent tabs, with their connection.
fn agent_tabs(h: &Harness) -> Vec<(String, bool)> {
    h.app
        .consoles()
        .iter()
        .filter(|console| console.name.starts_with("MCP · "))
        .map(|console| (console.id.to_string(), console.connection.is_some()))
        .collect()
}

#[test]
fn the_agent_tab_opens_at_approval_on_the_right_and_takes_the_first_connection() {
    let mut h = Harness::new("tab-open");
    let active = h.app.console().id;
    h.access();
    assert!(agent_tabs(&h).is_empty(), "a request opens no tab");
    h.key(KeyCode::F(8));
    h.key(KeyCode::Enter);
    h.key(KeyCode::Right);
    h.key(KeyCode::Enter);
    assert_eq!(h.access()["status"], "authorized");
    // The tab opens without a connection, and the focus stays.
    let tabs = agent_tabs(&h);
    assert_eq!(tabs.len(), 1, "{tabs:?}");
    assert!(!tabs[0].1);
    assert_eq!(h.app.console().id, active);
    for width in [80, 120] {
        let screen = h.draw(width, 24);
        let row = screen.lines().next().unwrap();
        let user = row.find("console-1").unwrap();
        let agent = row.find("MCP · test agent").unwrap();
        assert!(agent > user, "{row}");
        // The agent tab stands at the right end, just before Quit.
        assert!(row[user..agent].contains("     "), "{row}");
        assert!(row.trim_end().ends_with("X  [Quit]"), "{row}");
    }
    // The first query binds that tab. No second tab opens.
    let own = h.own();
    let tabs = agent_tabs(&h);
    assert_eq!(tabs, vec![(own, true)]);
    // A new user console opens to the left of the agent tab.
    let consoles = h.app.consoles().len();
    h.click_label("tab-open.db");
    h.key(KeyCode::Char('n'));
    assert_eq!(h.app.consoles().len(), consoles + 1);
    let screen = h.draw(120, 24);
    let row = screen.lines().next().unwrap();
    assert!(
        row.rfind("console-").unwrap() < row.find("MCP · ").unwrap(),
        "{row}"
    );
}

#[test]
fn a_rejected_request_opens_no_agent_tab() {
    let mut h = Harness::new("tab-reject");
    h.access();
    h.key(KeyCode::F(8));
    h.key(KeyCode::Enter);
    h.key(KeyCode::Enter);
    assert_eq!(h.access()["status"], "denied");
    assert!(agent_tabs(&h).is_empty());
}

#[test]
fn the_last_agent_tab_closes_only_with_a_revocation() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut h = Harness::mode(&format!("tab-close-{mode:?}"), mode);
        h.authorize();
        let own = h.own();
        h.click_label("MCP · test agent");
        // Cancel has the initial focus and keeps the tab and the access.
        close_tab(&mut h);
        let screen = h.draw(80, 24);
        assert!(screen.contains("Close the agent tab"), "{screen}");
        assert!(screen.contains("[Revoke and close]"), "{screen}");
        h.key(KeyCode::Enter);
        assert_eq!(agent_tabs(&h).len(), 1);
        assert_eq!(h.access()["status"], "authorized");
        // Revoke and close ends the access, then the tab closes.
        close_tab(&mut h);
        h.key(KeyCode::Right);
        h.key(KeyCode::Enter);
        assert_eq!(h.access()["status"], "revoked");
        assert!(agent_tabs(&h).is_empty());
        assert!(
            !h.app
                .consoles()
                .iter()
                .any(|item| item.id.to_string() == own)
        );
    }
}

#[test]
fn a_revoked_agent_tab_stays_and_closes_without_a_question() {
    let mut h = Harness::new("tab-after");
    h.authorize();
    h.own();
    h.key(KeyCode::F(8));
    h.key(KeyCode::Enter);
    for _ in 0..3 {
        h.key(KeyCode::Right);
    }
    h.key(KeyCode::Enter);
    assert_eq!(h.access()["status"], "revoked");
    assert_eq!(agent_tabs(&h).len(), 1, "the tab stays with its log");
    assert!(h.draw(80, 24).contains("MCP off ·"));
    h.click_label("MCP off ·");
    close_tab(&mut h);
    assert!(!h.draw(80, 24).contains("Close the agent tab"));
    assert!(agent_tabs(&h).is_empty());
}

/// Opens SQLN with MCP preferences, as the configuration gives them.
fn with_settings(name: &str, agent: AgentSettings) -> Harness {
    Harness::options(
        Options {
            database: Some(seeded_db(name)),
            agent,
            ..Default::default()
        },
        KeyboardMode::Conventional,
    )
}

#[test]
fn the_default_access_allows_a_new_session_without_a_question_up_to_the_maximum() {
    let mut h = with_settings(
        "default-access",
        AgentSettings {
            default_access: AgentDefault::Full,
            max_access: AgentAccess::Direct,
            ..Default::default()
        },
    );
    let reply = h.access();
    assert_eq!(reply["status"], "authorized");
    assert_eq!(reply["access"], "direct");
    let screen = h.draw(80, 24);
    assert!(!screen.contains("MCP access request"), "{screen}");
    assert!(screen.contains("MCP 1 all"), "{screen}");
    let log = agent_log(&h);
    assert!(log.contains("default access: Allow all"), "{log}");
}

#[test]
fn the_maximum_access_hides_the_higher_options() {
    let mut h = with_settings(
        "max-access",
        AgentSettings {
            max_access: AgentAccess::Propose,
            ..Default::default()
        },
    );
    assert_eq!(h.access()["status"], "pending");
    h.click_label("[Review]");
    let screen = h.draw(80, 24);
    assert!(screen.contains("[Allow]"), "{screen}");
    assert!(!screen.contains("[Allow all]"), "{screen}");
    assert!(!screen.contains("[Full access]"), "{screen}");
    h.key(KeyCode::Right);
    h.key(KeyCode::Enter);
    assert_eq!(h.access()["access"], "propose");
    // The session dialog offers no higher level.
    h.key(KeyCode::F(8));
    h.key(KeyCode::Enter);
    let screen = h.draw(80, 24);
    assert!(screen.contains("[Keep access]"), "{screen}");
    assert!(!screen.contains("[Allow all]"), "{screen}");
    assert!(!screen.contains("[Full access]"), "{screen}");
}

#[test]
fn without_remembered_clients_a_reconnected_client_asks_again() {
    let mut h = with_settings(
        "no-remember",
        AgentSettings {
            remember_clients: false,
            ..Default::default()
        },
    );
    h.grant(3);
    h.client = Some(
        h.runtime
            .block_on(Client::connect(&h.instance, "test agent\u{1b}[31m"))
            .unwrap(),
    );
    assert_eq!(h.access()["status"], "pending");
}

/// Returns the Messages of the agent tabs, as one text.
fn agent_log(h: &Harness) -> String {
    h.app
        .consoles()
        .iter()
        .filter(|console| console.name.starts_with("MCP · "))
        .flat_map(|console| console.messages.iter().map(|message| message.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn saved_mcp_settings_lower_the_access_close_and_open_the_endpoint_at_once() {
    use sqln::store::{Root, Store};
    let base = crate::common::temp_dir("agent-mcp-live");
    let project = Root::project(&base.join("project"));
    std::fs::create_dir_all(&project.dir).unwrap();
    let store = Store {
        global: Root::global(base.join("global")),
        project: Some(project),
    };
    let mut h = Harness::options(
        Options {
            database: Some(seeded_db("mcp-live")),
            store: Some(store),
            ..Default::default()
        },
        KeyboardMode::Conventional,
    );
    h.grant(3);
    h.own();
    // The MCP settings at 80x24. Scope, MCP, Default access, then Max access.
    // Four rows down from General is the MCP section of the sidebar.
    let open_mcp = |h: &mut Harness| {
        h.app.on_event(Event::Key(crate::common::ctrl('w')));
        h.key(KeyCode::Char(','));
        for _ in 0..4 {
            h.key(KeyCode::Down);
        }
        for _ in 0..2 {
            h.key(KeyCode::Tab);
        }
    };
    open_mcp(&mut h);
    let screen = h.draw(80, 24);
    assert!(screen.contains("Remember clients"), "{screen}");
    assert!(screen.contains("[Save]"), "{screen}");
    for _ in 0..2 {
        h.key(KeyCode::Tab);
    }
    h.key(KeyCode::Right);
    assert!(h.draw(80, 24).contains("< Allow >"));
    h.click_label("[Save]");
    let reply = h.access();
    assert_eq!(reply["status"], "authorized");
    assert_eq!(reply["access"], "propose");
    assert!(agent_log(&h).contains("access lowered to Allow"));
    // MCP off revokes the session and removes the endpoint.
    open_mcp(&mut h);
    h.key(KeyCode::Right);
    h.key(KeyCode::Right);
    assert!(h.draw(80, 24).contains("< Off >"));
    h.click_label("[Save]");
    let screen = h.draw(80, 24);
    assert!(screen.contains("MCP off ·"), "{screen}");
    assert!(!h.instance.socket.exists());
    let descriptors = |h: &Harness| {
        std::fs::read_dir(&h.directory)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "json")
            })
            .count()
    };
    assert_eq!(descriptors(&h), 0);
    // The default, MCP on, opens a new endpoint.
    open_mcp(&mut h);
    h.key(KeyCode::Right);
    assert!(h.draw(80, 24).contains("< Default: On >"));
    h.click_label("[Save]");
    assert_eq!(descriptors(&h), 1);
}

/// An edit that replaces the characters from `column` to `end` on `line`.
fn replace(line: usize, column: usize, end: usize, text: &str) -> Value {
    json!({"start":{"line":line,"column":column},"end":{"line":line,"column":end},"text":text})
}

#[test]
fn each_change_has_its_own_decision_undo_step_and_count() {
    let mut h = Harness::new("partial-review");
    h.authorize();
    let before = "SELECT 1;\nSELECT 2;\nSELECT 3;\nSELECT 4;\nSELECT 5;";
    h.app.on_event(Event::Paste(before.into()));
    let console = h.app.console().id.to_string();
    let revision = h.app.console().editor.revision();
    let reply = h
        .tool(
            json!({"method":"edit_console","console_id":console,"revision":revision,
            "edits":[replace(0, 7, 8, "10"), replace(4, 7, 8, "50")]}),
        )
        .unwrap();
    let proposal = reply["job_id"].as_str().unwrap().to_owned();
    assert_eq!(h.wait_proposal(&proposal)["status"], "pending");
    h.key(KeyCode::F(9));
    h.key(KeyCode::Enter);
    let screen = h.draw(80, 24);
    assert!(screen.contains("1/2 changes"), "{screen}");
    assert!(screen.contains("- SELECT 1;"), "{screen}");
    assert!(screen.contains("+ SELECT 10;"), "{screen}");
    // The first change goes into the text at once.
    h.key(KeyCode::Char('a'));
    let first = "SELECT 10;\nSELECT 2;\nSELECT 3;\nSELECT 4;\nSELECT 5;";
    assert_eq!(h.app.console().editor.text(), first);
    assert!(h.draw(80, 24).contains("1/1 change"));
    // Later keeps the decision and the proposal. Review opens the remaining change.
    h.key(KeyCode::Esc);
    assert_eq!(h.job_status(&proposal), "pending");
    h.key(KeyCode::F(9));
    h.key(KeyCode::Enter);
    let screen = h.draw(80, 24);
    assert!(screen.contains("1/1 change"), "{screen}");
    assert!(screen.contains("+ SELECT 50;"), "{screen}");
    h.key(KeyCode::Char('r'));
    let done = h.wait_proposal(&proposal);
    assert_eq!(done["status"], "partial");
    assert_eq!(done["accepted_changes"], 1);
    assert_eq!(done["rejected_changes"], 1);
    assert_eq!(h.app.console().editor.text(), first);
    assert!(h.draw(120, 24).contains("partly applied"));
    // The editor stays, and the text is free again.
    assert!(h.app.console().editor.review().is_none());
    h.app.on_event(Event::Key(crate::common::ctrl('z')));
    assert_eq!(h.app.console().editor.text(), before);
}

#[test]
fn custom_review_keys_show_in_the_bar_and_the_old_keys_are_text() {
    let overrides = HashMap::from([
        ("agentSessions".into(), vec!["f8".into()]),
        ("agentReview".into(), vec!["f9".into()]),
        ("reviewAccept".into(), vec!["y".into()]),
        ("reviewClose".into(), vec!["q".into()]),
    ]);
    let mode = KeyboardMode::Conventional;
    let options = Options {
        database: Some(seeded_db("review-keys")),
        keymap: Some(Keymap::with_overrides(mode, &overrides).unwrap()),
        ..Default::default()
    };
    let mut h = Harness::options(options, mode);
    h.authorize();
    let proposal = h.proposal("SELECT 8;");
    h.wait_proposal(&proposal);
    h.key(KeyCode::F(9));
    h.key(KeyCode::Enter);
    let screen = h.draw(80, 24);
    assert!(screen.contains("[y Accept]"), "{screen}");
    assert!(screen.contains("[q"), "{screen}");
    // The Ctrl+W panel menu lists the review commands.
    h.app.on_event(Event::Key(crate::common::ctrl('w')));
    h.key(KeyCode::Char(' '));
    let menu = h.draw(80, 24);
    assert!(menu.contains("Ctrl+W · Review"), "{menu}");
    assert!(menu.contains("y    Accept change"), "{menu}");
    h.key(KeyCode::Esc);
    h.key(KeyCode::Char('a'));
    assert!(h.app.console().editor.text().is_empty());
    h.key(KeyCode::Char('q'));
    assert_eq!(h.job_status(&proposal), "pending");
    h.key(KeyCode::F(9));
    h.key(KeyCode::Enter);
    h.key(KeyCode::Char('y'));
    assert_eq!(h.job_status(&proposal), "applied");
    assert_eq!(h.app.console().editor.text(), "SELECT 8;");
}
