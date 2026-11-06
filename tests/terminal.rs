// The real program in a terminal: the start, the end, the password, a signal, and a
// panic. A pseudoterminal takes the place of the terminal.
#![cfg(unix)]

mod common;

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::BorrowedFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use common::temp_dir;
use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use rustix::io::{FdFlags, fcntl_setfd};
use rustix::process::{Pid, Signal, ioctl_tiocsctty, kill_process, setsid};
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
use rustix::termios::{LocalModes, Winsize, tcgetattr, tcsetwinsize};
use sqln::store::Root;
use sqln::terminal::Session;
use sqln::ui::text;

const LIMIT: Duration = Duration::from_secs(30);

const SIZE: Winsize = Winsize {
    ws_row: 30,
    ws_col: 100,
    ws_xpixel: 0,
    ws_ypixel: 0,
};

const ENTER_SCREEN: &str = "\x1b[?1049h";
const LEAVE_SCREEN: &str = "\x1b[?1049l";
const MOUSE_OFF: &str = "\x1b[?1000l";
const PASTE_OFF: &str = "\x1b[?2004l";
const SHOW_CURSOR: &str = "\x1b[?25h";
const KEY_REPORTS_ON: &str = "\x1b[>1u";
const KEY_REPORTS_OFF: &str = "\x1b[<1u";

/// The question of the program about the reports of keys.
const KEY_REPORTS_QUERY: &str = "\x1b[?u";
/// The answer of a terminal without reports of keys.
const NO_KEY_REPORTS: &[u8] = b"\x1b[?62c";
/// The answer of a terminal with reports of keys.
const KEY_REPORTS: &[u8] = b"\x1b[?0u\x1b[?62c";

const QUIT: &[u8] = b"\x17q";
/// A word of the first screen. The words of a sentence do not stay together in the
/// output, because the terminal skips the cells that do not change.
const FIRST_SCREEN: &str = "EXPLORER";

/// The test that runs in a second process finds its folder here.
const PANIC_FOLDER: &str = "SQLN_TEST_PANIC_FOLDER";

/// One start at a time. A new file description has no close-on-exec flag at first on
/// macOS, and a child of a parallel test must not receive it.
static START: Mutex<()> = Mutex::new(());

struct Terminal {
    child: Child,
    /// The side of the test. A drop of this side closes the terminal.
    master: Option<File>,
    /// The side of the program. A test reads the mode of the terminal here.
    slave: File,
    output: Vec<u8>,
}

impl Terminal {
    fn start(mut command: Command) -> Self {
        let _start = START.lock().unwrap_or_else(PoisonError::into_inner);
        let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
        fcntl_setfd(&master, FdFlags::CLOEXEC).unwrap();
        grantpt(&master).unwrap();
        unlockpt(&master).unwrap();
        let flags = fcntl_getfl(&master).unwrap();
        fcntl_setfl(&master, flags | OFlags::NONBLOCK).unwrap();
        let path = ptsname(&master, Vec::new()).unwrap();
        let slave = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(OFlags::NOCTTY.bits() as i32)
            .open(path.to_str().unwrap())
            .unwrap();
        tcsetwinsize(&slave, SIZE).unwrap();
        // The pseudoterminal becomes the controlling terminal of the child, because the
        // program also writes to `/dev/tty`. Without this step, that text reaches the
        // terminal of the person who runs the tests.
        //
        // SAFETY: the closure makes only system calls, so it is safe after a fork.
        unsafe {
            command.pre_exec(|| {
                setsid()?;
                ioctl_tiocsctty(BorrowedFd::borrow_raw(0))?;
                Ok(())
            });
        }
        let child = command
            .stdin(slave.try_clone().unwrap())
            .stdout(slave.try_clone().unwrap())
            .stderr(slave.try_clone().unwrap())
            .spawn()
            .unwrap();
        Self {
            child,
            master: Some(File::from(master)),
            slave,
            output: Vec::new(),
        }
    }

    /// Reads what the program wrote. Each wait reads, because a full terminal stops the
    /// program.
    fn read(&mut self) {
        let Some(master) = &mut self.master else {
            return;
        };
        let mut chunk = [0; 4096];
        while let Ok(count @ 1..) = master.read(&mut chunk) {
            self.output.extend_from_slice(&chunk[..count]);
        }
    }

    fn output(&self) -> String {
        String::from_utf8_lossy(&self.output).into_owned()
    }

    /// Reads until the condition is true.
    fn wait(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + LIMIT;
        loop {
            self.read();
            if done(self) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{what} did not happen: {:?}",
                self.output()
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn wait_for(&mut self, text: &str) {
        self.wait(text, |terminal| terminal.output().contains(text));
    }

    fn wait_for_raw_mode(&mut self) {
        self.wait("the raw mode", |terminal| !terminal.echoes());
    }

    fn wait_for_end(&mut self) -> ExitStatus {
        let deadline = Instant::now() + LIMIT;
        loop {
            self.read();
            if let Some(status) = self.child.try_wait().unwrap() {
                self.read();
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "the program did not stop: {:?}",
                self.output()
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        let master = self.master.as_mut().unwrap();
        master.write_all(bytes).unwrap();
    }

    /// Answers the question of the program about the keys, as a terminal does.
    fn answer(&mut self, reply: &[u8]) {
        self.wait_for(KEY_REPORTS_QUERY);
        self.send(reply);
    }

    /// Returns true when the terminal shows each key. The raw mode stops this.
    fn echoes(&self) -> bool {
        tcgetattr(&self.slave)
            .unwrap()
            .local_modes
            .contains(LocalModes::ECHO)
    }

    fn close(&mut self) {
        self.master = None;
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Prepares a command in the home folder of one test. The `.sqln` folder stops the
/// question about a new project.
fn in_home(home: &Path, file: &str) -> Command {
    fs::create_dir_all(home.join(".sqln")).unwrap();
    let mut command = Command::new(file);
    command
        .current_dir(home)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .env("TERM", "xterm-256color");
    command
}

fn program(home: &Path, args: &[&str]) -> Command {
    let mut command = in_home(home, env!("CARGO_BIN_EXE_sqln"));
    command.args(args);
    command
}

/// Runs the program below a shell. After the end of the program, the shell prints the
/// mode of the terminal and the exit code, and then it waits. It keeps the terminal
/// open, because macOS closes a terminal at the end of its first process.
fn program_in_shell(home: &Path, args: &[&str]) -> Command {
    let mut command = in_home(home, "/bin/sh");
    let script = r#""$@"; status=$?; stty -a; echo "status=$status"; exec sleep 60"#;
    command
        .args(["-c", script, "sh", env!("CARGO_BIN_EXE_sqln")])
        .args(args);
    command
}

/// Waits for the report of `program_in_shell`. The raw mode shows as `-echo`.
fn assert_ended_with_echo(terminal: &mut Terminal, status: i32) {
    terminal.wait_for(&format!("status={status}"));
    let output = terminal.output();
    assert!(
        output.split_whitespace().any(|word| word == "echo"),
        "the raw mode did not end: {output:?}"
    );
}

/// Returns a home folder with a saved SQLite connection, so a console has a script.
fn home_with_connection(name: &str) -> PathBuf {
    let home = temp_dir(name);
    let root = Root::global(home.join("sqln"));
    fs::create_dir_all(&root.dir).unwrap();
    let database = home.join("one.db");
    rusqlite::Connection::open(&database).unwrap();
    let text = format!(
        r#"{{"version":1,"preferences":{{}},"connections":[{{"id":"g1","name":"one","kind":"sqlite","file":"{}"}}]}}"#,
        database.display()
    );
    fs::write(root.config_file(), text).unwrap();
    home
}

fn scripts_in(home: &Path) -> Vec<String> {
    let folder = Root::global(home.join("sqln")).consoles_dir();
    fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "sql"))
        .map(|path| fs::read_to_string(path).unwrap())
        .collect()
}

/// Starts the program and goes to the first screen.
fn started(home: &Path) -> Terminal {
    let mut terminal = Terminal::start(program(home, &[]));
    terminal.answer(NO_KEY_REPORTS);
    terminal.wait_for(FIRST_SCREEN);
    terminal
}

fn paste(terminal: &mut Terminal, text: &str) {
    terminal.send(format!("\x1b[200~{text}\x1b[201~").as_bytes());
}

#[test]
#[ignore = "Requires explicit SQLN_TEST_DEEPSEEK_KEY_FILE and a paid DeepSeek API request."]
fn deepseek_fim_completion_runs_through_the_real_terminal() {
    let key_file = std::env::var_os("SQLN_TEST_DEEPSEEK_KEY_FILE")
        .map(PathBuf::from)
        .expect("Set SQLN_TEST_DEEPSEEK_KEY_FILE to a private key file outside the repository.");
    assert!(key_file.is_absolute() && key_file.is_file());
    let home = home_with_connection("terminal-deepseek-live");
    let db = home.join("one.db");
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT)")
        .unwrap();
    let root = Root::global(home.join("sqln"));
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(root.config_file()).unwrap()).unwrap();
    config["preferences"] = serde_json::json!({"mcp":false,"queryHistory":false,"aiCompletion":{"enabled":true,"provider":{
        "kind":"deepseek","model":"deepseek-flash","baseUrl":"https://api.deepseek.com/beta/completions",
        "maxOutputTokens":128,"credential":{"source":"file","path":key_file}
    }}});
    fs::write(root.config_file(), serde_json::to_vec(&config).unwrap()).unwrap();
    let log = home.join("live.log");
    let mut terminal = Terminal::start(program(&home, &["--log", log.to_str().unwrap()]));
    terminal.answer(NO_KEY_REPORTS);
    terminal.wait_for(FIRST_SCREEN);
    paste(&mut terminal, "-- List the user names\nSELECT ");
    terminal.wait_for("SELECT");
    terminal.send(b"\x00");
    terminal.wait_for("Complete with AI");
    terminal.send(b"\x1b[A\t");
    terminal.wait_for("Send SQL and schema");
    terminal.send(b"\t\r");
    terminal.wait_for("AI insertion preview");
    terminal.send(b"\t\r");
    terminal.send(QUIT);
    assert!(terminal.wait_for_end().success());
    let scripts = scripts_in(&home);
    assert_eq!(scripts.len(), 1);
    assert_ne!(scripts[0], "-- List the user names\nSELECT ");
    assert!(scripts[0].to_lowercase().contains("users"));
    let connection = rusqlite::Connection::open(db).unwrap();
    assert!(connection.prepare(&scripts[0]).unwrap().readonly());
    assert!(
        !fs::read_to_string(log)
            .unwrap()
            .contains("execution finished"),
        "Accepting generated SQL must not execute it."
    );
}

#[test]
fn a_start_and_a_quit_leave_the_terminal_as_it_was() {
    let home = temp_dir("terminal-quit");
    let log = home.join("sqln.log");
    let arguments = ["--log", log.to_str().unwrap()];
    let mut terminal = Terminal::start(program_in_shell(&home, &arguments));
    terminal.answer(NO_KEY_REPORTS);
    terminal.wait_for(ENTER_SCREEN);
    terminal.wait_for(FIRST_SCREEN);
    assert!(!terminal.echoes(), "the raw mode hides the keys");

    terminal.send(QUIT);
    assert_ended_with_echo(&mut terminal, 0);
    let output = terminal.output();
    for sequence in [LEAVE_SCREEN, MOUSE_OFF, PASTE_OFF, SHOW_CURSOR] {
        assert!(output.contains(sequence), "{sequence:?}: {output:?}");
    }
    assert!(!output.contains(KEY_REPORTS_OFF), "{output:?}");
    assert!(fs::read_to_string(&log).unwrap().contains(" exit"));
}

#[test]
fn a_terminal_with_reports_of_keys_gets_their_request_and_their_end() {
    let home = temp_dir("terminal-key-reports");
    let mut terminal = Terminal::start(program_in_shell(&home, &[]));
    terminal.answer(KEY_REPORTS);
    terminal.wait_for(KEY_REPORTS_ON);
    terminal.wait_for(FIRST_SCREEN);
    terminal.send(QUIT);
    assert_ended_with_echo(&mut terminal, 0);
    assert!(terminal.output().contains(KEY_REPORTS_OFF));
}

#[test]
fn a_signal_saves_the_text_and_restores_the_terminal() {
    let home = home_with_connection("terminal-signal");
    let mut terminal = started(&home);
    paste(&mut terminal, "SELECT 1;");
    terminal.wait_for("SELECT");

    kill_process(Pid::from_child(&terminal.child), Signal::TERM).unwrap();
    terminal.wait_for(LEAVE_SCREEN);
    assert!(terminal.wait_for_end().success());
    assert_eq!(scripts_in(&home), ["SELECT 1;"]);
}

#[test]
fn the_program_stops_when_its_terminal_closes() {
    let home = temp_dir("terminal-closed");
    let mut terminal = started(&home);
    terminal.close();
    terminal.wait_for_end();
}

#[test]
fn a_password_stays_off_the_terminal() {
    let home = temp_dir("terminal-password");
    let mut terminal = Terminal::start(program(&home, &["postgres://ana@127.0.0.1:1/shop"]));
    terminal.wait_for(&text::password_prompt("ana"));
    terminal.wait_for_raw_mode();
    terminal.send(b"hidden9\x7f\r");
    terminal.answer(NO_KEY_REPORTS);
    terminal.wait_for(FIRST_SCREEN);
    terminal.send(QUIT);
    assert!(terminal.wait_for_end().success());
    assert!(!terminal.output().contains("hidden"));
}

#[test]
fn escape_and_ctrl_c_stop_the_start_at_the_password() {
    for (name, key) in [("escape", b"\x1b"), ("ctrl-c", b"\x03")] {
        let home = temp_dir(&format!("terminal-password-{name}"));
        let target = ["postgres://ana@127.0.0.1:1/shop"];
        let mut terminal = Terminal::start(program_in_shell(&home, &target));
        terminal.wait_for(&text::password_prompt("ana"));
        terminal.wait_for_raw_mode();
        terminal.send(key);
        assert_ended_with_echo(&mut terminal, 1);
        assert!(terminal.output().contains("The start stopped."), "{name}");
    }
}

/// `a_panic_restores_the_terminal_first_and_writes_a_report` runs this test in a terminal.
/// The test has no effect without the folder of that test.
#[test]
fn a_session_that_panics() {
    let Some(folder) = std::env::var_os(PANIC_FOLDER) else {
        return;
    };
    let _session = Session::start(Some(folder.into())).unwrap();
    panic!("SELECT secret");
}

#[test]
fn a_panic_restores_the_terminal_first_and_writes_a_report() {
    let folder = temp_dir("terminal-panic");
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["a_session_that_panics", "--exact", "--nocapture"])
        .env(PANIC_FOLDER, &folder);
    let mut terminal = Terminal::start(command);
    terminal.answer(NO_KEY_REPORTS);
    terminal.wait_for("SELECT secret");
    assert!(!terminal.wait_for_end().success());

    let output = terminal.output();
    let left = output.find(LEAVE_SCREEN).unwrap();
    let message = output.find("SELECT secret").unwrap();
    assert!(left < message, "the message stays on the normal screen");

    let report = fs::read_dir(&folder).unwrap().next().unwrap().unwrap();
    let report = fs::read_to_string(report.path()).unwrap();
    assert!(report.contains("tests/terminal.rs"), "{report}");
    assert!(!report.contains("SELECT secret"), "{report}");
}
