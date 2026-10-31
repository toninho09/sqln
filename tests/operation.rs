// The log, the panic report, and the command line.

mod common;

use std::fs;

use common::temp_dir;
use sqln::terminal::write_panic_report;

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
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
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
    assert!(text.lines().nth(1).unwrap().ends_with(" execution finished: Completed"), "{text}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
