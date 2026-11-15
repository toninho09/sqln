use std::cell::RefCell;
use std::rc::Rc;

use crossterm::event::{KeyCode, KeyEvent};
use sqln_core::catalog::Catalog;
use sqln_core::clipboard::MemoryClipboard;
use sqln_core::input::SuggestionKeys;
use sqln_sql::sql::Dialect;
use sqln_ui::editor::{KeyboardMode, SqlEditor};

use crate::{key, sample_catalog};

pub struct Fixture {
    pub editor: SqlEditor,
    pub clipboard: Rc<RefCell<MemoryClipboard>>,
    pub catalog: Catalog,
}

impl Fixture {
    pub fn new(text: &str, keyboard: KeyboardMode) -> Self {
        Self::with_dialect(text, keyboard, Dialect::Postgres)
    }

    pub fn with_dialect(text: &str, keyboard: KeyboardMode, dialect: Dialect) -> Self {
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        let editor = SqlEditor::new(text, keyboard, dialect, clipboard.clone());
        Self {
            editor,
            clipboard,
            catalog: sample_catalog(),
        }
    }

    pub fn conventional(text: &str) -> Self {
        Self::new(text, KeyboardMode::Conventional)
    }

    pub fn vim(text: &str) -> Self {
        Self::new(text, KeyboardMode::Vim)
    }

    /// Sends one key through the path that the application uses.
    pub fn press(&mut self, key: KeyEvent) {
        if !self.editor.on_list_key(key, SuggestionKeys::Tab) {
            self.editor.on_key(key, Some(&self.catalog));
        }
    }

    pub fn press_all(&mut self, keys: &[KeyEvent]) {
        for key in keys {
            self.press(*key);
        }
    }

    pub fn write(&mut self, text: &str) {
        for c in text.chars() {
            self.press(key(KeyCode::Char(c)));
        }
    }

    pub fn text(&self) -> String {
        self.editor.text()
    }

    pub fn labels(&self) -> Vec<String> {
        self.editor.suggestions().map_or_else(Vec::new, |list| {
            list.completion
                .items
                .iter()
                .map(|item| item.label.clone())
                .collect()
        })
    }

    pub fn clipboard_text(&self) -> String {
        self.clipboard.borrow().text.clone()
    }

    pub fn selected(&self) -> Option<String> {
        self.editor.selected_text()
    }
}
