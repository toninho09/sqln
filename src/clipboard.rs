//! Clipboard access. The system clipboard has priority, and OSC 52 is the alternative.

use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;

use base64::Engine;
use edtui::clipboard::ClipboardTrait;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyOutcome {
    Copied,
    /// The terminal received the request, but it does not confirm the result.
    SentToTerminal,
    Failed,
}

pub trait Clipboard {
    fn copy(&mut self, text: &str) -> CopyOutcome;
    fn paste(&mut self) -> Option<String>;
}

pub type SharedClipboard = Rc<RefCell<dyn Clipboard>>;

/// Keeps the text in memory. Tests use it, so they do not change the system clipboard.
#[derive(Default)]
pub struct MemoryClipboard {
    pub text: String,
}

impl Clipboard for MemoryClipboard {
    fn copy(&mut self, text: &str) -> CopyOutcome {
        self.text = text.to_owned();
        CopyOutcome::Copied
    }

    fn paste(&mut self) -> Option<String> {
        Some(self.text.clone())
    }
}

#[derive(Default)]
pub struct SystemClipboard {
    system: Option<arboard::Clipboard>,
}

impl SystemClipboard {
    pub fn new() -> Self {
        Self {
            system: arboard::Clipboard::new().ok(),
        }
    }
}

impl Clipboard for SystemClipboard {
    fn copy(&mut self, text: &str) -> CopyOutcome {
        if let Some(system) = &mut self.system
            && system.set_text(text).is_ok()
        {
            return CopyOutcome::Copied;
        }
        write_osc52(&mut std::io::stdout(), text)
    }

    fn paste(&mut self) -> Option<String> {
        self.system.as_mut()?.get_text().ok()
    }
}

/// Asks the terminal to copy the text.
pub fn write_osc52(output: &mut impl Write, text: &str) -> CopyOutcome {
    let sequence = format!("\x1b]52;c;{}\x07", base64(text.as_bytes()));
    match output
        .write_all(sequence.as_bytes())
        .and_then(|()| output.flush())
    {
        Ok(()) => CopyOutcome::SentToTerminal,
        Err(_) => CopyOutcome::Failed,
    }
}

/// Writes the text of `base64`.
pub fn base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Reads the text of `base64`. It returns `None` for text that `base64` cannot
/// write, including a value whose last symbol holds unused bits.
pub fn base64_bytes(text: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(text).ok()
}

/// Connects the Vim commands of EdTUI to the clipboard of the application.
pub struct EditorClipboard(pub SharedClipboard);

impl ClipboardTrait for EditorClipboard {
    fn set_text(&mut self, text: String) {
        self.0.borrow_mut().copy(&text);
    }

    fn get_text(&mut self) -> String {
        self.0.borrow_mut().paste().unwrap_or_default()
    }
}
