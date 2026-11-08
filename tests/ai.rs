mod common;

mod agent;
mod ai_editor;
mod ai_schema;
mod mcp;

use common::{ctrl, key, shift};
use crossterm::event::{Event, KeyCode, KeyModifiers};
use edtui::Index2;
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};
use sqln::{
    ai::{self, Context, Credential, Kind, Object, Preferences, Provider, Snapshot},
    app::{App, Options},
    clipboard::MemoryClipboard,
    editor::KeyboardMode,
    grid::Budget,
    input::Action,
    ui::text,
    ui::theme::Theme,
};
use std::{
    cell::RefCell,
    io::{Read, Write},
    net::TcpListener,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthStr;

struct Server {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new(text: &str, delay: Duration) -> Self {
        let body =
            json!({"choices":[{"text":text,"message":{"content":text},"finish_reason":"stop"}]})
                .to_string();
        Self::response(200, body, delay)
    }
    fn response(status: u16, body: String, delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let thread = std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                let (mut socket, _) = match listener.accept() {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(_) => break,
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 8192];
                loop {
                    let n = match socket.read(&mut buffer) {
                        Ok(n) if n > 0 => n,
                        _ => break,
                    };
                    request.extend_from_slice(&buffer[..n]);
                    if let Some(end) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&request[..end]);
                        let size = header
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|s| s.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if request.len() >= end + 4 + size {
                            break;
                        }
                    }
                }
                if request.is_empty() {
                    continue;
                }
                captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(request).unwrap());
                std::thread::sleep(delay);
                let _ = write!(
                    socket,
                    "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
            }
        });
        Self {
            url,
            requests,
            stop,
            thread: Some(thread),
        }
    }
    fn provider(&self) -> Provider {
        Provider {
            kind: Kind::Compatible,
            model: "test-model".into(),
            base_url: self.url.clone(),
            ..Provider::default()
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            t.join().unwrap();
        }
    }
}

fn snapshot(objects: Vec<Object>) -> Snapshot {
    Snapshot::new(objects, &Arc::new(Budget::new(64 * 1024 * 1024))).unwrap()
}
fn context() -> Context {
    Context {
        prefix: "SELECT ".into(),
        original: String::new(),
        suffix: " FROM users;".into(),
        dialect: sqln::sql::Dialect::Sqlite,
        schema: Some("main".into()),
        instruction: None,
    }
}
fn screen(app: &mut App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}
fn pump(app: &mut App, condition: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        app.drain_events();
        let text = screen(app);
        if condition(&text) {
            return text;
        }
        assert!(Instant::now() < deadline, "screen: {text}");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn app(name: &str, provider: Provider) -> App {
    let path = common::temp_db(name);
    rusqlite::Connection::open(&path).unwrap().execute_batch("CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT); CREATE VIEW active_users AS SELECT name FROM users;").unwrap();
    App::new(
        Options {
            database: Some(path),
            ai_completion: Preferences {
                enabled: Some(true),
                max_simultaneous: None,
                provider: Some(provider),
            },
            ..Options::default()
        },
        Rc::new(RefCell::new(MemoryClipboard::default())),
        Theme::plain(),
    )
}
/// Returns true when the editor shows ghost text with its bar.
fn ghost(screen: &str) -> bool {
    screen.contains("[Tab Accept]")
}
fn rows(screen: &str) -> String {
    let chars: Vec<char> = screen.chars().collect();
    chars
        .chunks(80)
        .map(|row| row.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}
fn request(app: &mut App) {
    app.on_event(Event::Key(ctrl(' ')));
    let list = app.console().editor.suggestions().unwrap();
    if !list.completion.items.is_empty() {
        app.on_event(Event::Key(key(KeyCode::Up)));
    }
    app.on_event(Event::Key(key(KeyCode::Tab)));
}
/// Selects the request row of the suggestion list, which opens the request input.
fn open_request(app: &mut App) {
    app.on_event(Event::Key(ctrl(' ')));
    let list = app.console().editor.suggestions().unwrap();
    let steps = if list.completion.items.is_empty() {
        1
    } else {
        2
    };
    for _ in 0..steps {
        app.on_event(Event::Key(key(KeyCode::Up)));
    }
    app.on_event(Event::Key(key(KeyCode::Tab)));
    pump(app, |screen| {
        screen.contains("Complete with AI with a request")
    });
}
fn approve(app: &mut App) {
    pump(app, |s| s.contains("Send SQL and schema"));
    app.on_event(Event::Key(key(KeyCode::Tab)));
    app.on_event(Event::Key(key(KeyCode::Enter)));
}

/// Clicks a label on the screen with the left button.
fn click_at(app: &mut App, label: &str) {
    let shown = rows(&screen(app));
    let (x, y) = shown
        .lines()
        .enumerate()
        .find_map(|(y, line)| {
            line.find(label)
                .map(|byte| (UnicodeWidthStr::width(&line[..byte]) + 1, y))
        })
        .unwrap_or_else(|| panic!("Missing {label}:\n{shown}"));
    app.on_event(Event::Mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: x as u16,
        row: y as u16,
        modifiers: KeyModifiers::NONE,
    }));
}

/// Pastes `text`, requests a completion at column 7 of the first line, and waits for
/// its ghost.
fn ghost_at_first_line(app: &mut App, text: &str) {
    app.on_event(Event::Paste(text.into()));
    app.console_mut().editor.set_cursor(Index2::new(0, 7));
    request(app);
    approve(app);
    pump(app, ghost);
}

#[test]
fn ai_is_above_local_items_but_not_initially_selected() {
    let mut f = common::Fixture::conventional("sel");
    f.editor.set_cursor(Index2::new(0, 3));
    f.editor.set_ai_available(true);
    f.press(ctrl(' '));
    // The list starts at the first local item, above the two AI rows.
    assert_eq!(f.editor.suggestions().unwrap().selected, 2);
    f.press(key(KeyCode::Up));
    assert_eq!(f.editor.suggestions().unwrap().selected, 1);
    f.press(key(KeyCode::Tab));
    assert_eq!(
        f.editor.take_ai_request(),
        Some(sqln::editor::ai::AiSelection::Plain)
    );
    assert_eq!(f.text(), "sel");
}
#[test]
fn the_request_row_sits_above_the_plain_ai_row() {
    let mut f = common::Fixture::conventional("sel");
    f.editor.set_cursor(Index2::new(0, 3));
    f.editor.set_ai_available(true);
    f.press(ctrl(' '));
    f.press(key(KeyCode::Up));
    f.press(key(KeyCode::Up));
    assert_eq!(f.editor.suggestions().unwrap().selected, 0);
    f.press(key(KeyCode::Up));
    // The selection wraps to the last local item.
    let count = f.editor.suggestions().unwrap().completion.items.len();
    assert_eq!(f.editor.suggestions().unwrap().selected, count + 1);
    f.press(key(KeyCode::Down));
    assert_eq!(f.editor.suggestions().unwrap().selected, 0);
    f.press(key(KeyCode::Tab));
    assert_eq!(
        f.editor.take_ai_request(),
        Some(sqln::editor::ai::AiSelection::Request)
    );
}
#[test]
fn explicit_ai_only_list_handles_repeated_arrows_without_panics() {
    let mut f = common::Fixture::conventional("-- describe a query");
    f.editor.set_cursor(Index2::new(0, 19));
    f.editor.set_ai_available(true);
    f.press(ctrl(' '));
    // The plain row stays the first selection of an AI-only list.
    assert_eq!(f.editor.suggestions().unwrap().selected, 1);
    for _ in 0..5 {
        f.press(key(KeyCode::Up));
        f.press(key(KeyCode::Down));
    }
    assert_eq!(f.editor.suggestions().unwrap().selected, 1);
    f.press(key(KeyCode::Tab));
    assert_eq!(
        f.editor.take_ai_request(),
        Some(sqln::editor::ai::AiSelection::Plain)
    );
}
#[test]
fn typing_no_local_match_does_not_open_an_ai_only_list() {
    let mut f = common::Fixture::conventional("");
    f.editor.set_ai_available(true);
    f.write("zzzzzz");
    assert!(f.editor.suggestions().is_none());
}
#[test]
fn a_replacement_preserves_unicode_and_is_one_undo_step() {
    for keyboard in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut f = common::Fixture::new("SELECT café;", keyboard);
        if keyboard == KeyboardMode::Vim {
            f.press(key(KeyCode::Char('i')));
        }
        // An agent edit replaces the selected word in one undo step, and the
        // multibyte text stays whole.
        let edit = sqln::agent::Edit {
            start: sqln::agent::Position { line: 0, column: 7 },
            end: sqln::agent::Position {
                line: 0,
                column: 11,
            },
            text: "42".into(),
        };
        f.editor
            .apply_agent_edits(f.editor.revision(), &[edit])
            .unwrap();
        assert_eq!(f.text(), "SELECT 42;");
        f.editor.on_command(sqln::input::Action::EditorUndo, None);
        assert_eq!(f.text(), "SELECT café;");
    }
}
#[test]
fn an_application_uses_the_anchor_not_the_cursor() {
    let mut f = common::Fixture::conventional("SELECT \nFROM t");
    f.editor.set_cursor(Index2::new(0, 7));
    let target = f.editor.ai_target(None);
    f.editor.set_cursor(Index2::new(1, 6));
    f.write("x");
    f.editor.apply_ghost(target.start, "42").unwrap();
    assert_eq!(f.text(), "SELECT 42\nFROM tx");
}
#[test]
fn the_chat_body_preserves_the_context_and_the_original() {
    let server = Server::new("1", Duration::ZERO);
    let p = server.provider();
    let mut ctx = context();
    ctx.original = "*".into();
    let prepared = ai::prepare(&p, &ctx, &snapshot(Vec::new())).unwrap();
    let user = prepared.body["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("FROM users"));
    assert!(user.contains("original"));
}
#[test]
fn context_reduction_is_deterministic_and_keeps_the_cache_complete() {
    let server = Server::new("1", Duration::ZERO);
    let mut p = server.provider();
    p.context_window_tokens = 4096;
    let objects = (0..200)
        .map(|n| Object {
            schema: "main".into(),
            name: format!("table_{n:03}"),
            kind: "Table".into(),
            metadata: vec![json!({"comment":"x".repeat(100)})],
            targets: Vec::new(),
        })
        .collect();
    let snap = snapshot(objects);
    let a = ai::prepare(&p, &context(), &snap).unwrap();
    let b = ai::prepare(&p, &context(), &snap).unwrap();
    assert!(a.omitted > 0);
    assert_eq!(a.body, b.body);
    assert_eq!(snap.objects.len(), 200);
}
#[test]
fn the_metadata_section_stays_identical_across_requests() {
    let server = Server::new("1", Duration::ZERO);
    let mut p = server.provider();
    p.context_window_tokens = 4096;
    let objects = (0..40)
        .map(|n| Object {
            schema: "main".into(),
            name: format!("table_{n:03}"),
            kind: "Table".into(),
            metadata: vec![json!({"section":"columns","values":{"c":"x".repeat(20)}})],
            targets: Vec::new(),
        })
        .collect();
    let snap = snapshot(objects);
    let mut first = context();
    first.prefix = "SELECT c FROM table_005 ".into();
    let mut second = context();
    second.prefix = "INSERT INTO table_039 ".into();
    second.suffix = " VALUES (1)".into();
    let a = ai::prepare(&p, &first, &snap).unwrap();
    let b = ai::prepare(&p, &second, &snap).unwrap();
    let section = |prepared: &sqln::ai::Prepared| {
        let user = prepared.body["messages"][1]["content"].as_str().unwrap();
        assert!(user.starts_with("{\"metadata\":"), "{user}");
        let end = user.find(",\"context\":").unwrap();
        user[..end].to_owned()
    };
    assert_eq!(section(&a), section(&b));
}
#[test]
fn referenced_objects_keep_the_canonical_order_and_list_in_focus() {
    let server = Server::new("1", Duration::ZERO);
    let p = server.provider();
    let objects = vec![
        Object {
            schema: "main".into(),
            name: "zeta".into(),
            kind: "Table".into(),
            metadata: Vec::new(),
            targets: vec![("other".into(), "alpha".into())],
        },
        Object {
            schema: "main".into(),
            name: "beta".into(),
            kind: "Table".into(),
            metadata: Vec::new(),
            targets: Vec::new(),
        },
        Object {
            schema: "other".into(),
            name: "alpha".into(),
            kind: "Table".into(),
            metadata: Vec::new(),
            targets: Vec::new(),
        },
    ];
    let snap = snapshot(objects);
    let mut ctx = context();
    ctx.prefix = "SELECT * FROM zeta ".into();
    ctx.suffix = String::new();
    let prepared = ai::prepare(&p, &ctx, &snap).unwrap();
    let user = prepared.body["messages"][1]["content"].as_str().unwrap();
    let order = (
        user.find("\"name\":\"beta\"").unwrap(),
        user.find("\"name\":\"zeta\"").unwrap(),
        user.find("\"name\":\"alpha\"").unwrap(),
    );
    assert!(order.0 < order.1 && order.1 < order.2, "{user}");
    assert!(
        user.contains("\"focus\":[\"main.zeta\",\"other.alpha\"]"),
        "{user}"
    );
    let system = prepared.body["messages"][0]["content"].as_str().unwrap();
    assert!(system.contains("focus field"));
}
#[test]
fn invalid_endpoints_and_credential_destination_changes_are_refused() {
    let mut p = Provider {
        kind: Kind::Compatible,
        model: "m".into(),
        ..Provider::default()
    };
    for endpoint in [
        "http://example.com/v1",
        "https://user:password@example.com",
        "https://example.com?key=secret",
        "https://example.com/#secret",
    ] {
        p.base_url = endpoint.into();
        assert!(p.validate().is_err());
    }
    p.base_url = "https://example.com/v1".into();
    p.credential = Credential::Vault {
        id: "id".into(),
        endpoint: "https://other.example/v1".into(),
    };
    assert!(p.validate().is_err());
}
#[test]
fn rotating_key_file_is_read_again_and_never_falls_back() {
    let path = std::env::temp_dir().join(format!("sqln-key-{}", uuid::Uuid::new_v4()));
    std::fs::write(&path, "first\r\n").unwrap();
    assert_eq!(ai::credentials::read_file(&path).unwrap().expose(), "first");
    std::fs::write(&path, "second\n").unwrap();
    assert_eq!(
        ai::credentials::read_file(&path).unwrap().expose(),
        "second"
    );
    std::fs::remove_file(&path).unwrap();
    assert!(ai::credentials::read_file(&path).is_err());
}
#[test]
fn replacement_rejects_empty_truncated_and_unchanged_content() {
    assert!(ai::replacement("", Some("stop"), "").is_err());
    assert!(ai::replacement("SELECT", Some("length"), "").is_err());
    assert!(ai::replacement("a", Some("stop"), "a").is_err());
    assert_eq!(
        ai::replacement("```sql\n  42\n```", Some("stop"), "").unwrap(),
        "  42"
    );
}
#[test]
fn configuration_inherits_provider_as_a_unit_and_project_can_disable() {
    let global = Preferences {
        enabled: Some(true),
        max_simultaneous: None,
        provider: Some(Provider::default()),
    };
    let project = Preferences {
        enabled: Some(false),
        max_simultaneous: None,
        provider: None,
    };
    let merged = Preferences::resolve(Some(&global), Some(&project));
    assert!(!merged.enabled());
    assert_eq!(merged.provider, global.provider);
    let old: sqln::store::config::Preferences = serde_json::from_str("{}").unwrap();
    assert!(old.ai_completion.is_none());
}
#[test]
fn the_request_field_travels_only_when_present() {
    let server = Server::new("1", Duration::ZERO);
    let p = server.provider();
    let plain = ai::prepare(&p, &context(), &snapshot(Vec::new())).unwrap();
    let user = plain.body["messages"][1]["content"].as_str().unwrap();
    assert!(!user.contains("\"request\":"));
    let mut ctx = context();
    ctx.instruction = Some("add ORDER BY id".into());
    let prepared = ai::prepare(&p, &ctx, &snapshot(Vec::new())).unwrap();
    let user = prepared.body["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("\"request\":\"add ORDER BY id\""), "{user}");
    let system = prepared.body["messages"][0]["content"].as_str().unwrap();
    assert!(system.contains("request field"));
}
#[test]
fn a_long_request_counts_against_the_context_limit() {
    let server = Server::new("1", Duration::ZERO);
    let p = server.provider();
    let mut ctx = context();
    ctx.instruction = Some("x".repeat(40_000));
    assert!(ai::prepare(&p, &ctx, &snapshot(Vec::new())).is_err());
}
#[test]
fn a_typed_request_travels_to_the_model() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-request", server.provider());
    app.on_event(Event::Paste("SELECT ".into()));
    open_request(&mut app);
    for c in "add ORDER BY id".chars() {
        app.on_event(Event::Key(key(KeyCode::Char(c))));
    }
    // The input is a dialog: the keys stay out of the editor.
    assert_eq!(app.console().editor.text(), "SELECT ");
    app.on_event(Event::Key(key(KeyCode::Enter)));
    approve(&mut app);
    let shown = pump(&mut app, ghost);
    assert!(shown.contains("SELECT 42"), "{shown}");
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].contains("add ORDER BY id") && requests[0].contains("\\\"request\\\""),
        "{}",
        requests[0]
    );
}
#[test]
fn an_empty_request_sends_a_plain_completion() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-request-empty", server.provider());
    app.on_event(Event::Paste("SELECT ".into()));
    open_request(&mut app);
    app.on_event(Event::Key(key(KeyCode::Enter)));
    approve(&mut app);
    let shown = pump(&mut app, ghost);
    assert!(shown.contains("SELECT 42"), "{shown}");
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(!requests[0].contains("\\\"request\\\""));
}
#[test]
fn escape_closes_the_request_input_without_a_request() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-request-escape", server.provider());
    app.on_event(Event::Paste("SELECT ".into()));
    open_request(&mut app);
    app.on_event(Event::Key(key(KeyCode::Esc)));
    let shown = pump(&mut app, |screen| {
        !screen.contains("Complete with AI with a request")
    });
    assert!(!ghost(&shown));
    assert_eq!(server.requests.lock().unwrap().len(), 0);
}
#[test]
fn the_request_input_accepts_paste() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-request-paste", server.provider());
    app.on_event(Event::Paste("SELECT ".into()));
    open_request(&mut app);
    app.on_event(Event::Paste("ORDER BY id".into()));
    assert!(screen(&mut app).contains("ORDER BY id"));
}
#[test]
fn the_palette_offers_the_request_command_and_opens_its_input() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-request-palette", server.provider());
    app.on_event(Event::Paste("SELECT ".into()));
    app.on_event(Event::Key(ctrl('p')));
    app.on_event(Event::Paste("with a request".into()));
    let shown = pump(&mut app, |screen| {
        screen.contains("Complete with AI with a request")
    });
    assert!(shown.contains("Complete with AI"), "{shown}");
    app.on_event(Event::Key(key(KeyCode::Enter)));
    pump(&mut app, |screen| screen.contains("Request: "));
}
#[test]
fn user_accepts_a_preview_without_executing_sql() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-insert", server.provider());
    app.on_event(Event::Paste("SELECT ".into()));
    request(&mut app);
    approve(&mut app);
    let shown = pump(&mut app, ghost);
    // The ghost text stands at the cursor, and the text does not change before Tab.
    assert!(shown.contains("SELECT 42"), "{shown}");
    assert_eq!(app.console().editor.text(), "SELECT ");
    assert!(app.console().results.is_empty());
    // The arrows keep the ghost text, and the cursor stays.
    let cursor = app.console().editor.cursor();
    for code in [KeyCode::Left, KeyCode::Right, KeyCode::Up, KeyCode::Down] {
        app.on_event(Event::Key(key(code)));
        assert!(ghost(&screen(&mut app)), "{code:?}");
    }
    assert_eq!(app.console().editor.cursor(), cursor);
    app.on_event(Event::Key(key(KeyCode::Tab)));
    assert_eq!(app.console().editor.text(), "SELECT 42");
    assert!(!ghost(&screen(&mut app)));
    assert!(app.console().results.is_empty());
    app.on_event(Event::Key(ctrl('z')));
    assert_eq!(app.console().editor.text(), "SELECT ");
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("active_users"));
    assert!(requests[0].contains("nullable"));
}

#[test]
fn the_ghost_buttons_sit_after_the_text_without_a_bar() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-buttons", server.provider());
    ghost_at_first_line(&mut app, "SELECT ");
    let shown = screen(&mut app);
    let rows = rows(&shown);
    // The buttons share the row after the ghost text, and no bar takes the last row.
    let accept = rows.lines().position(|line| line.contains("[Tab Accept]"));
    let ghost_row = rows.lines().position(|line| line.contains("SELECT 42"));
    assert_eq!(accept, ghost_row.map(|row| row + 1), "{rows}");
    assert!(shown.contains("[Esc Reject]"), "{rows}");
}

#[test]
fn a_click_moves_the_cursor_and_keeps_the_ghost() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-click", server.provider());
    ghost_at_first_line(&mut app, "SELECT \nFROM t");
    click_at(&mut app, "FROM t");
    assert_eq!(app.console().editor.cursor().row, 1);
    assert!(app.console().editor.has_ghosts());
    // Tab decides the ghost at the line of the cursor: a cursor on another line
    // refuses it, and the cursor on the anchor line accepts at the anchor.
    app.on_event(Event::Key(key(KeyCode::Tab)));
    assert_eq!(app.console().editor.text(), "SELECT \nFROM t");
    app.on_event(Event::Key(key(KeyCode::Up)));
    app.on_event(Event::Key(key(KeyCode::Tab)));
    assert_eq!(app.console().editor.text(), "SELECT 42\nFROM t");
}

#[test]
fn edits_and_undo_decide_the_ghost() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-edit", server.provider());
    ghost_at_first_line(&mut app, "SELECT \nFROM t");
    // Typing on another line keeps the ghost, and the cursor moves with the arrows.
    app.on_event(Event::Key(key(KeyCode::Down)));
    assert!(app.console().editor.has_ghosts());
    assert_eq!(app.console().editor.cursor().row, 1);
    app.on_event(Event::Key(key(KeyCode::Char('x'))));
    assert!(app.console().editor.has_ghosts());
    assert_eq!(app.console().editor.text(), "SELECT \nFROM tx");
    // Any undo discards the ghost, in silence.
    app.on_event(Event::Key(ctrl('z')));
    assert!(!app.console().editor.has_ghosts());
    assert_eq!(app.console().editor.text(), "SELECT \nFROM t");
}

#[test]
fn an_edit_on_the_anchor_line_discards_the_ghost() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-anchor", server.provider());
    ghost_at_first_line(&mut app, "SELECT ");
    app.on_event(Event::Key(key(KeyCode::Backspace)));
    assert!(!app.console().editor.has_ghosts());
    assert_eq!(app.console().editor.text(), "SELECT");
}

#[test]
fn run_executes_without_the_ghost_and_keeps_it() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-run", server.provider());
    ghost_at_first_line(&mut app, "SELECT 1;");
    app.on_event(Event::Key(key(KeyCode::F(5))));
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.console().is_busy() {
        assert!(Instant::now() < deadline);
        app.drain_events();
        std::thread::sleep(Duration::from_millis(2));
    }
    // The statement runs without the ghost text, and the ghost still waits.
    assert_eq!(app.console().editor.text(), "SELECT 1;");
    assert!(!app.console().results.is_empty());
    assert!(app.console().editor.has_ghosts());
}

#[test]
fn a_new_request_on_a_line_without_a_ghost_adds_one() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-replace", server.provider());
    ghost_at_first_line(&mut app, "SELECT \nFROM t");
    app.on_event(Event::Key(key(KeyCode::Down)));
    request(&mut app);
    pump(&mut app, |s| s.contains("FROM t42"));
    assert_eq!(server.requests.lock().unwrap().len(), 2);
    assert_eq!(app.console().editor.ghosts().len(), 2);
    // Tab decides the ghost at the line of the cursor, and the other ghost waits.
    app.on_event(Event::Key(key(KeyCode::Tab)));
    assert_eq!(app.console().editor.text(), "SELECT \nFROM t42");
    assert_eq!(app.console().editor.ghosts().len(), 1);
}

#[test]
fn a_new_request_on_the_line_of_a_ghost_replaces_it() {
    let server = Server::new("7", Duration::ZERO);
    let mut app = app("ai-replace-line", server.provider());
    ghost_at_first_line(&mut app, "SELECT ");
    request(&mut app);
    pump(&mut app, |s| s.contains("SELECT 7"));
    assert_eq!(app.console().editor.ghosts().len(), 1);
    app.on_event(Event::Key(key(KeyCode::Tab)));
    assert_eq!(app.console().editor.text(), "SELECT 7");
}

#[test]
fn the_tab_mark_shows_a_pending_ghost() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-mark", server.provider());
    ghost_at_first_line(&mut app, "SELECT ");
    assert!(screen(&mut app).contains("console-1\u{b1}1"));
    app.on_event(Event::Key(key(KeyCode::Tab)));
    assert!(!screen(&mut app).contains("console-1\u{b1}1"));
}
/// Clicks the `nth` occurrence of a label on the screen with the left button.
fn click_nth(app: &mut App, label: &str, nth: usize) {
    let shown = rows(&screen(app));
    let places: Vec<_> = shown
        .lines()
        .enumerate()
        .flat_map(|(y, line)| {
            line.match_indices(label)
                .map(move |(byte, _)| (y, byte))
                .collect::<Vec<_>>()
        })
        .collect();
    let &(y, byte) = places
        .get(nth)
        .unwrap_or_else(|| panic!("Missing {label} #{nth}:\n{shown}"));
    let x = UnicodeWidthStr::width(&shown.lines().nth(y).unwrap()[..byte]) + 1;
    app.on_event(Event::Mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: x as u16,
        row: y as u16,
        modifiers: KeyModifiers::NONE,
    }));
}

#[test]
fn the_anchor_of_a_running_request_shows_in_the_gutter() {
    let server = Server::new("42", Duration::from_millis(600));
    let mut app = app("ai-anchor-mark", server.provider());
    app.on_event(Event::Paste("SELECT \nFROM t".into()));
    app.console_mut().editor.set_cursor(Index2::new(0, 7));
    request(&mut app);
    approve(&mut app);
    pump(&mut app, |s| s.contains("AI thinking"));
    // The requested line shows the running mark while the user edits another line.
    app.console_mut().editor.set_cursor(Index2::new(1, 6));
    app.on_event(Event::Key(key(KeyCode::Char('x'))));
    let shown = rows(&screen(&mut app));
    let anchor = shown
        .lines()
        .find(|line| line.contains("SELECT"))
        .expect("anchor line");
    assert!(
        ["|1", "/1", "-1", "\\1"]
            .iter()
            .any(|mark| anchor.contains(mark)),
        "{shown}"
    );
    // The ghost replaces the mark when the answer shows.
    let shown = rows(&pump(&mut app, ghost));
    let line = shown
        .lines()
        .find(|line| line.contains("SELECT 42"))
        .expect("ghost line");
    assert!(
        !["|1", "/1", "-1", "\\1"]
            .iter()
            .any(|mark| line.contains(mark)),
        "{shown}"
    );
}

#[test]
fn typing_during_a_request_discards_the_late_response() {
    let server = Server::new("42", Duration::from_millis(600));
    let mut app = app("ai-stale", server.provider());
    app.on_event(Event::Paste("SELECT ".into()));
    request(&mut app);
    approve(&mut app);
    pump(&mut app, |s| s.contains("AI thinking"));
    app.on_event(Event::Paste("1".into()));
    // The request ends in the same event; it does not wait for the answer.
    let shown = rows(&screen(&mut app));
    assert!(!shown.contains("AI thinking"), "{shown}");
    std::thread::sleep(Duration::from_millis(700));
    for _ in 0..8 {
        app.drain_events();
    }
    assert!(!ghost(&screen(&mut app)));
    assert_eq!(app.console().editor.text(), "SELECT 1");
}

#[test]
fn an_edit_above_keeps_the_request_and_moves_the_answer() {
    let server = Server::new("42", Duration::from_millis(200));
    let mut app = app("ai-move", server.provider());
    app.on_event(Event::Paste("SELECT \nFROM t".into()));
    app.console_mut().editor.set_cursor(Index2::new(0, 7));
    request(&mut app);
    approve(&mut app);
    pump(&mut app, |s| s.contains("AI thinking"));
    // An edit on another line moves the requested line; the answer follows it.
    app.on_event(Event::Key(key(KeyCode::Home)));
    app.on_event(Event::Key(key(KeyCode::Enter)));
    let shown = rows(&pump(&mut app, ghost));
    assert!(shown.contains("2   SELECT 42"), "{shown}");
}
#[tokio::test]
async fn client_uses_chat_without_sending_keys_in_the_body() {
    let server = Server::new("42", Duration::ZERO);
    let key = sqln::connector::target::Secret::new("test-secret");
    let cancel = tokio_util::sync::CancellationToken::new();
    let provider = server.provider();
    let prepared = ai::prepare(&provider, &context(), &snapshot(Vec::new())).unwrap();
    assert_eq!(
        ai::client::complete(&provider, &prepared, Some(&key), "", &cancel)
            .await
            .unwrap(),
        "42"
    );
    let requests = server.requests.lock().unwrap();
    assert!(requests[0].starts_with("POST /v1/chat/completions"));
    for request in requests.iter() {
        let (_, body) = request.split_once("\r\n\r\n").unwrap();
        assert!(!body.contains("test-secret"));
        let _: Value = serde_json::from_str(body).unwrap();
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer test-secret")
        );
    }
}

#[tokio::test]
async fn http_failures_hide_external_text_and_never_retry() {
    for status in [302, 401, 403, 429, 500] {
        let server = Server::response(status, "secret_sql_and_api_key".into(), Duration::ZERO);
        let provider = server.provider();
        let prepared = ai::prepare(&provider, &context(), &snapshot(Vec::new())).unwrap();
        let error = ai::client::complete(
            &provider,
            &prepared,
            None,
            "",
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert!(!error.contains("secret_sql_and_api_key"));
        assert_eq!(
            server.requests.lock().unwrap().len(),
            1,
            "status {status}: {error}"
        );
    }
}
#[tokio::test]
async fn invalid_and_excessive_http_bodies_fail_safely() {
    for body in [
        "invalid secret JSON".to_owned(),
        "x".repeat(ai::client::RESPONSE_LIMIT + 1),
    ] {
        let server = Server::response(200, body, Duration::ZERO);
        let provider = server.provider();
        let prepared = ai::prepare(&provider, &context(), &snapshot(Vec::new())).unwrap();
        let error = ai::client::complete(
            &provider,
            &prepared,
            None,
            "",
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert!(!error.contains("secret JSON"));
    }
}
#[tokio::test]
async fn a_cancelled_http_request_does_not_produce_a_proposal() {
    let server = Server::new("42", Duration::ZERO);
    let provider = server.provider();
    let prepared = ai::prepare(&provider, &context(), &snapshot(Vec::new())).unwrap();
    let cancel = tokio_util::sync::CancellationToken::new();
    cancel.cancel();
    assert!(
        ai::client::complete(&provider, &prepared, None, "", &cancel)
            .await
            .is_err()
    );
}
#[test]
fn key_files_reject_invalid_content_and_support_rotating_symlinks() {
    let dir = common::temp_dir("ai-rotating-key");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("key");
    for bytes in [vec![], b"key\nother".to_vec(), vec![255], vec![b'x'; 65537]] {
        std::fs::write(&path, bytes).unwrap();
        assert!(ai::credentials::read_file(&path).is_err());
    }
    assert!(ai::credentials::read_file(&dir).is_err());
    #[cfg(unix)]
    {
        let first = dir.join("first");
        let second = dir.join("second");
        let link = dir.join("current");
        std::fs::write(&first, "first").unwrap();
        std::fs::write(&second, "second").unwrap();
        std::os::unix::fs::symlink(&first, &link).unwrap();
        assert_eq!(ai::credentials::read_file(&link).unwrap().expose(), "first");
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&second, &link).unwrap();
        assert_eq!(
            ai::credentials::read_file(&link).unwrap().expose(),
            "second"
        );
    }
}
#[test]
fn api_vault_keys_use_an_isolated_service_and_endpoint_identity() {
    let store = keyring_core::mock::Store::new().unwrap();
    let vault = sqln::creds::SystemVault::with_store(store);
    let first = ai::credentials::account("global", "key-id", "https://example.com/v1");
    let second = ai::credentials::account("project:/other", "key-id", "https://example.com/v1");
    vault
        .write_api_key(&first, &sqln::connector::target::Secret::new("api-key"))
        .unwrap();
    assert_eq!(
        vault.read_api_key(&first).unwrap().unwrap().expose(),
        "api-key"
    );
    assert!(vault.read_api_key(&second).unwrap().is_none());
}
#[test]
fn consent_is_private_and_changes_with_project_endpoint_or_key_reference() {
    let root = common::temp_dir("ai-consent");
    let dest = ai::trust::Destination {
        project: Some(root.join("project")),
        connection: "connection-id".into(),
        endpoint: "https://example.com/v1".into(),
        credential: "key-reference".into(),
    };
    assert!(!ai::trust::approved(&root, &dest).unwrap());
    ai::trust::confirm(&root, dest.clone()).unwrap();
    assert!(ai::trust::approved(&root, &dest).unwrap());
    let mut changed = dest.clone();
    changed.endpoint = "https://other.example/v1".into();
    assert!(!ai::trust::approved(&root, &changed).unwrap());
    changed = dest.clone();
    changed.credential = "new-reference".into();
    assert!(!ai::trust::approved(&root, &changed).unwrap());
    changed = dest;
    changed.project = Some(root.join("moved"));
    assert!(!ai::trust::approved(&root, &changed).unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(root.join("ai-trust.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
/// A settings form for a project. `project_ai` is the AI block of the project;
/// `None` inherits the global block.
fn ai_settings(
    global_ai: Preferences,
    project_ai: Option<Preferences>,
) -> sqln::settings::SettingsForm {
    let global = sqln::store::config::Preferences {
        ai_completion: Some(global_ai),
        ..Default::default()
    };
    let project = sqln::store::config::Preferences {
        ai_completion: project_ai,
        ..Default::default()
    };
    sqln::settings::SettingsForm::new(global, Some(project))
}

#[test]
fn settings_keep_text_keys_as_text_and_allow_project_disable_with_inheritance() {
    let global = Preferences {
        enabled: Some(true),
        max_simultaneous: None,
        provider: Some(Provider::default()),
    };
    let local = Preferences {
        enabled: Some(false),
        max_simultaneous: None,
        provider: None,
    };
    let mut form = ai_settings(global, Some(local.clone()));
    // The simultaneous count writes its effective value, so the fields pin it.
    let mut expected = local;
    expected.max_simultaneous = Some(3);
    assert_eq!(form.ai_preferences().unwrap(), expected);
    form.focus_on(sqln::settings::Field::AiModel);
    form.on_key(key(KeyCode::Char('j')), false);
    form.on_key(key(KeyCode::Char('k')), false);
    assert!(
        !form.ai_inherit(),
        "Typing makes the provider block custom."
    );
    assert_eq!(form.ai_preferences().unwrap().provider.unwrap().model, "jk");
}
#[test]
fn a_warmed_schema_stays_cached_until_refresh_then_contains_external_ddl() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-cache", server.provider());
    app.on_event(Event::Paste("SELECT ".into()));
    request(&mut app);
    approve(&mut app);
    pump(&mut app, ghost);
    app.on_event(Event::Key(key(KeyCode::Esc)));
    let path = common::temp_db_path("ai-cache");
    rusqlite::Connection::open(path)
        .unwrap()
        .execute_batch("CREATE TABLE later_object(id INTEGER)")
        .unwrap();
    request(&mut app);
    pump(&mut app, ghost);
    app.on_event(Event::Key(key(KeyCode::Esc)));
    assert!(!server.requests.lock().unwrap()[1].contains("later_object"));
    app.on_event(Event::Key(ctrl('w')));
    app.on_event(Event::Key(key(KeyCode::Char('h'))));
    for _ in 0..20 {
        app.on_event(Event::Key(key(KeyCode::Up)));
    }
    app.on_event(Event::Key(key(KeyCode::Char('r'))));
    app.on_event(Event::Key(ctrl('w')));
    app.on_event(Event::Key(key(KeyCode::Char('l'))));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.drain_events();
        if app.connections()[0].catalog_state == sqln::connection::CatalogState::Loaded
            && app.connections()[0].catalog.as_ref().is_some_and(|c| {
                c.schemas
                    .iter()
                    .any(|s| s.relations.iter().any(|r| r.name == "later_object"))
            })
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    request(&mut app);
    pump(&mut app, ghost);
    let requests = server.requests.lock().unwrap();
    assert!(
        requests.last().unwrap().contains("later_object"),
        "{} requests; fresh catalog: {:?}",
        requests.len(),
        app.connections()[0].catalog_state
    );
}
#[test]
fn an_open_palette_keeps_focus_when_a_response_arrives() {
    let server = Server::new("42", Duration::from_millis(150));
    let mut app = app("ai-modal", server.provider());
    app.on_event(Event::Paste("SELECT ".into()));
    request(&mut app);
    approve(&mut app);
    pump(&mut app, |s| s.contains("AI thinking"));
    app.on_event(Event::Key(ctrl('p')));
    assert!(
        screen(&mut app).contains("Commands"),
        "{}",
        screen(&mut app)
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !app.console().editor.has_ghosts() {
        assert!(Instant::now() < deadline);
        app.drain_events();
        std::thread::sleep(Duration::from_millis(2));
    }
    // The ghost text waits in the editor. The palette keeps the keys.
    app.on_event(Event::Key(key(KeyCode::Char('x'))));
    assert!(screen(&mut app).contains("Commands"));
    app.on_event(Event::Key(key(KeyCode::Esc)));
    assert!(ghost(&screen(&mut app)));
    assert_eq!(app.console().editor.text(), "SELECT ");
}
#[tokio::test]
async fn full_completion_urls_are_not_appended_twice() {
    // The legacy FIM route of an old file maps to the chat route on the same base.
    for (configured, expected) in [
        ("/chat/completions/", "/v1/chat/completions"),
        ("/completions/", "/v1/chat/completions"),
    ] {
        let server = Server::new("name", Duration::ZERO);
        let mut provider = server.provider();
        provider.base_url = format!("{}{configured}", server.url);
        let prepared = ai::prepare(&provider, &context(), &snapshot(Vec::new())).unwrap();
        let key = sqln::connector::target::Secret::new("fixture-key");
        ai::client::complete(
            &provider,
            &prepared,
            Some(&key),
            "",
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
        ai::client::test(
            &provider,
            Some(&key),
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
        let requests = server.requests.lock().unwrap();
        assert!(
            requests[0].starts_with(&format!("POST {expected} HTTP/")),
            "The client must use the configured completion URL exactly."
        );
        assert!(
            requests[1].starts_with("GET /v1/models HTTP/"),
            "The connection test must use the API base, not the completion route."
        );
    }
}
#[test]
fn the_default_endpoint_and_legacy_urls_keep_one_chat_route() {
    // An empty endpoint uses the OpenRouter base.
    let mut provider = Provider {
        model: "fixture".into(),
        ..Provider::default()
    };
    assert_eq!(
        provider.completion_url().unwrap(),
        "https://openrouter.ai/api/v1/chat/completions"
    );
    assert_eq!(
        provider.models_url().unwrap(),
        "https://openrouter.ai/api/v1/models"
    );
    // A full completion URL of an old file keeps its identity and uses the chat route.
    provider.base_url = "https://example.com/v1/completions/".into();
    let identity = provider.endpoint().unwrap();
    provider.credential = Credential::Vault {
        id: "fixture-reference".into(),
        endpoint: identity.clone(),
    };
    provider.validate().unwrap();
    assert_eq!(
        provider.completion_url().unwrap(),
        "https://example.com/v1/chat/completions"
    );
    assert_eq!(
        provider.endpoint().unwrap(),
        identity,
        "URL routing must not change saved credential identities."
    );
}
#[test]
fn pasting_provider_fields_creates_a_project_override() {
    let server = Server::new("name", Duration::ZERO);
    let global = Preferences {
        enabled: Some(true),
        max_simultaneous: None,
        provider: Some(Provider {
            model: String::new(),
            ..server.provider()
        }),
    };
    let mut form = ai_settings(global, None);
    assert!(form.ai_inherit());
    form.focus_on(sqln::settings::Field::AiModel);
    form.paste("deepseek-flash");
    assert!(
        !form.ai_inherit(),
        "Pasting must have the same override behavior as typing."
    );
    assert_eq!(
        form.ai_preferences().unwrap().provider.unwrap().model,
        "deepseek-flash"
    );
}

#[test]
fn the_limit_of_simultaneous_answers_blocks_new_requests() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-limit", server.provider());
    app.on_event(Event::Paste("a\nb\nc\nd".into()));
    for (row, letter) in [(0, 'a'), (1, 'b'), (2, 'c')] {
        app.console_mut().editor.set_cursor(Index2::new(row, 1));
        request(&mut app);
        if row == 0 {
            approve(&mut app);
        }
        let answer = format!("{letter}42");
        pump(&mut app, |s| s.contains(&answer));
    }
    assert_eq!(app.console().editor.ghosts().len(), 3);
    // The fourth request on a free line hits the default limit of three.
    app.console_mut().editor.set_cursor(Index2::new(3, 1));
    request(&mut app);
    let shown = screen(&mut app);
    assert!(shown.contains("limit reached (3)"), "{shown}");
    // A request on a line with a ghost replaces it, so the limit does not block it.
    // The waiting bar of the request leaves the screen when its answer lands.
    app.console_mut().editor.set_cursor(Index2::new(0, 1));
    request(&mut app);
    pump(&mut app, |s| !s.contains("Esc: Cancel"));
    assert_eq!(app.console().editor.ghosts().len(), 3);
    // A decision frees a place, and a new line can ask again.
    app.console_mut().editor.set_cursor(Index2::new(1, 1));
    app.on_event(Event::Key(key(KeyCode::Tab)));
    assert_eq!(app.console().editor.ghosts().len(), 2);
    app.console_mut().editor.set_cursor(Index2::new(3, 1));
    request(&mut app);
    pump(&mut app, |s| !s.contains("Esc: Cancel"));
    assert_eq!(app.console().editor.ghosts().len(), 3);
}

#[test]
fn esc_ends_only_the_run_at_the_line_of_the_cursor() {
    let server = Server::new("42", Duration::from_millis(600));
    let mut app = app("ai-esc", server.provider());
    app.on_event(Event::Paste("SELECT \nFROM t".into()));
    app.console_mut().editor.set_cursor(Index2::new(0, 7));
    request(&mut app);
    approve(&mut app);
    // The first request reaches the server before the second one: its approval
    // covers the same destination, so the second request asks no consent again.
    pump(&mut app, |s| s.contains("AI thinking"));
    app.console_mut().editor.set_cursor(Index2::new(1, 6));
    request(&mut app);
    pump(&mut app, |s| s.contains("×2"));
    // Esc ends only the run of the cursor line, so that answer never shows.
    app.on_event(Event::Key(key(KeyCode::Esc)));
    let shown = screen(&mut app);
    assert!(!shown.contains("×2"), "{shown}");
    pump(&mut app, |s| s.contains("SELECT 42"));
    assert_eq!(app.console().editor.ghosts().len(), 1);
    assert_eq!(app.console().editor.text(), "SELECT \nFROM t");
}

#[test]
fn the_tab_mark_counts_the_ghosts_that_wait() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-mark-count", server.provider());
    app.on_event(Event::Paste("SELECT \nFROM t".into()));
    app.console_mut().editor.set_cursor(Index2::new(0, 7));
    request(&mut app);
    approve(&mut app);
    pump(&mut app, ghost);
    app.console_mut().editor.set_cursor(Index2::new(1, 6));
    request(&mut app);
    pump(&mut app, |s| s.contains("FROM t42"));
    assert!(screen(&mut app).contains("console-1\u{b1}2"));
}

#[test]
fn each_ghost_keeps_its_own_buttons() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-two-buttons", server.provider());
    app.on_event(Event::Paste("SELECT \nFROM t".into()));
    app.console_mut().editor.set_cursor(Index2::new(0, 7));
    request(&mut app);
    approve(&mut app);
    pump(&mut app, ghost);
    app.console_mut().editor.set_cursor(Index2::new(1, 6));
    request(&mut app);
    let shown = rows(&pump(&mut app, |s| s.contains("FROM t42")));
    assert_eq!(shown.matches("[Tab Accept]").count(), 2, "{shown}");
    // The reject button of the second ghost decides only that ghost.
    click_nth(&mut app, "[Esc Reject]", 1);
    assert_eq!(app.console().editor.ghosts().len(), 1);
    assert_eq!(app.console().editor.text(), "SELECT \nFROM t");
}

#[test]
fn a_selection_refuses_ai_completion_and_keeps_the_selection() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-selection-refused", server.provider());
    app.on_event(Event::Paste("SELECT x".into()));
    app.console_mut().editor.set_cursor(Index2::new(0, 7));
    app.on_event(Event::Key(shift(KeyCode::Right)));
    let selected = app.console().editor.selected_text();
    // The palette command refuses: the completion works at the cursor only.
    app.on_event(Event::Key(ctrl('p')));
    for c in "complete with".chars() {
        app.on_event(Event::Key(key(KeyCode::Char(c))));
    }
    app.on_event(Event::Key(key(KeyCode::Enter)));
    let shown = screen(&mut app);
    assert!(
        shown.contains("AI completion works at the cursor"),
        "{shown}"
    );
    // The selection stays, and no answer starts.
    assert_eq!(app.console().editor.selected_text(), selected);
    assert!(!app.console().editor.has_ghosts());
}

#[test]
fn an_explicit_list_hides_the_ai_row_under_a_selection() {
    let mut f = common::Fixture::conventional("SELECT name FROM users");
    f.editor.set_ai_available(true);
    // Without a selection, the explicit list carries the AI row.
    f.press(ctrl(' '));
    assert!(f.editor.suggestions().is_some_and(|l| l.ai));
    f.press(key(KeyCode::Esc));
    // With a selection, the local list shows and the AI row hides.
    f.editor.set_cursor(Index2::new(0, 20));
    f.press(shift(KeyCode::Right));
    f.press(ctrl(' '));
    assert!(f.editor.suggestions().is_some_and(|l| !l.ai));
}

#[test]
fn legacy_provider_fields_of_old_files_stay_valid() {
    let file = r#"{"kind":"deepseek","model":"m","baseUrl":"https://example.com/v1/completions","protocol":"fim"}"#;
    let provider: Provider = serde_json::from_str(file).unwrap();
    assert_eq!(
        provider.completion_url().unwrap(),
        "https://example.com/v1/chat/completions"
    );
    let saved = serde_json::to_string(&provider).unwrap();
    assert!(
        !saved.contains("protocol"),
        "New saves omit the legacy field."
    );
}

#[test]
fn the_reasoning_effort_appears_in_the_body_only_when_it_is_on() {
    let server = Server::new("1", Duration::ZERO);
    let p = server.provider();
    let ctx = context();
    let prepared = ai::prepare(&p, &ctx, &snapshot(Vec::new())).unwrap();
    assert!(
        prepared.body.get("reasoning_effort").is_none(),
        "The default provider sends no reasoning field."
    );
    assert_eq!(prepared.body["max_completion_tokens"], p.max_output_tokens);
    assert!(
        prepared.body.get("max_tokens").is_none(),
        "The body uses the current completion field, not the deprecated one."
    );
    let mut p = p;
    p.reasoning_effort = ai::Reasoning::High;
    let prepared = ai::prepare(&p, &ctx, &snapshot(Vec::new())).unwrap();
    assert_eq!(prepared.body["reasoning_effort"], "high");
    p.reasoning_effort = ai::Reasoning::None;
    let prepared = ai::prepare(&p, &ctx, &snapshot(Vec::new())).unwrap();
    assert_eq!(prepared.body["reasoning_effort"], "none");
}

#[test]
fn the_reasoning_effort_of_old_files_stays_off_when_unknown() {
    let provider: Provider = serde_json::from_str(r#"{"kind":"compatible","model":"m"}"#).unwrap();
    assert_eq!(provider.reasoning_effort, ai::Reasoning::Off);
    let saved = serde_json::to_string(&provider).unwrap();
    assert!(!saved.contains("reasoning"), "Off stays out of the file.");
    let unknown: Provider =
        serde_json::from_str(r#"{"kind":"compatible","model":"m","reasoningEffort":"minimal"}"#)
            .unwrap();
    assert_eq!(unknown.reasoning_effort, ai::Reasoning::Off);
    let set: Provider =
        serde_json::from_str(r#"{"kind":"compatible","model":"m","reasoningEffort":"none"}"#)
            .unwrap();
    assert_eq!(set.reasoning_effort, ai::Reasoning::None);
    let set: Provider =
        serde_json::from_str(r#"{"kind":"compatible","model":"m","reasoningEffort":"high"}"#)
            .unwrap();
    assert_eq!(set.reasoning_effort, ai::Reasoning::High);
}

#[test]
fn cycling_the_reasoning_effort_makes_the_block_custom_and_saves_the_value() {
    let global = Preferences {
        enabled: Some(true),
        max_simultaneous: None,
        provider: Some(Provider {
            model: "test-model".into(),
            ..Provider::default()
        }),
    };
    let mut form = ai_settings(global, None);
    assert!(form.ai_inherit());
    form.focus_on(sqln::settings::Field::AiReasoning);
    form.on_key(key(KeyCode::Right), false);
    assert!(
        !form.ai_inherit(),
        "The reasoning effort belongs to the provider block."
    );
    assert_eq!(form.ai_reasoning(), 1);
    assert_eq!(
        form.ai_preferences()
            .unwrap()
            .provider
            .unwrap()
            .reasoning_effort,
        ai::Reasoning::None
    );
    form.on_key(key(KeyCode::Right), false);
    assert_eq!(
        form.ai_preferences()
            .unwrap()
            .provider
            .unwrap()
            .reasoning_effort,
        ai::Reasoning::Low
    );
    for _ in 0..2 {
        form.on_key(key(KeyCode::Right), false);
    }
    let provider = form.ai_preferences().unwrap().provider.unwrap();
    assert_eq!(provider.reasoning_effort, ai::Reasoning::High);
    let saved = serde_json::to_string(&provider).unwrap();
    assert!(saved.contains("reasoningEffort\":\"high\""));
    // Four back steps return to Disabled, and the file keeps no field.
    for _ in 0..4 {
        form.on_key(key(KeyCode::Left), false);
    }
    assert_eq!(form.ai_reasoning(), 0);
    let provider = form.ai_preferences().unwrap().provider.unwrap();
    let saved = serde_json::to_string(&provider).unwrap();
    assert!(!saved.contains("reasoning"));
}

/// Asks the open palette for the technical name of the action.
fn palette_has(app: &mut App, action: Action) -> bool {
    app.drain_events();
    app.on_event(Event::Key(ctrl('p')));
    app.on_event(Event::Paste(action.name().to_owned()));
    let shown = screen(app);
    let found = shown.contains(text::TITLE_PALETTE) && !shown.contains(text::NO_COMMANDS);
    app.on_event(Event::Key(key(KeyCode::Esc)));
    found
}

#[test]
fn the_palette_offers_every_command_of_the_ai_completion() {
    // The editor with text: the two ways to ask for a completion.
    let server = Server::new("42", Duration::ZERO);
    let mut first = app("ai-palette-all", server.provider());
    first.on_event(Event::Paste("SELECT ".into()));
    for action in [Action::EditorAiComplete, Action::EditorAiCompleteRequest] {
        assert!(palette_has(&mut first, action), "{action:?} missing");
    }
    // A completion that waits: the cancel of the run, then its ghost.
    let slow = Server::new("42", Duration::from_millis(400));
    let mut app = app("ai-palette-run", slow.provider());
    app.on_event(Event::Paste("SELECT ".into()));
    request(&mut app);
    approve(&mut app);
    pump(&mut app, |s| s.contains("AI thinking"));
    assert!(palette_has(&mut app, Action::EditorAiCancel), "missing");
    pump(&mut app, ghost);
    for action in [Action::EditorAiAccept, Action::EditorAiReject] {
        assert!(palette_has(&mut app, action), "{action:?} missing");
    }
}
