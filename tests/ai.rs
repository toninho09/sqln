mod common;

use common::{ctrl, key, shift};
use crossterm::event::{Event, KeyCode};
use edtui::Index2;
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};
use sqln::{
    ai::{self, Context, Credential, Kind, Object, Preferences, Protocol, Provider, Snapshot},
    app::{App, Options},
    clipboard::MemoryClipboard,
    editor::KeyboardMode,
    grid::Budget,
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
            protocol: Protocol::Chat,
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
                provider: Some(provider),
            },
            ..Options::default()
        },
        Rc::new(RefCell::new(MemoryClipboard::default())),
        Theme::plain(),
    )
}
fn request(app: &mut App) {
    app.on_event(Event::Key(ctrl(' ')));
    let list = app.console().editor.suggestions().unwrap();
    if !list.completion.items.is_empty() {
        app.on_event(Event::Key(key(KeyCode::Up)));
    }
    app.on_event(Event::Key(key(KeyCode::Tab)));
}
fn approve(app: &mut App) {
    pump(app, |s| s.contains("Send SQL and schema"));
    app.on_event(Event::Key(key(KeyCode::Tab)));
    app.on_event(Event::Key(key(KeyCode::Enter)));
}

#[test]
fn ai_is_above_local_items_but_not_initially_selected() {
    let mut f = common::Fixture::conventional("sel");
    f.editor.set_cursor(Index2::new(0, 3));
    f.editor.set_ai_available(true);
    f.press(ctrl(' '));
    assert_eq!(f.editor.suggestions().unwrap().selected, 0);
    f.press(key(KeyCode::Up));
    assert_eq!(f.editor.suggestions().unwrap().selected, usize::MAX);
    f.press(key(KeyCode::Tab));
    assert!(f.editor.take_ai_request());
    assert_eq!(f.text(), "sel");
}
#[test]
fn explicit_ai_only_list_handles_repeated_arrows_without_panics() {
    let mut f = common::Fixture::conventional("-- describe a query");
    f.editor.set_cursor(Index2::new(0, 19));
    f.editor.set_ai_available(true);
    f.press(ctrl(' '));
    for _ in 0..5 {
        f.press(key(KeyCode::Up));
        f.press(key(KeyCode::Down));
    }
    assert_eq!(f.editor.suggestions().unwrap().selected, usize::MAX);
    f.press(key(KeyCode::Tab));
    assert!(f.editor.take_ai_request());
}
#[test]
fn typing_no_local_match_does_not_open_an_ai_only_list() {
    let mut f = common::Fixture::conventional("");
    f.editor.set_ai_available(true);
    f.write("zzzzzz");
    assert!(f.editor.suggestions().is_none());
}
#[test]
fn target_replacement_preserves_unicode_and_is_one_undo_step() {
    for keyboard in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut f = common::Fixture::new("SELECT café;", keyboard);
        if keyboard == KeyboardMode::Vim {
            f.press(key(KeyCode::Char('i')));
        }
        f.editor.set_cursor(Index2::new(0, 7));
        if keyboard == KeyboardMode::Conventional {
            for _ in 0..4 {
                f.press(shift(KeyCode::Right));
            }
        }
        let target = f.editor.ai_target(None);
        f.editor.apply_ai(&target, "42".into()).unwrap();
        f.editor.on_command(sqln::input::Action::EditorUndo, None);
        assert_eq!(f.text(), "SELECT café;");
    }
}
#[test]
fn old_cursor_and_revision_refuse_application() {
    let mut f = common::Fixture::conventional("SELECT ");
    let target = f.editor.ai_target(None);
    f.press(key(KeyCode::Right));
    f.write("x");
    assert!(f.editor.apply_ai(&target, "1".into()).is_err());
}
#[test]
fn protocols_preserve_suffix_and_original_selection() {
    let server = Server::new("1", Duration::ZERO);
    let mut p = server.provider();
    let mut ctx = context();
    ctx.original = "*".into();
    let prepared = ai::prepare(&p, &ctx, &snapshot(Vec::new())).unwrap();
    let user = prepared.body["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("FROM users"));
    assert!(user.contains("original"));
    p.protocol = Protocol::Fim;
    let prepared = ai::prepare(&p, &ctx, &snapshot(Vec::new())).unwrap();
    assert_eq!(prepared.body["suffix"], ctx.suffix);
    assert!(
        prepared.body["prompt"]
            .as_str()
            .unwrap()
            .ends_with("SELECT ")
    );
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
        provider: Some(Provider::default()),
    };
    let project = Preferences {
        enabled: Some(false),
        provider: None,
    };
    let merged = Preferences::resolve(Some(&global), Some(&project));
    assert!(!merged.enabled());
    assert_eq!(merged.provider, global.provider);
    let old: sqln::store::config::Preferences = serde_json::from_str("{}").unwrap();
    assert!(old.ai_completion.is_none());
}
#[test]
fn user_accepts_a_preview_without_executing_sql() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-insert", server.provider());
    app.on_event(Event::Paste("SELECT ".into()));
    request(&mut app);
    approve(&mut app);
    pump(&mut app, |s| s.contains("AI insertion preview"));
    assert_eq!(app.console().editor.text(), "SELECT ");
    assert!(app.console().results.is_empty());
    app.on_event(Event::Key(key(KeyCode::Tab)));
    app.on_event(Event::Key(key(KeyCode::Enter)));
    assert_eq!(app.console().editor.text(), "SELECT 42");
    assert!(app.console().results.is_empty());
    app.on_event(Event::Key(ctrl('z')));
    assert_eq!(app.console().editor.text(), "SELECT ");
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("active_users"));
    assert!(requests[0].contains("nullable"));
}
#[test]
fn a_selection_opens_a_diff_and_rejection_keeps_the_selection() {
    let server = Server::new("name", Duration::ZERO);
    let mut app = app("ai-select", server.provider());
    app.on_event(Event::Paste("SELECT * FROM users;".into()));
    app.console_mut().editor.set_cursor(Index2::new(0, 7));
    app.on_event(Event::Key(shift(KeyCode::Right)));
    let selected = app.console().editor.selected_text();
    request(&mut app);
    approve(&mut app);
    pump(&mut app, |s| s.contains("AI replacement"));
    app.on_event(Event::Key(key(KeyCode::F(5))));
    assert!(!app.console().is_busy());
    app.on_event(Event::Mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 27,
        row: 1,
        modifiers: crossterm::event::KeyModifiers::NONE,
    }));
    assert!(!app.console().is_busy());
    assert!(screen(&mut app).contains("AI replacement"));
    app.on_event(Event::Key(key(KeyCode::Esc)));
    assert_eq!(app.console().editor.text(), "SELECT * FROM users;");
    assert_eq!(app.console().editor.selected_text(), selected);
}
#[test]
fn typing_during_a_request_discards_the_late_response() {
    let server = Server::new("42", Duration::from_millis(150));
    let mut app = app("ai-stale", server.provider());
    app.on_event(Event::Paste("SELECT ".into()));
    request(&mut app);
    approve(&mut app);
    pump(&mut app, |s| s.contains("AI thinking"));
    app.on_event(Event::Paste("1".into()));
    std::thread::sleep(Duration::from_millis(250));
    for _ in 0..8 {
        app.drain_events();
    }
    assert!(!screen(&mut app).contains("AI insertion preview"));
    assert_eq!(app.console().editor.text(), "SELECT 1");
}
#[tokio::test]
async fn client_uses_fim_and_chat_without_sending_keys_in_the_body() {
    let server = Server::new("42", Duration::ZERO);
    let key = sqln::connector::target::Secret::new("test-secret");
    let cancel = tokio_util::sync::CancellationToken::new();
    for protocol in [Protocol::Chat, Protocol::Fim] {
        let mut provider = server.provider();
        provider.protocol = protocol;
        let prepared = ai::prepare(&provider, &context(), &snapshot(Vec::new())).unwrap();
        assert_eq!(
            ai::client::complete(&provider, &prepared, Some(&key), "", &cancel)
                .await
                .unwrap(),
            "42"
        );
    }
    let requests = server.requests.lock().unwrap();
    assert!(requests[0].starts_with("POST /v1/chat/completions"));
    assert!(requests[1].starts_with("POST /v1/completions"));
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
#[test]
fn settings_keep_text_keys_as_text_and_allow_project_disable_with_inheritance() {
    use ai::form::{Field, Form};
    let global = Preferences {
        enabled: Some(true),
        provider: Some(Provider::default()),
    };
    let local = Preferences {
        enabled: Some(false),
        provider: None,
    };
    let mut form = Form::new(sqln::store::Scope::Project, Some(&local), Some(&global));
    assert_eq!(form.preferences().unwrap(), local);
    form.focus = Field::Model;
    form.on_key(key(KeyCode::Char('j')));
    form.on_key(key(KeyCode::Char('k')));
    assert_eq!(form.model.text(), "jk");
    assert_eq!(form.focus, Field::Model);
}
#[test]
fn a_warmed_schema_stays_cached_until_refresh_then_contains_external_ddl() {
    let server = Server::new("42", Duration::ZERO);
    let mut app = app("ai-cache", server.provider());
    app.on_event(Event::Paste("SELECT ".into()));
    request(&mut app);
    approve(&mut app);
    pump(&mut app, |s| s.contains("AI insertion preview"));
    app.on_event(Event::Key(key(KeyCode::Esc)));
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("ai-cache.db");
    rusqlite::Connection::open(path)
        .unwrap()
        .execute_batch("CREATE TABLE later_object(id INTEGER)")
        .unwrap();
    request(&mut app);
    pump(&mut app, |s| s.contains("AI insertion preview"));
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
    pump(&mut app, |s| s.contains("AI insertion preview"));
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
    std::thread::sleep(Duration::from_millis(250));
    for _ in 0..8 {
        app.drain_events();
    }
    assert!(!screen(&mut app).contains("AI insertion preview"));
    app.on_event(Event::Key(key(KeyCode::Esc)));
    pump(&mut app, |s| s.contains("AI insertion preview"));
    assert_eq!(app.console().editor.text(), "SELECT ");
}
#[test]
fn long_preview_lines_wrap_and_remain_readable_after_resize() {
    let response = format!("{}END_OF_PROPOSAL", "long_column ".repeat(100));
    let server = Server::new(&response, Duration::ZERO);
    let mut app = app("ai-wrap", server.provider());
    app.on_event(Event::Paste("SELECT ".into()));
    request(&mut app);
    approve(&mut app);
    pump(&mut app, |s| s.contains("AI insertion preview"));
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    for _ in 0..20 {
        app.on_event(Event::Key(key(KeyCode::PageDown)));
        screen(&mut app);
    }
    assert!(screen(&mut app).contains("END_OF_PROPOSAL"));
    assert_eq!(app.console().editor.text(), "SELECT ");
}

#[tokio::test]
async fn full_completion_urls_are_not_appended_twice() {
    for protocol in [Protocol::Fim, Protocol::Chat] {
        let server = Server::new("name", Duration::ZERO);
        let mut provider = server.provider();
        provider.model = "deepseek-flash".into();
        provider.protocol = protocol;
        let route = match protocol {
            Protocol::Fim => "completions",
            Protocol::Chat => "chat/completions",
        };
        provider.base_url = format!("{}/{route}/", server.url);
        if protocol == Protocol::Fim {
            provider.kind = Kind::Deepseek;
            provider.credential = Credential::File {
                path: "not-read-by-the-http-client".into(),
            };
        }
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
            requests[0].starts_with(&format!("POST /v1/{route} HTTP/")),
            "The client must use the configured completion URL exactly."
        );
        assert!(
            requests[1].starts_with("GET /v1/models HTTP/"),
            "The connection test must use the API base, not the completion route."
        );
    }
}

#[test]
fn deepseek_beta_completion_and_model_urls_use_different_routes() {
    for configured in [
        "",
        "https://api.deepseek.com/beta",
        "https://api.deepseek.com/beta/completions/",
    ] {
        let mut provider = Provider {
            model: "deepseek-flash".into(),
            base_url: configured.into(),
            ..Provider::default()
        };
        let identity = provider.endpoint().unwrap();
        provider.credential = Credential::Vault {
            id: "fixture-reference".into(),
            endpoint: identity.clone(),
        };
        provider.validate().unwrap();
        assert_eq!(
            provider.completion_url().unwrap(),
            "https://api.deepseek.com/beta/completions"
        );
        assert_eq!(
            provider.models_url().unwrap(),
            "https://api.deepseek.com/models"
        );
        assert_eq!(
            provider.endpoint().unwrap(),
            identity,
            "URL routing must not change saved credential identities."
        );
    }
}

#[test]
fn completion_urls_must_match_the_configured_protocol() {
    let mut provider = Provider {
        kind: Kind::Compatible,
        model: "fixture".into(),
        base_url: "https://example.com/v1/completions".into(),
        protocol: Protocol::Chat,
        ..Provider::default()
    };
    assert!(provider.validate().is_err());
    provider.protocol = Protocol::Fim;
    provider.base_url = "https://example.com/v1/chat/completions".into();
    assert!(provider.validate().is_err());
}

#[test]
fn pasting_provider_fields_creates_a_project_override() {
    let server = Server::new("name", Duration::ZERO);
    let global = Preferences {
        enabled: Some(true),
        provider: Some(server.provider()),
    };
    let mut form = ai::form::Form::new(sqln::store::Scope::Project, None, Some(&global));
    assert!(form.inherit);
    form.focus = ai::form::Field::Model;
    form.model.set("");
    form.paste("deepseek-flash");
    assert!(
        !form.inherit,
        "Pasting must have the same override behavior as typing."
    );
    assert_eq!(
        form.preferences().unwrap().provider.unwrap().model,
        "deepseek-flash"
    );
}
