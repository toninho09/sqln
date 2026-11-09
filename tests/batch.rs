//! The real batch command with closed stdin and no controlling terminal.
#![cfg(unix)]

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};

use serde_json::{Value, json};
use sqln::connection::ConnectionKey;
use sqln::connector::target::parse_uri;
use sqln::creds::Identity;
use sqln::store::{Root, Scope, Store, config};
use sqln::trust::Trust;

struct Home(PathBuf);

impl Home {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sqln-batch-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sqln"));
        command
            .current_dir(&self.0)
            .env("HOME", &self.0)
            .env("XDG_CONFIG_HOME", &self.0)
            .env_remove("SQLN_PASSWORD")
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: setsid is the only operation after fork.
        unsafe {
            command.pre_exec(|| {
                rustix::process::setsid()?;
                Ok(())
            });
        }
        command
    }

    fn run(&self, arguments: &[&str]) -> Output {
        self.command(arguments).output().unwrap()
    }

    fn query(&self, sql: &str, format: &str) -> Output {
        self.run(&[":memory:", "--execute", sql, "--output", format])
    }

    fn config(&self, project: bool, connections: Value) {
        let folder = self.0.join(if project { ".sqln" } else { "sqln" });
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("config.json"),
            json!({"version":1,"connections":connections}).to_string(),
        )
        .unwrap();
    }

    fn database(&self) -> PathBuf {
        let path = self.0.join("one.db");
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch("CREATE TABLE items(id INTEGER); INSERT INTO items VALUES (1), (2);")
            .unwrap();
        path
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn wait(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("The batch process does not stop.");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn success(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn csv_preserves_null_empty_unicode_quotes_and_binary() {
    let home = Home::new();
    let output = home.query(
        r#"SELECT NULL AS n, '' AS empty, 'ação,"x' AS text, X'00FF' AS bytes"#,
        "csv",
    );
    assert_eq!(
        success(&output),
        "n,empty,text,bytes\n,\"\",\"ação,\"\"x\",0x00FF\n"
    );
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.contains(&0x1b));
    assert!(!home.0.join("sqln").exists());
    assert!(!home.0.join(".sqln").exists());
}

#[test]
fn json_preserves_precision_and_disambiguates_names() {
    let home = Home::new();
    let output = home.query(
        "SELECT 9223372036854775807 AS id, NULL AS id, X'00FF' AS bytes, '' AS empty",
        "json",
    );
    let text = success(&output);
    assert!(text.contains("9223372036854775807"));
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value[0]["id"], 9223372036854775807_i64);
    assert_eq!(value[0]["id_2"], Value::Null);
    assert_eq!(value[0]["bytes"], "AP8=");
    assert_eq!(value[0]["empty"], "");
}

#[test]
fn empty_result_keeps_its_format() {
    let home = Home::new();
    assert_eq!(
        success(&home.query("SELECT 1 AS id WHERE 0", "csv")),
        "id\n"
    );
    assert_eq!(
        success(&home.query("SELECT 1 AS id WHERE 0", "json")),
        "[]\n"
    );
}

#[test]
fn streams_more_than_the_grid_cap_without_a_reread() {
    let home = Home::new();
    let output = home.query("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<5000) SELECT x FROM n", "csv");
    // Drain the pipe while the process writes more than its capacity.
    let text = success(&output);
    assert_eq!(text.lines().count(), 5001);
    assert!(text.ends_with("5000\n"));
}

#[test]
fn arguments_and_multiple_statements_fail_before_creation() {
    let home = Home::new();
    for args in [
        vec!["missing.db", "--execute", "SELECT 1", "--output", "xml"],
        vec!["--output", "csv"],
        vec!["missing.db", "--single-transaction"],
        vec!["missing.db", "--continue-on-error"],
        vec![":memory:", "--execute", "-"],
        vec![
            "missing.db",
            "--execute",
            "-- comment",
            "--output",
            "csv",
            "--yes",
        ],
        vec![
            "missing.db",
            "--execute",
            "SELECT 1",
            "--output",
            "csv",
            "--script",
            "file.sql",
        ],
        vec![
            "missing.db",
            "--execute",
            "SELECT 1",
            "--output",
            "csv",
            "--connection",
            "one",
        ],
        vec![
            ":memory:",
            "--execute",
            "SELECT 1",
            "--execute",
            "SELECT 2",
            "--output",
            "csv",
        ],
    ] {
        let output = home.run(&args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert!(!home.0.join("missing.db").exists());
    }
    assert_eq!(
        success(&home.query("/* comment */ SELECT ';' AS text; -- end", "csv")),
        "text\n;\n"
    );
}

#[test]
fn several_instructions_write_every_result_set() {
    let home = Home::new();
    let sql = "SELECT 1 AS a; SELECT 2 AS b";
    assert_eq!(
        success(&home.run(&[":memory:", "--execute", sql, "--output", "csv"])),
        "a\n1\nb\n2\n"
    );
    assert_eq!(
        success(&home.run(&[":memory:", "--execute", sql, "--output", "json"])),
        "[\n{\"a\":1}\n]\n[\n{\"b\":2}\n]\n"
    );
    let output = home.run(&[":memory:", "--execute", sql]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(errors.contains("sqln: [1] 1 rows returned."));
    assert!(errors.contains("sqln: [2] 1 rows returned."));
}

#[test]
fn writes_without_output_report_rows_affected_per_instruction() {
    let home = Home::new();
    let path = home.database();
    let output = home.run(&[
        path.to_str().unwrap(),
        "--execute",
        "INSERT INTO items VALUES (3); INSERT INTO items VALUES (4)",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(errors.contains("sqln: [1] 1 rows affected."));
    assert!(errors.contains("sqln: [2] 1 rows affected."));
}

#[test]
fn a_failure_stops_the_script_by_default() {
    let home = Home::new();
    let path = home.database();
    let target = path.to_str().unwrap();
    let output = home.run(&[
        target,
        "--execute",
        "INSERT INTO items VALUES (3); SELECT * FROM absent; INSERT INTO items VALUES (4)",
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("[2]"));
    assert_eq!(rows(&path), 3);
}

#[test]
fn continue_on_error_runs_the_rest_and_reports_every_failure() {
    let home = Home::new();
    let path = home.database();
    let target = path.to_str().unwrap();
    let output = home.run(&[
        target,
        "--execute",
        "INSERT INTO items VALUES (3); SELECT * FROM absent; INSERT INTO items VALUES (4)",
        "--continue-on-error",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(errors.contains("sqln: [2]"));
    assert!(errors.contains("1 of 3 instructions failed."));
    assert_eq!(rows(&path), 4);
}

#[test]
fn single_transaction_commits_and_rolls_back() {
    let home = Home::new();
    let path = home.database();
    let target = path.to_str().unwrap();
    let ok = home.run(&[
        target,
        "--execute",
        "INSERT INTO items VALUES (3); INSERT INTO items VALUES (4)",
        "--single-transaction",
    ]);
    assert_eq!(ok.status.code(), Some(0));
    assert_eq!(rows(&path), 4);
    let failed = home.run(&[
        target,
        "--execute",
        "INSERT INTO items VALUES (9); SELECT * FROM absent",
        "--single-transaction",
    ]);
    assert_eq!(failed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("[2]"));
    assert_eq!(rows(&path), 4);
}

#[test]
fn single_transaction_flushes_results_before_commit() {
    for format in ["csv", "json"] {
        let home = Home::new();
        let path = home.database();
        let output = home.run(&[
            path.to_str().unwrap(),
            "--execute",
            "INSERT INTO items VALUES (3) RETURNING id",
            "--single-transaction",
            "--output",
            format,
        ]);
        let text = success(&output);
        if format == "json" {
            assert_eq!(
                serde_json::from_str::<Value>(&text).unwrap(),
                json!([{"id":3}])
            );
        } else {
            assert_eq!(text, "id\n3\n");
        }
        assert!(output.stderr.is_empty());
        assert_eq!(rows(&path), 3);
    }
}

#[test]
fn single_transaction_rolls_back_when_final_output_fails() {
    for format in ["csv", "json"] {
        for continue_on_error in [false, true] {
            let home = Home::new();
            let path = home.database();
            let (reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();
            drop(reader);
            let mut command = home.command(&[
                path.to_str().unwrap(),
                "--execute",
                "INSERT INTO items VALUES (3) RETURNING id",
                "--single-transaction",
                "--output",
                format,
            ]);
            if continue_on_error {
                command.arg("--continue-on-error");
            }
            // The small result stays in BufWriter until the final flush.
            let mut child = command
                .stdout(Stdio::from(std::os::fd::OwnedFd::from(writer)))
                .spawn()
                .unwrap();
            wait(&mut child);
            let output = child.wait_with_output().unwrap();
            assert_eq!(output.status.code(), Some(1));
            assert_eq!(rows(&path), 2);
            let errors = String::from_utf8_lossy(&output.stderr);
            assert!(errors.contains("Output failed"), "{errors}");
            assert!(!errors.contains("commit failed"), "{errors}");
            assert!(!errors.contains("rollback failed"), "{errors}");
        }
    }
}

#[test]
fn single_transaction_rolls_back_when_final_output_times_out() {
    for format in ["csv", "json"] {
        let home = Home::new();
        let path = home.database();
        home.config(
            false,
            json!([{"id":"g1","name":"one","kind":"sqlite","file":path,"sqlTimeoutSeconds":1}]),
        );
        let (_reader, output) = full_output();
        let mut child = home
            .command(&[
                "--connection",
                "one",
                "--execute",
                "INSERT INTO items VALUES (3) RETURNING id",
                "--single-transaction",
                "--output",
                format,
            ])
            .stdout(output)
            .spawn()
            .unwrap();
        wait(&mut child);
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(rows(&path), 2);
        let errors = String::from_utf8_lossy(&output.stderr);
        assert!(errors.contains("Output failed"), "{errors}");
        assert!(errors.contains("timeout"), "{errors}");
    }
}

#[test]
fn single_transaction_rolls_back_when_final_output_gets_a_signal() {
    for format in ["csv", "json"] {
        for signal in [rustix::process::Signal::INT, rustix::process::Signal::TERM] {
            let home = Home::new();
            let path = home.database();
            home.config(
                false,
                json!([{"id":"g1","name":"one","kind":"sqlite","file":path,"sqlTimeoutSeconds":null}]),
            );
            let log = home.0.join("events.log");
            let (_reader, output) = full_output();
            let mut child = home
                .command(&[
                    "--connection",
                    "one",
                    "--execute",
                    "INSERT INTO items VALUES (3) RETURNING id",
                    "--single-transaction",
                    "--output",
                    format,
                    "--log",
                    log.to_str().unwrap(),
                ])
                .stdout(output)
                .spawn()
                .unwrap();
            wait_for_batch(&log);
            std::thread::sleep(Duration::from_millis(100));
            assert!(child.try_wait().unwrap().is_none());
            let pid = rustix::process::Pid::from_raw(child.id() as i32).unwrap();
            rustix::process::kill_process(pid, signal).unwrap();
            wait(&mut child);
            let output = child.wait_with_output().unwrap();
            assert_eq!(output.status.code(), Some(1));
            assert_eq!(rows(&path), 2);
            let errors = String::from_utf8_lossy(&output.stderr);
            assert!(errors.contains("signal"), "{errors}");
        }
    }
}

#[test]
fn single_transaction_refuses_redis() {
    let home = Home::new();
    let output = home.run(&[
        "redis://127.0.0.1:1/0",
        "--execute",
        "PING",
        "--single-transaction",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Redis"));
}

#[test]
fn standard_input_runs_a_script() {
    let home = Home::new();
    let mut child = home
        .command(&[":memory:", "--execute", "-", "--output", "csv"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"SELECT 1 AS a; SELECT 2 AS b")
        .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "a\n1\nb\n2\n",
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn standard_input_above_the_limit_is_refused() {
    let home = Home::new();
    let mut child = home
        .command(&[":memory:", "--execute", "-", "--output", "csv"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    // The process stops reading after the limit, so the test writes exactly one
    // byte more and never blocks on a full pipe.
    let over = 10 * 1024 * 1024 + 1;
    let chunk = [b'x'; 65536];
    let mut sent = 0;
    while sent < over {
        let end = (over - sent).min(chunk.len());
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(&chunk[..end])
            .unwrap();
        sent += end;
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("10 MiB"));
}

#[test]
fn standard_input_on_a_terminal_is_refused() {
    let home = Home::new();
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
    grantpt(&master).unwrap();
    unlockpt(&master).unwrap();
    let path = ptsname(&master, Vec::new()).unwrap();
    let slave = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path.to_str().unwrap())
        .unwrap();
    let output = home
        .command(&[":memory:", "--execute", "-", "--output", "csv"])
        .stdin(Stdio::from(slave))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("standard input"));
}

fn rows(path: &std::path::Path) -> i64 {
    rusqlite::Connection::open(path)
        .unwrap()
        .query_row("SELECT count(*) FROM items", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn a_missing_database_needs_authorization() {
    let home = Home::new();
    let args = [
        "new.db",
        "--execute",
        "CREATE TABLE t(id)",
        "--output",
        "csv",
    ];
    let refused = home.run(&args);
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--yes"));
    assert!(!home.0.join("new.db").exists());
    let mut authorized = args.to_vec();
    authorized.push("--yes");
    assert!(success(&home.run(&authorized)).is_empty());
    assert!(home.0.join("new.db").exists());
}

#[test]
fn saved_names_and_scopes_preserve_paths_and_preferences() {
    let home = Home::new();
    let path = home.database();
    home.config(
        false,
        json!([{"id":"g1","name":"one","kind":"sqlite","file":path}]),
    );
    home.config(
        true,
        json!([{"id":"p1","name":"ONE","kind":"sqlite","file":"one.db"}]),
    );
    let args = [
        "--connection",
        "one",
        "--execute",
        "SELECT count(*) AS n FROM items",
        "--output",
        "csv",
    ];
    assert_eq!(home.run(&args).status.code(), Some(2));
    let before = fs::read(home.0.join(".sqln/config.json")).unwrap();
    for scope in ["global", "project"] {
        let mut scoped = args.to_vec();
        scoped.extend(["--scope", scope]);
        assert_eq!(success(&home.run(&scoped)), "n\n2\n");
    }
    assert_eq!(before, fs::read(home.0.join(".sqln/config.json")).unwrap());
    assert!(!home.0.join(".sqln/workspace.json").exists());
    assert!(!home.0.join("sqln/history.jsonl").exists());
    home.config(
        false,
        json!([{"id":"g2","name":"Produção","kind":"sqlite","file":path}]),
    );
    assert_eq!(
        success(&home.run(&[
            "--connection",
            "PRODUÇÃO",
            "--execute",
            "SELECT 1 AS n",
            "--output",
            "csv"
        ])),
        "n\n1\n"
    );
}

#[test]
fn protected_writes_need_yes_and_execute_only_once() {
    let home = Home::new();
    let path = home.database();
    let target = path.to_str().unwrap();
    let args = [
        target,
        "--execute",
        "UPDATE items SET id=id+1",
        "--output",
        "json",
    ];
    let refused = home.run(&args);
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--yes"));
    let mut authorized = args.to_vec();
    authorized.push("--yes");
    let output = home.run(&authorized);
    assert!(success(&output).is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("2 rows affected"));
    let output = home.run(&[
        target,
        "--execute",
        "SELECT sum(id) AS n FROM items",
        "--output",
        "csv",
    ]);
    assert_eq!(success(&output), "n\n5\n");
}

#[test]
fn production_and_read_only_rules_apply_to_saved_connections() {
    let home = Home::new();
    let path = home.database();
    home.config(
        false,
        json!([{"id":"g1","name":"one","kind":"sqlite","file":path,"production":true}]),
    );
    let args = [
        "--connection",
        "one",
        "--execute",
        "INSERT INTO items VALUES (3)",
        "--output",
        "csv",
    ];
    assert_eq!(home.run(&args).status.code(), Some(1));
    let mut authorized = args.to_vec();
    authorized.push("--yes");
    assert!(success(&home.run(&authorized)).is_empty());
    home.config(
        false,
        json!([{"id":"g1","name":"one","kind":"sqlite","file":path,"readOnly":true}]),
    );
    assert_eq!(home.run(&authorized).status.code(), Some(1));
}

#[test]
fn sql_errors_and_open_transactions_do_not_report_success() {
    let home = Home::new();
    for sql in ["SELECT * FROM absent", "BEGIN"] {
        let output = home.query(sql, "json");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn database_errors_cannot_send_terminal_controls() {
    let home = Home::new();
    let output = home.query("SELECT * FROM \"bad\u{1b}[31m\"", "csv");
    assert_eq!(output.status.code(), Some(1));
    assert!(!output.stderr.contains(&0x1b));
}

#[test]
fn saved_timeout_still_applies_to_streaming() {
    let home = Home::new();
    home.config(
        false,
        json!([{"id":"g1","name":"one","kind":"sqlite","file":":memory:","sqlTimeoutSeconds":1}]),
    );
    let output = home.run(&["--connection", "one", "--execute", "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000000000) SELECT sum(x) FROM n", "--output", "json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Timeout"));
}

#[test]
fn saved_timeout_stops_a_full_output_pipe() {
    let home = Home::new();
    home.config(
        false,
        json!([{"id":"g1","name":"one","kind":"sqlite","file":":memory:","sqlTimeoutSeconds":1}]),
    );
    let mut child = home.command(&["--connection", "one", "--execute", "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000000000) SELECT x FROM n", "--output", "csv"]).spawn().unwrap();
    // Keep the pipe open without reading until the process stops.
    wait(&mut child);
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .to_lowercase()
            .contains("timeout")
    );
}

#[test]
fn saved_timeout_stops_output_after_a_finite_query_finishes() {
    for format in ["csv", "json"] {
        let home = Home::new();
        home.config(
            false,
            json!([{"id":"g1","name":"one","kind":"sqlite","file":":memory:","sqlTimeoutSeconds":1}]),
        );
        let mut child = home
            .command(&[
                "--connection",
                "one",
                "--execute",
                "SELECT hex(zeroblob(1024 * 1024)) AS payload",
                "--output",
                format,
            ])
            .spawn()
            .unwrap();
        // One row fills stdout, but the executor can queue ResultDone and finish the query.
        wait(&mut child);
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .to_lowercase()
                .contains("timeout")
        );
    }
}

/// Fills a separate output stream before the batch process starts.
fn full_output() -> (std::os::unix::net::UnixStream, Stdio) {
    let (reader, mut writer) = std::os::unix::net::UnixStream::pair().unwrap();
    writer.set_nonblocking(true).unwrap();
    for bytes in [&[b'x'; 4096][..], &b"x"[..]] {
        loop {
            match writer.write(bytes) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("Cannot fill the output stream: {error}"),
            }
        }
    }
    writer.set_nonblocking(false).unwrap();
    (reader, Stdio::from(std::os::fd::OwnedFd::from(writer)))
}

fn wait_for_batch(log: &std::path::Path) {
    let ready = Instant::now() + Duration::from_secs(20);
    while !fs::read_to_string(log)
        .unwrap_or_default()
        .contains("batch started")
    {
        assert!(Instant::now() < ready, "The batch executor does not start.");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn saved_timeout_stops_a_full_error_stream() {
    let home = Home::new();
    let path = home.database();
    home.config(
        false,
        json!([{"id":"g1","name":"one","kind":"sqlite","file":path,"sqlTimeoutSeconds":1}]),
    );
    let (_reader, errors) = full_output();
    let mut child = home
        .command(&[
            "--connection",
            "one",
            "--execute",
            "INSERT INTO items VALUES (3)",
            "--output",
            "csv",
        ])
        .stderr(errors)
        .spawn()
        .unwrap();
    wait(&mut child);
    assert_eq!(child.wait().unwrap().code(), Some(1));
}

#[test]
fn signals_stop_a_full_error_stream_without_repeating_the_write() {
    for signal in [rustix::process::Signal::INT, rustix::process::Signal::TERM] {
        let home = Home::new();
        let path = home.database();
        home.config(
            false,
            json!([{"id":"g1","name":"one","kind":"sqlite","file":path,"sqlTimeoutSeconds":null}]),
        );
        let log = home.0.join("events.log");
        let (_reader, errors) = full_output();
        let mut child = home
            .command(&[
                "--connection",
                "one",
                "--execute",
                "INSERT INTO items VALUES (3)",
                "--output",
                "csv",
                "--log",
                log.to_str().unwrap(),
            ])
            .stderr(errors)
            .spawn()
            .unwrap();
        wait_for_batch(&log);
        std::thread::sleep(Duration::from_millis(100));
        assert!(child.try_wait().unwrap().is_none());
        let pid = rustix::process::Pid::from_raw(child.id() as i32).unwrap();
        rustix::process::kill_process(pid, signal).unwrap();
        wait(&mut child);
        assert_eq!(child.wait().unwrap().code(), Some(1));
        let inserted: i64 = rusqlite::Connection::open(&path)
            .unwrap()
            .query_row("SELECT count(*) FROM items WHERE id=3", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(inserted, 1);
    }
}

#[test]
fn a_disabled_timeout_allows_error_output_to_wait_for_its_reader() {
    let home = Home::new();
    let path = home.database();
    home.config(
        false,
        json!([{"id":"g1","name":"one","kind":"sqlite","file":path,"sqlTimeoutSeconds":null}]),
    );
    let (mut reader, errors) = full_output();
    let mut child = home
        .command(&[
            "--connection",
            "one",
            "--execute",
            "INSERT INTO items VALUES (3)",
            "--output",
            "csv",
        ])
        .stderr(errors)
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(1200));
    assert!(child.try_wait().unwrap().is_none());
    let reader = std::thread::spawn(move || {
        let mut errors = String::new();
        reader.read_to_string(&mut errors).unwrap();
        errors
    });
    wait(&mut child);
    assert_eq!(child.wait().unwrap().code(), Some(0));
    assert!(
        reader
            .join()
            .unwrap()
            .ends_with("sqln: [1] 1 rows affected.\n")
    );
}

#[test]
fn final_errors_do_not_wait_for_a_full_error_stream() {
    let home = Home::new();
    let (_reader, errors) = full_output();
    let mut child = home
        .command(&[
            ":memory:",
            "--execute",
            "SELECT * FROM no_such_table",
            "--output",
            "csv",
        ])
        .stderr(errors)
        .spawn()
        .unwrap();
    wait(&mut child);
    assert_eq!(child.wait().unwrap().code(), Some(1));
}

#[test]
fn a_disabled_timeout_allows_finite_output_to_wait_for_its_reader() {
    let home = Home::new();
    home.config(
        false,
        json!([{"id":"g1","name":"one","kind":"sqlite","file":":memory:","sqlTimeoutSeconds":null}]),
    );
    let mut child = home
        .command(&[
            "--connection",
            "one",
            "--execute",
            "SELECT hex(zeroblob(1024 * 1024)) AS payload",
            "--output",
            "csv",
        ])
        .spawn()
        .unwrap();
    child.stdout.as_mut().unwrap().read_exact(&mut [0]).unwrap();
    std::thread::sleep(Duration::from_millis(1200));
    assert!(child.try_wait().unwrap().is_none());
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.len() > 1024 * 1024);
}

#[test]
fn broken_output_cancels_execution() {
    let home = Home::new();
    let mut child = home.command(&[":memory:", "--execute", "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000000000) SELECT x FROM n", "--output", "csv"]).spawn().unwrap();
    drop(child.stdout.take());
    wait(&mut child);
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Output failed"));
}

#[test]
fn signals_cancel_queries_and_blocked_output() {
    for blocked in [false, true] {
        let home = Home::new();
        let sql = if blocked {
            "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000000000) SELECT x FROM n"
        } else {
            "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000000000) SELECT sum(x) FROM n"
        };
        let log = home.0.join("events.log");
        let mut child = home
            .command(&[
                ":memory:",
                "--execute",
                sql,
                "--output",
                "csv",
                "--log",
                log.to_str().unwrap(),
            ])
            .spawn()
            .unwrap();
        wait_for_batch(&log);
        if blocked {
            std::thread::sleep(Duration::from_millis(100));
        }
        let pid = rustix::process::Pid::from_raw(child.id() as i32).unwrap();
        rustix::process::kill_process(
            pid,
            if blocked {
                rustix::process::Signal::TERM
            } else {
                rustix::process::Signal::INT
            },
        )
        .unwrap();
        wait(&mut child);
        let output = child.wait_with_output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn duckdb_uses_the_same_output_contract() {
    let home = Home::new();
    let output = home.run(&[
        "duckdb:",
        "--execute",
        "SELECT 123456789012345678901234567890::HUGEINT AS n",
        "--output",
        "json",
    ]);
    assert!(success(&output).contains("123456789012345678901234567890"));
}

#[test]
fn password_sources_preserve_spaces_and_follow_priority() {
    for source in ["file", "environment", "uri"] {
        let home = Home::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let observed = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut length = [0; 4];
            socket.read_exact(&mut length).unwrap();
            let mut startup = vec![0; u32::from_be_bytes(length) as usize - 4];
            socket.read_exact(&mut startup).unwrap();
            // PostgreSQL requests clear-text authentication on this loopback test socket.
            socket.write_all(b"R\0\0\0\x08\0\0\0\x03").unwrap();
            let mut kind = [0];
            socket.read_exact(&mut kind).unwrap();
            assert_eq!(kind, [b'p']);
            socket.read_exact(&mut length).unwrap();
            let mut password = vec![0; u32::from_be_bytes(length) as usize - 4];
            socket.read_exact(&mut password).unwrap();
            password.pop();
            password
        });
        let uri = format!("postgres://user:uri-password@{address}/db");
        let mut command = home.command(&[&uri, "--execute", "SELECT 1", "--output", "json"]);
        if source != "uri" {
            command.env("SQLN_PASSWORD", "environment-password");
        }
        let file = home.0.join("password");
        if source == "file" {
            fs::write(&file, " file-password \r\n").unwrap();
            command.args(["--password-file", file.to_str().unwrap()]);
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let expected = match source {
            "file" => " file-password ",
            "environment" => "environment-password",
            _ => "uri-password",
        };
        assert_eq!(observed.join().unwrap(), expected.as_bytes());
        assert!(!String::from_utf8_lossy(&output.stderr).contains(expected));
    }
    let home = Home::new();
    let output = home
        .command(&[
            "postgres://user@127.0.0.1:1/db",
            "--execute",
            "SELECT 1",
            "--output",
            "csv",
            "--password-file",
            "absent",
        ])
        .env("SQLN_PASSWORD", "do-not-use")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("password file"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("do-not-use"));
}

fn server(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(uri) => Some(uri),
        Err(_) => {
            println!("skipped: {name} is not set");
            None
        }
    }
}

#[test]
fn yes_does_not_authorize_a_saved_destination_or_a_new_project_path() {
    let home = Home::new();
    let definition = sqln::connection::Definition::new(
        "network",
        sqln::connection::Source::Network(parse_uri("postgres://user@127.0.0.1:1/db").unwrap()),
    );
    let entry = config::to_entry("p1", &definition);
    home.config(true, serde_json::to_value(vec![entry]).unwrap());
    let args = [
        "--connection",
        "network",
        "--execute",
        "SELECT 1",
        "--output",
        "json",
        "--yes",
    ];
    let output = home
        .command(&args)
        .env("SQLN_PASSWORD", "not-sent")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("trust"));
    assert!(!home.0.join("sqln/trust.json").exists());
    let identity = Identity::of(
        &ConnectionKey {
            scope: Scope::Project,
            id: "p1".into(),
        },
        &definition,
    )
    .unwrap();
    let store = Store {
        global: Root::global(home.0.join("sqln")),
        project: Some(Root::project(&home.0)),
    };
    let old = home.0.join("old-project");
    fs::create_dir(&old).unwrap();
    Trust::new(&store).confirm(&identity, Some(&old)).unwrap();
    let before = fs::read(store.global.dir.join("trust.json")).unwrap();
    let output = home.run(&args);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("trust"));
    assert_eq!(
        before,
        fs::read(store.global.dir.join("trust.json")).unwrap()
    );
}

fn network_credentials(variable: &str) {
    let Some(uri) = server(variable) else { return };
    let home = Home::new();
    let target = parse_uri(&uri).unwrap();
    let password = target
        .password
        .clone()
        .expect("The integration server has a password.");
    let definition =
        sqln::connection::Definition::new("network", sqln::connection::Source::Network(target));
    let mut entry = config::to_entry("g1", &definition);
    entry.credential = config::CredentialName::None;
    home.config(false, serde_json::to_value(vec![entry]).unwrap());
    let identity = Identity::of(
        &ConnectionKey {
            scope: Scope::Global,
            id: "g1".into(),
        },
        &definition,
    )
    .unwrap();
    let args = [
        "--connection",
        "network",
        "--execute",
        "SELECT 1 AS n",
        "--output",
        "csv",
        "--yes",
    ];
    let refused = home.run(&args);
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("trust"));
    assert!(!home.0.join("sqln/trust.json").exists());
    Trust::new(&Store {
        global: Root::global(home.0.join("sqln")),
        project: None,
    })
    .confirm(&identity, None)
    .unwrap();
    let password_file = home.0.join("password");
    fs::write(&password_file, format!("{}\r\n", password.expose())).unwrap();
    let mut environment = home.command(&args);
    environment.env("SQLN_PASSWORD", password.expose());
    assert_eq!(success(&environment.output().unwrap()), "n\n1\n");
    let mut file = home.command(&args);
    file.env("SQLN_PASSWORD", "wrong-password")
        .args(["--password-file", password_file.to_str().unwrap()]);
    assert_eq!(success(&file.output().unwrap()), "n\n1\n");
    let mut invalid = home.command(&args);
    invalid
        .env("SQLN_PASSWORD", password.expose())
        .args(["--password-file", "absent-secret"]);
    let output = invalid.output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(password.expose()));
    assert!(!home.0.join("sqln/credentials.json").exists());
}

#[test]
fn postgres_notices_stop_on_a_full_error_stream() {
    let Some(uri) = server("SQLN_TEST_POSTGRES") else {
        return;
    };
    let home = Home::new();
    let target = parse_uri(&uri).unwrap();
    let password = target.password.clone().unwrap();
    let mut definition =
        sqln::connection::Definition::new("network", sqln::connection::Source::Network(target));
    definition.sql_timeout = Some(Duration::from_secs(1));
    home.config(
        false,
        serde_json::to_value(vec![config::to_entry("g1", &definition)]).unwrap(),
    );
    let identity = Identity::of(
        &ConnectionKey {
            scope: Scope::Global,
            id: "g1".into(),
        },
        &definition,
    )
    .unwrap();
    Trust::new(&Store {
        global: Root::global(home.0.join("sqln")),
        project: None,
    })
    .confirm(&identity, None)
    .unwrap();
    let (_reader, errors) = full_output();
    let mut child = home
        .command(&[
            "--connection",
            "network",
            "--execute",
            "DO $$ BEGIN RAISE NOTICE 'batch notice'; END $$",
            "--output",
            "csv",
        ])
        .env("SQLN_PASSWORD", password.expose())
        .stderr(errors)
        .spawn()
        .unwrap();
    wait(&mut child);
    assert_eq!(child.wait().unwrap().code(), Some(1));
}

#[test]
fn postgres_batch_credentials() {
    network_credentials("SQLN_TEST_POSTGRES");
}

#[test]
fn mysql_multiple_results_write_every_set() {
    let Some(uri) = server("SQLN_TEST_MYSQL") else {
        return;
    };
    let home = Home::new();
    let name = format!("sqln_batch_{}", uuid::Uuid::new_v4().simple());
    let create = format!(
        "CREATE PROCEDURE {name}() BEGIN SELECT 1 AS first_column; SELECT 2 AS second_column; END"
    );
    success(&home.run(&[&uri, "--execute", &create, "--output", "csv"]));
    let output = home.run(&[
        &uri,
        "--execute",
        &format!("CALL {name}()"),
        "--output",
        "csv",
    ]);
    let cleanup = home.run(&[
        &uri,
        "--execute",
        &format!("DROP PROCEDURE {name}"),
        "--output",
        "csv",
    ]);
    success(&cleanup);
    let text = success(&output);
    assert!(text.contains("first_column"));
    assert!(text.contains("second_column"));
}

#[test]
fn mysql_batch_credentials() {
    network_credentials("SQLN_TEST_MYSQL");
}
