//! Terminal setup and restoration.

use std::backtrace::Backtrace;
use std::io::{self, IsTerminal, Stdin, Stdout, Write, stderr, stdout};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossterm::cursor::Show;
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    supports_keyboard_enhancement,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use zeroize::Zeroizing;

use crate::connector::target::Secret;
use crate::store::file::{self, Expect};

pub type Screen = Terminal<CrosstermBackend<Stdout>>;

static KEYBOARD_ENHANCED: AtomicBool = AtomicBool::new(false);

/// The time between two checks of the terminal by the watchdog.
const TERMINAL_CHECK: Duration = Duration::from_secs(2);

/// Starts a thread that watches the terminal. A closed terminal stops the answer
/// of the event reader, so the event loop cannot end by itself. The thread sees
/// the closed terminal and stops the process.
pub fn watch_terminal() {
    #[cfg(unix)]
    {
        let _ = std::thread::Builder::new()
            .name("terminal-watchdog".to_owned())
            .spawn(watch_tty);
    }
}

/// The file description that the event reader watches: stdin for a terminal,
/// or `/dev/tty` without one.
#[cfg(unix)]
enum TerminalProbe {
    Stdin(Stdin),
    Tty(std::fs::File),
}

/// Reports the state of the terminal. `poll` never reads, so it takes no input
/// from the event reader.
#[cfg(unix)]
fn watch_tty() {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use std::os::fd::AsFd;

    let stdin = std::io::stdin();
    let probe = if stdin.is_terminal() {
        TerminalProbe::Stdin(stdin)
    } else {
        // `poll` cannot watch `/dev/tty` on macOS, so the watchdog stops there.
        // The event reader fails by itself on that path.
        let Ok(tty) = std::fs::File::open("/dev/tty") else {
            return;
        };
        TerminalProbe::Tty(tty)
    };
    let zero = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    loop {
        std::thread::sleep(TERMINAL_CHECK);
        let fd = match &probe {
            TerminalProbe::Stdin(stdin) => stdin.as_fd(),
            TerminalProbe::Tty(tty) => tty.as_fd(),
        };
        let mut fds = [PollFd::new(&fd, PollFlags::HUP | PollFlags::ERR)];
        if poll(&mut fds, Some(&zero)).is_err() {
            continue;
        }
        let events = fds[0].revents();
        if events.intersects(PollFlags::HUP | PollFlags::ERR) {
            crate::log::event("terminal closed");
            // The write can fail, because the terminal is gone. A failed write must not
            // stop this thread before `exit`, so it stays without an error.
            let _ = writeln!(stderr(), "sqln: the terminal closed.");
            std::process::exit(1);
        }
        if events.contains(PollFlags::NVAL) {
            return;
        }
    }
}

/// Holds the terminal in application mode. Its drop restores the terminal.
pub struct Session {
    pub screen: Screen,
    _guard: Guard,
}

struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        restore();
    }
}

impl Session {
    /// `reports` is the folder of the panic reports.
    pub fn start(reports: Option<PathBuf>) -> io::Result<Self> {
        install_panic_hook(reports);
        enable_raw_mode()?;
        let guard = Guard;
        let mut output = stdout();
        execute!(
            output,
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste
        )?;
        // The request includes only the separation of keys, not a report of all keys.
        if supports_keyboard_enhancement().unwrap_or(false) {
            execute!(
                output,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )?;
            KEYBOARD_ENHANCED.store(true, Ordering::Relaxed);
        }
        let screen = Terminal::new(CrosstermBackend::new(output))?;
        Ok(Self {
            screen,
            _guard: guard,
        })
    }
}

/// Reads a password before the interface starts, without an echo. An empty text means
/// no password. `Esc` and `Ctrl+C` return an error, which stops the start.
pub fn read_password(prompt: &str) -> io::Result<Option<Secret>> {
    let mut output = stderr();
    write!(output, "{prompt}")?;
    output.flush()?;
    enable_raw_mode()?;
    let password = read_hidden();
    disable_raw_mode()?;
    writeln!(output)?;
    let password = password?;
    Ok((!password.is_empty()).then(|| Secret::new(password.as_str())))
}

fn read_hidden() -> io::Result<Zeroizing<String>> {
    // The capacity keeps a common password in one buffer, which the drop clears.
    let mut password = Zeroizing::new(String::with_capacity(256));
    let stopped = || io::Error::new(io::ErrorKind::Interrupted, "The start stopped.");
    loop {
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Enter => return Ok(password),
            KeyCode::Esc => return Err(stopped()),
            KeyCode::Char('c') if control => return Err(stopped()),
            KeyCode::Backspace => {
                password.pop();
            }
            KeyCode::Char(c) if !control => password.push(c),
            _ => {}
        }
    }
}

fn restore() {
    let mut output = stdout();
    if KEYBOARD_ENHANCED.swap(false, Ordering::Relaxed) {
        let _ = execute!(output, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(
        output,
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen,
        Show
    );
    let _ = disable_raw_mode();
}

fn install_panic_hook(reports: Option<PathBuf>) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
        if let Some(folder) = &reports {
            let place = info.location().map(ToString::to_string);
            write_panic_report(folder, place.as_deref());
        }
    }));
}

/// Writes the place of a panic and its call stack. The message of a panic stays out,
/// because it can quote SQL or a value.
pub fn write_panic_report(folder: &Path, place: Option<&str>) -> Option<PathBuf> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs());
    let path = folder.join(format!("panic-{seconds}.txt"));
    let report = format!(
        "sqln {}\n{} {}\n{}\n\n{}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        place.unwrap_or("unknown place"),
        Backtrace::force_capture(),
    );
    file::write_private_atomic(&path, report.as_bytes(), Expect::Any).ok()?;
    crate::log::event("panic");
    Some(path)
}
