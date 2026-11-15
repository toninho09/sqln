//! Bounded text. Events and logs never copy an unlimited text.

use std::fmt::{self, Write};

pub const HISTORY_BYTES: usize = 10 * 1024 * 1024;
const TEXT_BYTES: usize = HISTORY_BYTES - 1024;
const TRUNCATED: &str = "\n[Text truncated.]";

/// A formatter that never copies an unlimited command into an event.
pub struct LimitedText {
    text: String,
    limit: usize,
    truncated: bool,
}

impl LimitedText {
    pub fn new(limit: usize) -> Self {
        Self {
            text: String::new(),
            limit,
            truncated: false,
        }
    }

    /// Returns the bytes of the kept text, without the truncation note.
    pub fn written(&self) -> usize {
        self.text.len()
    }

    pub fn finish(mut self) -> String {
        if self.truncated {
            self.text.push_str(TRUNCATED);
        }
        self.text.shrink_to_fit();
        self.text
    }
}

impl Write for LimitedText {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if self.truncated {
            return Err(fmt::Error);
        }
        let free = self.limit.saturating_sub(TRUNCATED.len() + self.text.len());
        let mut end = free.min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.text.push_str(&text[..end]);
        self.truncated = end != text.len();
        if self.truncated {
            Err(fmt::Error)
        } else {
            Ok(())
        }
    }
}

pub fn limited(text: impl fmt::Display) -> String {
    let mut out = LimitedText::new(TEXT_BYTES);
    let _ = write!(out, "{text}");
    out.finish()
}
