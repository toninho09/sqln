//! Terminal setup and restoration.

use std::io::{self, Stdout, stdout};
use std::sync::atomic::{AtomicBool, Ordering};

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
    pub fn start() -> io::Result<Self> {
        install_panic_hook();
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

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
}
