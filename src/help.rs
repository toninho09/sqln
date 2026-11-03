//! The searchable groups of the key reference.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::line_input::LineInput;
use crate::ui::which_key::KeyGroup;

pub struct Help {
    pub title: String,
    pub groups: Vec<KeyGroup>,
    pub input: LineInput,
    /// `None` shows all groups.
    pub group: Option<usize>,
    pub scroll: usize,
    pub visible_rows: usize,
}

impl Help {
    pub fn new(title: String, groups: Vec<KeyGroup>) -> Self {
        Self {
            title,
            groups: groups
                .into_iter()
                .filter(|group| !group.keys.is_empty())
                .collect(),
            input: LineInput::default(),
            group: None,
            scroll: 0,
            visible_rows: 1,
        }
    }

    /// Returns the full reference for the clipboard.
    pub fn text(&self) -> String {
        self.groups
            .iter()
            .map(|group| {
                let rows = group
                    .keys
                    .iter()
                    .map(|(key, label)| format!("{key:<24}{label}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{}\n{rows}", group.title)
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Returns true when every search word matches the group, key, or description.
    pub fn matches(&self, group: &KeyGroup, key: &str, label: &str) -> bool {
        let text = format!("{} {key} {label}", group.title).to_lowercase();
        self.input
            .text()
            .split_whitespace()
            .all(|word| text.contains(&word.to_lowercase()))
    }

    pub fn paste(&mut self, text: &str) {
        self.input.insert(text);
        self.scroll = 0;
    }

    /// Returns true when the key closes the reference. Other keys never run commands.
    pub fn on_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::F(1) => return true,
            KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Down => self.scroll = self.scroll.saturating_add(1),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(self.visible_rows),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(self.visible_rows),
            KeyCode::Char('u') if ctrl => {
                self.input.set("");
                self.scroll = 0;
            }
            KeyCode::Home if ctrl => self.scroll = 0,
            KeyCode::End if ctrl => self.scroll = usize::MAX,
            KeyCode::Tab | KeyCode::BackTab => {
                let count = self.groups.len() + 1;
                let current = self.group.map_or(0, |index| index + 1);
                let previous =
                    key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT);
                let next = if previous {
                    (current + count - 1) % count
                } else {
                    (current + 1) % count
                };
                self.group = next.checked_sub(1);
                self.scroll = 0;
            }
            _ => {
                let before = self.input.text().to_owned();
                self.input.on_key(key);
                if self.input.text() != before {
                    self.scroll = 0;
                }
            }
        }
        false
    }
}
