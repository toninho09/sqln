//! A text field of one line, with a cursor counted in characters.

use std::ops::Range;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use zeroize::Zeroizing;

/// What a key did to the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    /// The key has no meaning for the field.
    None,
    /// The cursor moved, and the text stayed.
    Moved,
    /// The key wrote a character that a name can hold.
    Wrote,
    /// The key removed a character.
    Removed,
    /// The key wrote a character that ends a name, such as a space.
    Typed,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct LineInput {
    text: Zeroizing<String>,
    /// The position in characters, from 0 to the length.
    cursor: usize,
}

impl std::fmt::Debug for LineInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LineInput")
            .field("text", &"<redacted>")
            .field("cursor", &self.cursor)
            .finish()
    }
}

impl LineInput {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Returns the position of the cursor in bytes.
    pub fn byte_cursor(&self) -> usize {
        self.byte_at(self.cursor)
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.text
            .char_indices()
            .nth(chars)
            .map_or(self.text.len(), |(byte, _)| byte)
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// Replaces the text and puts the cursor at its end.
    pub fn set(&mut self, text: &str) {
        self.text = single_line(text);
        self.cursor = self.len();
    }

    /// Writes text at the cursor. A field holds one line, so the text has no
    /// line breaks and no other control characters.
    pub fn insert(&mut self, text: &str) {
        let line = single_line(text);
        let byte = self.byte_cursor();
        self.text.insert_str(byte, &line);
        self.cursor += line.chars().count();
    }

    /// Replaces a byte range of the text, and it puts the cursor after the new text.
    pub fn replace(&mut self, range: Range<usize>, with: &str) {
        let line = single_line(with);
        if range.start > range.end
            || range.end > self.text.len()
            || !self.text.is_char_boundary(range.start)
            || !self.text.is_char_boundary(range.end)
        {
            return;
        }
        let before = self.text[..range.start].chars().count();
        self.text.replace_range(range, &line);
        self.cursor = before + line.chars().count();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        self.delete();
    }

    fn delete(&mut self) {
        if self.cursor < self.len() {
            let byte = self.byte_cursor();
            self.text.remove(byte);
        }
    }

    /// Applies an editing key. Other keys have no effect.
    pub fn on_key(&mut self, key: KeyEvent) -> Edit {
        match key.code {
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.len(),
            KeyCode::Backspace => {
                self.backspace();
                return Edit::Removed;
            }
            KeyCode::Delete => {
                self.delete();
                return Edit::Removed;
            }
            KeyCode::Char(c) if (key.modifiers - KeyModifiers::SHIFT).is_empty() => {
                self.insert(c.encode_utf8(&mut [0; 4]));
                return if c.is_alphanumeric() || matches!(c, '_' | '.' | '"' | '`' | '[') {
                    Edit::Wrote
                } else {
                    Edit::Typed
                };
            }
            _ => return Edit::None,
        }
        Edit::Moved
    }
}

/// A line break or a tab becomes a space, so pasted lines stay apart.
fn single_line(text: &str) -> Zeroizing<String> {
    Zeroizing::new(
        text.chars()
            .filter_map(|c| match c {
                '\n' | '\t' => Some(' '),
                c if c.is_control() => None,
                c => Some(c),
            })
            .collect(),
    )
}
