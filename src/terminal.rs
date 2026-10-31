//! Terminal setup and restoration.

use std::backtrace::Backtrace;
use std::io::{self, Stdout, stdout};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crossterm::cursor::Show;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    supports_keyboard_enhancement,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::store::file::{self, Expect};

pub type Screen = Terminal<CrosstermBackend<Stdout>>;

static KEYBOARD_ENHANCED: AtomicBool = AtomicBool::new(false);

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
        execute!(output, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
        // The request includes only the separation of keys, not a report of all keys.
        if supports_keyboard_enhancement().unwrap_or(false) {
            execute!(
                output,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )?;
            KEYBOARD_ENHANCED.store(true, Ordering::Relaxed);
        }
        let screen = Terminal::new(CrosstermBackend::new(output))?;
        Ok(Self { screen, _guard: guard })
    }
}

fn restore() {
    let mut output = stdout();
    if KEYBOARD_ENHANCED.swap(false, Ordering::Relaxed) {
        let _ = execute!(output, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(output, DisableBracketedPaste, DisableMouseCapture, LeaveAlternateScreen, Show);
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
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_secs());
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
