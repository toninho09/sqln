//! Clipboard access. The system clipboard has priority, and OSC 52 is the alternative.

use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;

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

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = chunk
            .iter()
            .fold(0u32, |value, byte| (value << 8) | u32::from(*byte))
            << (8 * (3 - chunk.len()));
        for index in 0..4 {
            if index <= chunk.len() {
                encoded.push(BASE64_ALPHABET[(value >> (18 - 6 * index)) as usize & 63] as char);
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

/// Reads the text of `base64`. It returns `None` for text that `base64` cannot write.
pub fn base64_bytes(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let body = text.trim_end_matches('=').as_bytes();
    if bytes.len() - body.len() > 2 {
        return None;
    }
    let mut decoded = Vec::with_capacity(body.len() / 4 * 3 + 2);
    for chunk in body.chunks(4) {
        let mut value = 0u32;
        for byte in chunk {
            let digit = BASE64_ALPHABET.iter().position(|item| item == byte)?;
            value = (value << 6) | digit as u32;
        }
        let bits = value << (6 * (4 - chunk.len()));
        let count = chunk.len() - 1;
        decoded.extend_from_slice(&bits.to_be_bytes()[1..=count]);
    }
    Some(decoded)
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
