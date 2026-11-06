// The log, the panic report, and the command line.

use std::fs;
use std::process::{Command, Output};

use crate::common::temp_dir;
use sqln::terminal::write_panic_report;

fn sqln(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sqln"))
        .args(args)
        .output()
        .unwrap()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn the_version_names_the_program() {
    let output = sqln(&["--version"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(text.trim(), format!("sqln {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn an_option_without_its_value_stops_the_start() {
    for (option, message) in [
        ("--project", "--project needs a folder."),
        ("--log", "--log needs a file."),
        ("--dialect", "Unknown dialect: "),
    ] {
        let output = sqln(&[option]);
        assert_eq!(output.status.code(), Some(2), "{option}");
        assert!(stderr_of(&output).contains(message), "{option}");
    }
}

#[test]
fn an_unknown_dialect_stops_the_start() {
    let output = sqln(&["--dialect", "oracle"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr_of(&output).contains("Unknown dialect: oracle"));
}

#[test]
fn each_dialect_name_is_valid() {
    for dialect in ["postgres", "mysql", "sqlite", "redis"] {
        let output = sqln(&["--dialect", dialect, "--version"]);
        assert!(output.status.success(), "{dialect}");
    }
}

#[test]
fn a_log_that_does_not_open_stops_the_start() {
    let base = temp_dir("log-refused");
    let output = Command::new(env!("CARGO_BIN_EXE_sqln"))
        .arg("--log")
        .arg(&base)
        .current_dir(&base)
        .env("XDG_CONFIG_HOME", &base)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr_of(&output).starts_with("sqln: "));
}

#[test]
fn a_panic_report_holds_the_place_and_never_a_message() {
    let base = temp_dir("panic-report");
    let path = write_panic_report(&base, Some("src/app.rs:10:5")).unwrap();
    let report = fs::read_to_string(&path).unwrap();
    assert!(report.starts_with("sqln "), "{report}");
    assert!(report.contains("src/app.rs:10:5"), "{report}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn the_log_writes_lines_with_the_time_and_only_the_owner_can_read_it() {
    let base = temp_dir("log-file");
    let path = base.join("sqln.log");
    sqln::log::open(&path).unwrap();
    sqln::log::event("execution finished: Completed");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.lines().next().unwrap().ends_with(" start"), "{text}");
    assert!(
        text.lines()
            .nth(1)
            .unwrap()
            .ends_with(" execution finished: Completed"),
        "{text}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
