//! The key map: the action of each key in each region.
//!
//! A binding is one key or a sequence of keys, such as `ctrl+w e`. The editor keys
//! stay in `editor`, because the EdTUI handler owns the Vim mode.

mod action;
mod catalog;
mod chord;
mod keymap;

pub use self::action::{Action, Grid, Region};
use self::action::{Grids, Item, both};
use self::catalog::DEFAULTS;
pub use self::chord::Chord;
use self::keymap::default_keys;
pub use self::keymap::{Keymap, Lookup};

use crossterm::event::{KeyCode, KeyEvent};
use serde::{Deserialize, Serialize};

/// The key scheme of the editor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyboardMode {
    #[default]
    Conventional,
    Vim,
}

/// The keys that accept the item of a suggestion list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SuggestionKeys {
    #[default]
    Tab,
    Enter,
    Both,
}

impl SuggestionKeys {
    /// Returns true when the key accepts the item of the list.
    pub fn accepts(self, key: KeyEvent) -> bool {
        let chosen = match key.code {
            KeyCode::Tab => self != Self::Enter,
            KeyCode::Enter => self != Self::Tab,
            _ => false,
        };
        chosen && key.modifiers.is_empty()
    }
}
