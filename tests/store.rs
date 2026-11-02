// The files of the application: folders, safe writes, locks, configuration, scripts.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::temp_dir;
use sqln::connection::Source;
use sqln::connector::target::{Address, Tls};
use sqln::editor::KeyboardMode;
use sqln::store::config::{self, Change, ConnectionEntry, CredentialName, KindName};
use sqln::store::file::{self, Expect, WriteError};
use sqln::store::scripts::{self, LineEnding, ScriptMeta};
use sqln::store::workspace::{self, EditorState, Position};
use sqln::store::{
    Located, Root, Scope, Store, create_project, find_project, global_dir, locate_project,
};

fn store_in(name: &str) -> (Store, PathBuf) {
    let base = temp_dir(name);
    let global = Root::global(base.join("global"));
    let project = Root::project(&base.join("project"));
    fs::create_dir_all(&project.dir).unwrap();
    (
        Store {
            global,
            project: Some(project),
        },
        base,
    )
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

// ---- folders ----

#[test]
fn global_folder_follows_xdg_then_home() {
    let xdg = Some("/custom/config".into());
    assert_eq!(
        global_dir(xdg, Some("/home/a".into())),
        Some("/custom/config/sqln".into())
    );
    assert_eq!(
        global_dir(None, Some("/home/a".into())),
        Some("/home/a/.config/sqln".into())
    );
    // A relative `XDG_CONFIG_HOME` is not valid, and the home folder applies.
    let relative = Some("config".into());
    assert_eq!(
        global_dir(relative, Some("/home/a".into())),
        Some("/home/a/.config/sqln".into())
    );
    assert_eq!(global_dir(None, None), None);
}

#[test]
fn project_search_uses_the_nearest_folder_and_ignores_git() {
    let base = temp_dir("find-project");
    fs::create_dir_all(base.join("outer/.sqln")).unwrap();
    fs::create_dir_all(base.join("outer/inner/.sqln")).unwrap();
    fs::create_dir_all(base.join("outer/inner/deep/.git")).unwrap();
    assert_eq!(
        find_project(&base.join("outer/inner/deep")),
        Some(base.join("outer/inner"))
    );
    assert_eq!(
        find_project(&base.join("outer/other")),
        Some(base.join("outer"))
    );
    // A file named `.sqln` is not a project.
    fs::create_dir_all(base.join("plain")).unwrap();
    fs::write(base.join("plain/.sqln"), "").unwrap();
    assert_eq!(
        find_project(&base.join("plain")).filter(|path| path.starts_with(&base)),
        None
    );
}

#[test]
fn creating_a_project_writes_the_gitignore_and_keeps_an_existing_one() {
    let base = temp_dir("create-project");
    let root = create_project(&base).unwrap();
    assert_eq!(root.dir, base.join(".sqln"));
    assert_eq!(root.base, base);
    let ignore = fs::read_to_string(base.join(".sqln/.gitignore")).unwrap();
    assert_eq!(ignore, "workspace.json\nlocks/\n*.tmp\n");
    assert!(!base.join(".git").exists());
    fs::write(base.join(".sqln/.gitignore"), "mine\n").unwrap();
    create_project(&base).unwrap();
    assert_eq!(
        fs::read_to_string(base.join(".sqln/.gitignore")).unwrap(),
        "mine\n"
    );
}

#[test]
fn the_start_uses_the_nearest_project_or_offers_the_working_folder() {
    let base = temp_dir("locate");
    fs::create_dir_all(base.join("a/.sqln")).unwrap();
    fs::create_dir_all(base.join("a/b/c")).unwrap();
    fs::create_dir_all(base.join("free")).unwrap();
    let found = locate_project(&base.join("a/b/c"), None);
    assert_eq!(
        found,
        Located {
            root: Some(base.join("a")),
            offer: None
        }
    );
    let none = locate_project(&base.join("free"), None);
    assert_eq!(none.root.filter(|root| root.starts_with(&base)), None);
    assert_eq!(none.offer, Some(base.join("free")));
}

#[test]
fn a_project_folder_of_the_command_line_replaces_the_search() {
    let base = temp_dir("locate-flag");
    fs::create_dir_all(base.join("outer/.sqln")).unwrap();
    fs::create_dir_all(base.join("outer/inner")).unwrap();
    fs::create_dir_all(base.join("other/.sqln")).unwrap();
    let working = base.join("outer/inner");
    // A folder with a project wins, even when an upper folder has another project.
    let flag = locate_project(&working, Some(Path::new("../../other")));
    assert_eq!(
        flag.root
            .as_deref()
            .map(|root| root.canonicalize().unwrap()),
        Some(base.join("other").canonicalize().unwrap())
    );
    assert_eq!(flag.offer, None);
    // A folder without a project gets the offer. The search upward does not run.
    let missing = locate_project(&working, Some(Path::new("fresh")));
    assert_eq!(
        missing,
        Located {
            root: None,
            offer: Some(working.join("fresh"))
        }
    );
    // An absolute folder stays as it is.
    let absolute = locate_project(&working, Some(&base.join("other")));
    assert_eq!(absolute.root, Some(base.join("other")));
}

// ---- safe writes ----

#[test]
fn a_write_checks_the_destination_before_it_replaces_the_file() {
    let dir = temp_dir("write-atomic");
    let path = dir.join("a.txt");
    let first = file::write_atomic(&path, b"one", Expect::Missing).unwrap();
    assert_eq!(first, file::hash(b"one"));
    assert_eq!(
        file::write_atomic(&path, b"two", Expect::Missing),
        Err(WriteError::Changed)
    );
    fs::write(&path, "other").unwrap();
    assert_eq!(
        file::write_atomic(&path, b"two", Expect::Hash(first)),
        Err(WriteError::Changed)
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "other");
    fs::remove_file(&path).unwrap();
    assert_eq!(
        file::write_atomic(&path, b"two", Expect::Hash(first)),
        Err(WriteError::Removed)
    );
    assert!(!path.exists());
    assert!(file::write_atomic(&path, b"two", Expect::Any).is_ok());
    assert_eq!(fs::read_to_string(&path).unwrap(), "two");
    // No temporary file stays after a refusal or a success.
    let names: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(names, ["a.txt"]);
}

#[test]
fn a_write_creates_the_missing_folders() {
    let dir = temp_dir("write-folders");
    let path = dir.join("a/b/c.txt");
    assert!(file::write_atomic(&path, b"x", Expect::Missing).is_ok());
}

#[test]
fn old_temporary_files_go_and_recent_ones_stay() {
    let dir = temp_dir("old-tmp");
    fs::write(dir.join("old.json.tmp"), "x").unwrap();
    fs::write(dir.join("new.json.tmp"), "x").unwrap();
    fs::write(dir.join("keep.json"), "x").unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3_600);
    fs::File::options()
        .write(true)
        .open(dir.join("old.json.tmp"))
        .unwrap()
        .set_modified(old)
        .unwrap();
    file::remove_old_temporary_files(&dir);
    assert!(!dir.join("old.json.tmp").exists());
    assert!(dir.join("new.json.tmp").exists());
    assert!(dir.join("keep.json").exists());
}

#[test]
fn a_lock_excludes_a_second_holder_and_frees_on_drop() {
    let dir = temp_dir("lock");
    let path = dir.join("locks/x.lock");
    let first = file::try_lock(&path).unwrap();
    assert!(first.is_some());
    assert!(file::try_lock(&path).unwrap().is_none());
    drop(first);
    assert!(file::try_lock(&path).unwrap().is_some());
}

#[test]
fn a_file_that_is_not_utf8_is_an_error() {
    let dir = temp_dir("not-utf8");
    fs::write(dir.join("a.sql"), [0xff, 0xfe]).unwrap();
    let error = file::read(&dir.join("a.sql")).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
}

// ---- configuration ----

const GLOBAL: &str = r#"{
  "version": 1,
  "preferences": { "keyboardMode": "conventional", "pageSize": 50, "rowCap": 200 },
  "connections": [
    { "id": "g1", "name": "app", "kind": "sqlite", "file": "data/app.db" },
    { "id": "g2", "name": "pg", "kind": "postgres", "host": "db.example.test", "port": 5433,
      "database": "app_db", "username": "me", "credential": "vault",
      "tls": { "mode": "verify", "caFile": "ca.pem", "clientCert": null, "clientKey": null },
      "pool": { "maxConnections": 3 }, "connectTimeoutSeconds": 10, "sqlTimeoutSeconds": null }
  ]
}"#;

const PROJECT: &str = r#"{
  "version": 1,
  "preferences": { "keyboardMode": "vim", "pageSize": 20 },
  "connections": [
    { "id": "p1", "name": "app", "kind": "sqlite", "file": "db/app.db", "readOnly": true }
  ]
}"#;

fn loaded(store: &Store) -> config::Config {
    config::load(store).config.unwrap()
}

#[test]
fn a_project_replaces_only_the_values_that_it_declares() {
    let (store, _) = store_in("precedence");
    write(&store.global.config_file(), GLOBAL);
    write(&store.project.as_ref().unwrap().config_file(), PROJECT);
    let config = loaded(&store);
    assert_eq!(config.keyboard, KeyboardMode::Vim);
    assert_eq!(config.page_size, Some(20));
    // The project does not declare `rowCap`, so the global value stays.
    assert_eq!(config.row_cap, Some(200));
    assert_eq!(config.budget_bytes, None);
}

#[test]
fn without_files_the_defaults_apply() {
    let (store, _) = store_in("no-files");
    let loaded = config::load(&store);
    assert_eq!(
        loaded.hashes,
        [(Scope::Global, None), (Scope::Project, None)]
    );
    let config = loaded.config.unwrap();
    assert_eq!(config.keyboard, KeyboardMode::Conventional);
    assert!(config.connections.is_empty());
    assert_eq!(config.page_size, None);
}

#[test]
fn both_scopes_list_their_connections_and_keep_their_names() {
    let (store, base) = store_in("connections");
    write(&store.global.config_file(), GLOBAL);
    write(&store.project.as_ref().unwrap().config_file(), PROJECT);
    let config = loaded(&store);
    let places: Vec<_> = config
        .connections
        .iter()
        .map(|item| (item.scope, item.id.as_str()))
        .collect();
    assert_eq!(
        places,
        [
            (Scope::Global, "g1"),
            (Scope::Global, "g2"),
            (Scope::Project, "p1")
        ]
    );
    // The same name in two scopes is valid.
    assert_eq!(
        config.connections[0].definition.name,
        config.connections[2].definition.name
    );
    let project = &config.connections[2].definition;
    assert!(project.read_only);
    // A relative path resolves from the root of the project, not from `.sqln`.
    let Source::Sqlite(target) = project.resolved_source() else {
        panic!("sqlite")
    };
    assert_eq!(target.path, base.join("project").join("db/app.db"));
    // A global connection uses the global folder.
    let Source::Sqlite(target) = config.connections[0].definition.resolved_source() else {
        panic!("sqlite")
    };
    assert_eq!(target.path, base.join("global").join("data/app.db"));
}

#[test]
fn a_network_entry_becomes_a_definition_without_a_password() {
    let (store, base) = store_in("network-entry");
    write(&store.global.config_file(), GLOBAL);
    let config = loaded(&store);
    let stored = &config.connections[1];
    assert_eq!(stored.credential, CredentialName::Vault);
    let definition = &stored.definition;
    assert_eq!(definition.max_connections, 3);
    assert_eq!(definition.connect_timeout.as_secs(), 10);
    assert_eq!(definition.sql_timeout, None);
    let Source::Network(target) = &definition.source else {
        panic!("network")
    };
    assert_eq!(
        target.address,
        Address::Tcp {
            host: "db.example.test".into(),
            port: 5433
        }
    );
    assert_eq!(target.database.as_deref(), Some("app_db"));
    assert!(target.password.is_none());
    let Tls::Verify(files) = &target.tls else {
        panic!("tls")
    };
    // The certificate file resolves from the folder of the scope.
    let Source::Network(resolved) = definition.resolved_source() else {
        panic!("network")
    };
    let Tls::Verify(resolved) = resolved.tls else {
        panic!("tls")
    };
    assert_eq!(files.ca.as_deref(), Some(Path::new("ca.pem")));
    assert_eq!(resolved.ca, Some(base.join("global").join("ca.pem")));
}

#[test]
fn a_missing_sql_timeout_is_the_default_and_null_is_no_limit() {
    let (store, _) = store_in("sql-timeout");
    write(
        &store.global.config_file(),
        r#"{"version":1,"connections":[
            {"id":"a","name":"a","kind":"sqlite","file":"a.db"},
            {"id":"b","name":"b","kind":"sqlite","file":"b.db","sqlTimeoutSeconds":null},
            {"id":"c","name":"c","kind":"sqlite","file":"c.db","sqlTimeoutSeconds":7}]}"#,
    );
    let config = loaded(&store);
    let timeouts: Vec<_> = config
        .connections
        .iter()
        .map(|item| item.definition.sql_timeout.map(|t| t.as_secs()))
        .collect();
    assert_eq!(timeouts, [Some(30), None, Some(7)]);
}

fn error_of(store: &Store) -> config::ConfigError {
    config::load(store).config.unwrap_err()
}

#[test]
fn a_syntax_error_names_the_file_and_the_position() {
    let (store, _) = store_in("syntax");
    write(
        &store.global.config_file(),
        "{\n  \"version\": 1,\n  \"connections\": [,]\n}",
    );
    let error = error_of(&store);
    assert_eq!(error.file, store.global.config_file());
    assert!(error.message.contains("line 3"), "{}", error.message);
    assert!(error.to_string().contains("config.json"));
}

#[test]
fn an_error_of_the_project_file_names_that_file() {
    let (store, _) = store_in("project-error");
    write(&store.global.config_file(), GLOBAL);
    let project = store.project.as_ref().unwrap().config_file();
    write(&project, r#"{"version": 1, "unknown": 1}"#);
    let error = error_of(&store);
    assert_eq!(error.file, project);
    assert!(error.message.contains("unknown field"), "{}", error.message);
}

#[test]
fn an_error_message_never_holds_a_value_of_the_file() {
    let (store, _) = store_in("secret");
    write(
        &store.global.config_file(),
        r#"{"version":1,"connections":[{"id":"a","name":"a","kind":"postgres","host":"h",
            "username":"u","port":"hunter2"}]}"#,
    );
    let error = error_of(&store);
    assert!(!error.to_string().contains("hunter2"), "{error}");
    assert!(error.message.contains("line"), "{}", error.message);
}

#[test]
fn the_rules_of_the_content_make_a_file_invalid() {
    let cases = [
        (r#"{"version": 2}"#, "version"),
        (r#"{"version":1,"preferences":{"pageSize":0}}"#, "pageSize"),
        (
            r#"{"version":1,"connections":[{"id":"a","name":"x","kind":"sqlite","file":"a"},
            {"id":"a","name":"y","kind":"sqlite","file":"b"}]}"#,
            "id",
        ),
        (
            r#"{"version":1,"connections":[{"id":"a","name":"X","kind":"sqlite","file":"a"},
            {"id":"b","name":"x","kind":"sqlite","file":"b"}]}"#,
            "name",
        ),
        (
            r#"{"version":1,"connections":[{"id":"a","name":"x","kind":"sqlite"}]}"#,
            "needs a file",
        ),
        (
            r#"{"version":1,"connections":[{"id":"a","name":"x","kind":"mysql","host":"h"}]}"#,
            "username",
        ),
        (
            r#"{"version":1,"connections":[{"id":"a","name":"x","kind":"sqlite","file":"a",
            "sqlTimeoutSeconds":0}]}"#,
            "timeout",
        ),
        (
            r#"{"version":1,"connections":[{"id":"a","name":"x","kind":"sqlite","file":"a",
            "credential":"globalPlainText"}]}"#,
            "globalPlainText",
        ),
    ];
    for (index, (text, word)) in cases.into_iter().enumerate() {
        let (store, _) = store_in(&format!("rules-{index}"));
        // The rule about `globalPlainText` applies to the project file.
        let target = if word == "globalPlainText" {
            store.project.as_ref().unwrap().config_file()
        } else {
            store.global.config_file()
        };
        write(&target, text);
        let error = error_of(&store);
        assert!(error.message.contains(word), "{word}: {}", error.message);
    }
}

#[test]
fn an_invalid_config_still_reports_the_hash_of_the_file() {
    let (store, _) = store_in("invalid-hash");
    write(&store.global.config_file(), "{");
    let loaded = config::load(&store);
    assert!(loaded.config.is_err());
    assert_eq!(loaded.hashes[0], (Scope::Global, Some(file::hash(b"{"))));
}

#[test]
fn the_keymap_of_the_active_mode_applies_and_a_bad_one_names_its_file() {
    let (store, _) = store_in("keymap");
    write(
        &store.global.config_file(),
        r#"{"version":1,"preferences":{"keyboardMode":"vim","keymap":{
            "conventional":{"run":["f9"]},"vim":{"run":["f6"]}}}}"#,
    );
    let config = loaded(&store);
    assert_eq!(config.keymap.keys(sqln::input::Action::Run), ["f6"]);

    write(
        &store.project.as_ref().unwrap().config_file(),
        r#"{"version":1,"preferences":{"keymap":{"vim":{"stop":["f6"]}}}}"#,
    );
    // The project map adds to the global one, and `f6` has two actions now.
    let error = error_of(&store);
    assert_eq!(error.file, store.project.as_ref().unwrap().config_file());
    assert!(error.message.contains("f6"), "{}", error.message);

    write(
        &store.project.as_ref().unwrap().config_file(),
        r#"{"version":1,"preferences":{"keymap":{"vim":{"nothing":["f6"]}}}}"#,
    );
    assert!(error_of(&store).message.contains("nothing"));
}

fn entry(id: &str, name: &str) -> ConnectionEntry {
    let value = serde_json::json!({
        "id": id, "name": name, "kind": "sqlite", "file": format!("{name}.db")
    });
    serde_json::from_value(value).unwrap()
}

#[test]
fn a_change_reads_the_file_again_and_keeps_the_other_values() {
    let (store, _) = store_in("change");
    let root = &store.global;
    write(&root.config_file(), GLOBAL);
    config::change_connections(
        root,
        Scope::Global,
        Change::Upsert(Box::new(entry("n1", "extra"))),
    )
    .unwrap();
    // Another instance adds a connection between two writes.
    let text = fs::read_to_string(root.config_file()).unwrap();
    let text = text.replace("\"pageSize\": 50", "\"pageSize\": 77");
    fs::write(root.config_file(), text).unwrap();
    config::change_connections(root, Scope::Global, Change::Remove("g1".into())).unwrap();
    let config = loaded(&store);
    let ids: Vec<_> = config
        .connections
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    assert_eq!(ids, ["g2", "n1"]);
    assert_eq!(config.page_size, Some(77));
    assert_eq!(config.row_cap, Some(200));
}

#[test]
fn a_change_replaces_an_entry_by_id_and_refuses_a_duplicate_name() {
    let (store, _) = store_in("change-replace");
    let root = &store.global;
    config::change_connections(
        root,
        Scope::Global,
        Change::Upsert(Box::new(entry("a", "one"))),
    )
    .unwrap();
    let mut renamed = entry("a", "two");
    renamed.read_only = true;
    config::change_connections(root, Scope::Global, Change::Upsert(Box::new(renamed))).unwrap();
    let config = loaded(&store);
    assert_eq!(config.connections.len(), 1);
    assert_eq!(config.connections[0].definition.name, "two");
    let error = config::change_connections(
        root,
        Scope::Global,
        Change::Upsert(Box::new(entry("b", "TWO"))),
    )
    .unwrap_err();
    assert!(error.contains("name"), "{error}");
    assert_eq!(loaded(&store).connections.len(), 1);
}

#[test]
fn a_change_does_not_overwrite_an_invalid_file() {
    let (store, _) = store_in("change-invalid");
    write(&store.global.config_file(), "{");
    let error = config::change_connections(
        &store.global,
        Scope::Global,
        Change::Upsert(Box::new(entry("a", "one"))),
    )
    .unwrap_err();
    assert!(error.contains("config.json"), "{error}");
    assert_eq!(fs::read_to_string(store.global.config_file()).unwrap(), "{");
}

#[test]
fn a_change_creates_the_file_of_a_scope_without_one() {
    let (store, _) = store_in("change-create");
    let root = store.project.as_ref().unwrap();
    assert!(!root.config_file().exists());
    config::change_connections(
        root,
        Scope::Project,
        Change::Upsert(Box::new(entry("a", "one"))),
    )
    .unwrap();
    let text = fs::read_to_string(root.config_file()).unwrap();
    assert!(text.contains("\"version\": 1"));
    // The file holds only the values that the project declares.
    assert!(!text.contains("pageSize"));
}

#[test]
fn a_definition_round_trips_through_an_entry_without_the_password() {
    use sqln::connection::Definition;
    use sqln::connector::sqlite::SqliteTarget;
    use sqln::connector::target::{Engine, NetworkTarget, Secret};

    let mut target = SqliteTarget::new("data/app.db");
    target.foreign_keys = Some(true);
    let mut sqlite = Definition::new("local", Source::Sqlite(target));
    let plain = serde_json::to_string(&config::to_entry("id-1", &sqlite)).unwrap();
    assert!(
        !plain.contains("theme") && !plain.contains("icon"),
        "{plain}"
    );
    sqlite.theme = Some(config::ThemeName::TokyoNight);
    sqlite.icon = Some(config::ConnectionIcon::Snowflake);
    let entry = config::to_entry("id-1", &sqlite);
    assert_eq!(entry.kind, KindName::Sqlite);
    let themed = serde_json::to_string(&entry).unwrap();
    assert!(themed.contains("\"theme\":\"tokyo-night\""), "{themed}");
    assert!(themed.contains("\"icon\":\"snowflake\""), "{themed}");
    // The file keeps the text of the user, so a project stays portable.
    assert_eq!(entry.file.as_deref(), Some("data/app.db"));
    assert_eq!(entry.credential, CredentialName::None);
    let back = config::to_definition(&entry, PathBuf::from("/base"));
    assert_eq!(back.theme, Some(config::ThemeName::TokyoNight));
    assert_eq!(back.icon, Some(config::ConnectionIcon::Snowflake));
    let Source::Sqlite(back_target) = &back.source else {
        panic!("sqlite")
    };
    assert_eq!(back_target.path, Path::new("data/app.db"));
    assert_eq!(back_target.foreign_keys, Some(true));

    let network = NetworkTarget {
        engine: Engine::MySql,
        address: Address::Tcp {
            host: "h".into(),
            port: 3307,
        },
        user: "u".into(),
        password: Some(Secret::new("hunter2")),
        database: Some("d".into()),
        tls: Tls::Disabled,
    };
    let mut definition = Definition::new("remote", Source::Network(network));
    definition.sql_timeout = None;
    let entry = config::to_entry("id-2", &definition);
    assert_eq!(entry.credential, CredentialName::Session);
    let text = serde_json::to_string(&entry).unwrap();
    assert!(!text.contains("hunter2"), "{text}");
    assert!(text.contains("\"sqlTimeoutSeconds\":null"), "{text}");
    let back = config::to_definition(&entry, PathBuf::from("/base"));
    assert_eq!(back.sql_timeout, None);
    let Source::Network(target) = &back.source else {
        panic!("network")
    };
    assert_eq!(target.engine, Engine::MySql);
    assert_eq!(
        target.address,
        Address::Tcp {
            host: "h".into(),
            port: 3307
        }
    );
    assert!(target.password.is_none());
    assert_eq!(target.tls, Tls::Disabled);
}

// ---- scripts ----

fn meta(id: &str, name: &str, connection: &str, scope: Scope) -> ScriptMeta {
    ScriptMeta::new(id.into(), name.into(), connection.into(), scope)
}

#[test]
fn a_script_has_two_files_that_list_and_read_back() {
    let (store, _) = store_in("scripts");
    let root = &store.global;
    let hash = scripts::create(
        root,
        &meta("s1", "console-1", "c1", Scope::Global),
        "SELECT 1;",
    )
    .unwrap();
    assert_eq!(hash, file::hash(b"SELECT 1;"));
    let list = scripts::list(root, Scope::Global);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].meta.name, "console-1");
    assert_eq!(list[0].meta.connection_id, "c1");
    assert_eq!(list[0].scope, Scope::Global);
    assert!(list[0].modified.is_some());
    assert_eq!(
        scripts::read_text(root, "s1").unwrap(),
        ("SELECT 1;".to_owned(), hash)
    );
    // The metadata file has the documented fields.
    let json = fs::read_to_string(root.consoles_dir().join("s1.json")).unwrap();
    assert!(json.contains("\"connectionId\": \"c1\""), "{json}");
    assert!(json.contains("\"connectionScope\": \"global\""), "{json}");
}

#[test]
fn creating_a_script_never_replaces_an_existing_one() {
    let (store, _) = store_in("scripts-create");
    let root = &store.global;
    scripts::create(root, &meta("s1", "a", "c1", Scope::Global), "one").unwrap();
    assert!(scripts::create(root, &meta("s1", "b", "c1", Scope::Global), "two").is_err());
    assert_eq!(scripts::read_text(root, "s1").unwrap().0, "one");
}

#[test]
fn the_list_skips_damaged_files_and_names_that_do_not_match() {
    let (store, _) = store_in("scripts-skip");
    let root = &store.global;
    scripts::create(root, &meta("ok", "ok", "c1", Scope::Global), "x").unwrap();
    write(&root.consoles_dir().join("bad.json"), "{");
    // A file whose name differs from its id could point outside the folder.
    write(
        &root.consoles_dir().join("other.json"),
        r#"{"version":1,"id":"../x","name":"n","connectionId":"c","connectionScope":"global"}"#,
    );
    let names: Vec<_> = scripts::list(root, Scope::Global)
        .into_iter()
        .map(|item| item.meta.id)
        .collect();
    assert_eq!(names, ["ok"]);
}

#[test]
fn a_large_folder_lists_each_script_in_the_same_order() {
    const SCRIPTS: usize = 600;
    let (store, _) = store_in("scripts-large");
    let root = &store.global;
    for index in 0..SCRIPTS {
        let id = format!("s{index}");
        let script = meta(&id, &format!("name {index:03}"), "c1", Scope::Global);
        scripts::create(root, &script, "x").unwrap();
    }
    write(&root.consoles_dir().join("bad.json"), "{");
    let list = scripts::list(root, Scope::Global);
    assert_eq!(list.len(), SCRIPTS);
    let order = |a: &scripts::ScriptEntry, b: &scripts::ScriptEntry| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.meta.name.cmp(&b.meta.name))
    };
    assert!(list.is_sorted_by(|a, b| order(a, b).is_le()));
}

#[test]
fn removing_a_script_removes_both_files_and_a_second_removal_is_quiet() {
    let (store, _) = store_in("scripts-remove");
    let root = &store.global;
    scripts::create(root, &meta("s1", "a", "c1", Scope::Global), "x").unwrap();
    scripts::remove(root, "s1").unwrap();
    assert!(scripts::list(root, Scope::Global).is_empty());
    assert!(!root.consoles_dir().join("s1.sql").exists());
    scripts::remove(root, "s1").unwrap();
}

#[test]
fn moving_a_script_to_another_scope_copies_checks_and_removes_the_original() {
    let (store, _) = store_in("scripts-move");
    let (from, to) = (&store.global, store.project.as_ref().unwrap());
    scripts::create(from, &meta("s1", "a", "old", Scope::Global), "SELECT 2;").unwrap();
    let moved = meta("s1", "a", "new", Scope::Project);
    let hash = scripts::move_to(from, to, &moved).unwrap();
    assert_eq!(hash, file::hash(b"SELECT 2;"));
    assert!(scripts::list(from, Scope::Global).is_empty());
    let list = scripts::list(to, Scope::Project);
    assert_eq!(list[0].meta.connection_id, "new");
    assert_eq!(list[0].meta.connection_scope, Scope::Project);
    assert_eq!(scripts::read_text(to, "s1").unwrap().0, "SELECT 2;");
}

#[test]
fn a_move_to_a_place_that_has_the_script_keeps_the_original() {
    let (store, _) = store_in("scripts-move-fail");
    let (from, to) = (&store.global, store.project.as_ref().unwrap());
    scripts::create(from, &meta("s1", "a", "old", Scope::Global), "mine").unwrap();
    scripts::create(to, &meta("s1", "a", "x", Scope::Project), "theirs").unwrap();
    assert!(scripts::move_to(from, to, &meta("s1", "a", "new", Scope::Project)).is_err());
    assert_eq!(scripts::read_text(from, "s1").unwrap().0, "mine");
    assert_eq!(scripts::read_text(to, "s1").unwrap().0, "theirs");
}

#[test]
fn the_line_ending_of_a_file_comes_back_on_write() {
    assert_eq!(
        scripts::decode("a\r\nb\r\n"),
        ("a\nb\n".to_owned(), LineEnding::Crlf)
    );
    assert_eq!(
        scripts::decode("a\nb\n"),
        ("a\nb\n".to_owned(), LineEnding::Lf)
    );
    assert_eq!(scripts::encode("a\nb\n", LineEnding::Crlf), "a\r\nb\r\n");
    assert_eq!(scripts::encode("a\nb\n", LineEnding::Lf), "a\nb\n");
}

#[test]
fn a_script_lock_is_per_script_and_per_folder() {
    let (store, _) = store_in("scripts-lock");
    let first = scripts::lock(&store.global, "s1").unwrap();
    assert!(first.is_some());
    assert!(scripts::lock(&store.global, "s1").unwrap().is_none());
    assert!(scripts::lock(&store.global, "s2").unwrap().is_some());
    assert!(
        scripts::lock(store.project.as_ref().unwrap(), "s1")
            .unwrap()
            .is_some()
    );
}

// ---- workspace ----

#[test]
fn the_workspace_keeps_the_cursor_and_the_layout_between_updates() {
    let (store, _) = store_in("workspace");
    let root = &store.global;
    assert_eq!(workspace::load(root), workspace::Workspace::default());
    let state = EditorState {
        cursor: Position { row: 3, col: 5 },
        scroll: Position { row: 1, col: 0 },
    };
    workspace::update(root, |workspace| {
        workspace.consoles.insert("s1".into(), state);
    })
    .unwrap();
    workspace::update(root, |workspace| {
        workspace.layout = Some(workspace::Layout {
            navigator_collapsed: true,
            navigator_width: 30,
            editor_ratio: 0.4,
        });
    })
    .unwrap();
    let workspace = workspace::load(root);
    assert_eq!(workspace.consoles["s1"], state);
    assert_eq!(workspace.layout.unwrap().navigator_width, 30);
}

#[test]
fn a_damaged_workspace_gives_an_empty_state() {
    let (store, _) = store_in("workspace-damaged");
    write(&store.global.workspace_file(), "not json");
    assert_eq!(
        workspace::load(&store.global),
        workspace::Workspace::default()
    );
    workspace::update(&store.global, |workspace| workspace.layout = None).unwrap();
}
