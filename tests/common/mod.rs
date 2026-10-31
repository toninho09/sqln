#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sqln::app::sample_catalog;
use sqln::catalog::Catalog;
use sqln::clipboard::MemoryClipboard;
use sqln::editor::{KeyboardMode, SqlEditor};
use sqln::sql::Dialect;

pub fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub fn shift(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

pub fn alt(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::ALT)
}

pub fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

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
        Self { editor, clipboard, catalog: sample_catalog() }
    }

    pub fn conventional(text: &str) -> Self {
        Self::new(text, KeyboardMode::Conventional)
    }

    pub fn vim(text: &str) -> Self {
        Self::new(text, KeyboardMode::Vim)
    }

    /// Sends one key through the path that the application uses.
    pub fn press(&mut self, key: KeyEvent) {
        if !self.editor.on_list_key(key) {
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
            list.completion.items.iter().map(|item| item.label.clone()).collect()
        })
    }

    pub fn clipboard_text(&self) -> String {
        self.clipboard.borrow().text.clone()
    }

    pub fn selected(&self) -> Option<String> {
        self.editor.selected_text()
    }
}

/// Returns a path for a database of one test. The file does not exist.
pub fn temp_db(name: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.db"));
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    path
}
