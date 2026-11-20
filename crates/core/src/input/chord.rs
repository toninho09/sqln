//! A key with its modifiers, and its configuration text.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl Chord {
    /// The Esc key, without modifiers.
    pub const ESC: Self = Self {
        code: KeyCode::Esc,
        modifiers: KeyModifiers::NONE,
    };

    /// Ctrl and a lowercase character, the form of `Chord::parse("ctrl+<c>")`.
    pub const fn ctrl(c: char) -> Self {
        Self {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::CONTROL,
        }
    }

    pub fn from_event(key: KeyEvent) -> Self {
        let mut modifiers =
            key.modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
        // A character holds its case, so Shift adds nothing.
        if matches!(key.code, KeyCode::Char(_)) {
            modifiers -= KeyModifiers::SHIFT;
        }
        let code = if key.code == KeyCode::BackTab {
            modifiers |= KeyModifiers::SHIFT;
            KeyCode::Tab
        } else {
            key.code
        };
        Self { code, modifiers }
    }

    /// Returns the configuration text of a key.
    pub fn text(self) -> Result<String, String> {
        let name = match self.code {
            KeyCode::Char(' ') => "space".to_owned(),
            KeyCode::Char(c) if !c.is_control() && !c.is_whitespace() => c.to_string(),
            KeyCode::F(n) if n > 0 => format!("f{n}"),
            KeyCode::Enter => "enter".to_owned(),
            KeyCode::Esc => "esc".to_owned(),
            KeyCode::Tab => "tab".to_owned(),
            KeyCode::Backspace => "backspace".to_owned(),
            KeyCode::Delete => "delete".to_owned(),
            KeyCode::Insert => "insert".to_owned(),
            KeyCode::Home => "home".to_owned(),
            KeyCode::End => "end".to_owned(),
            KeyCode::PageUp => "pageup".to_owned(),
            KeyCode::PageDown => "pagedown".to_owned(),
            KeyCode::Up => "up".to_owned(),
            KeyCode::Down => "down".to_owned(),
            KeyCode::Left => "left".to_owned(),
            KeyCode::Right => "right".to_owned(),
            _ => return Err("This key cannot be recorded.".to_owned()),
        };
        let mut parts = Vec::new();
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            parts.push("ctrl");
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            parts.push("alt");
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            parts.push("shift");
        }
        parts.push(&name);
        Ok(parts.join("+"))
    }

    /// Returns true for `Ctrl+W`, the key before the commands of the application.
    pub fn is_prefix(self) -> bool {
        self.code == KeyCode::Char('w') && self.modifiers == KeyModifiers::CONTROL
    }

    /// Returns true for a character without Ctrl or Alt: during input, it is text.
    pub fn is_plain_char(self) -> bool {
        matches!(self.code, KeyCode::Char(_)) && (self.modifiers - KeyModifiers::SHIFT).is_empty()
    }

    /// Returns the same key without Shift, when the key has Shift.
    pub fn without_shift(self) -> Option<Self> {
        self.modifiers.contains(KeyModifiers::SHIFT).then(|| Self {
            modifiers: self.modifiers - KeyModifiers::SHIFT,
            ..self
        })
    }

    /// Reads a key such as `ctrl+enter`, `shift+f5`, `alt+up`, or `<`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (modifier_text, name) = match text.rsplit_once('+') {
            Some((start, "")) => (start.strip_suffix('+').unwrap_or(start), "+"),
            Some(parts) => parts,
            None => ("", text),
        };
        let mut modifiers = KeyModifiers::NONE;
        for modifier in modifier_text
            .split('+')
            .filter(|modifier| !modifier.is_empty())
        {
            modifiers |= match modifier.to_ascii_lowercase().as_str() {
                "ctrl" => KeyModifiers::CONTROL,
                "alt" => KeyModifiers::ALT,
                "shift" => KeyModifiers::SHIFT,
                _ => return Err(format!("Unknown key: {text}")),
            };
        }
        let code = key_code(name).ok_or_else(|| format!("Unknown key: {text}"))?;
        Ok(Self::from_event(KeyEvent::new(code, modifiers)))
    }
}

pub(super) fn key_code(name: &str) -> Option<KeyCode> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Some(KeyCode::Char(c));
    }
    let code = match name.to_ascii_lowercase().as_str() {
        "enter" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "space" => KeyCode::Char(' '),
        other => KeyCode::F(other.strip_prefix('f')?.parse().ok()?),
    };
    Some(code)
}
