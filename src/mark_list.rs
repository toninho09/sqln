//! A list with a filter and a mark on each row: the choice of the visible schemas.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::filter_list::{FilterList, ListRequest};

/// What a key asks from the owner of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkRequest {
    None,
    Cancel,
    /// The marked names, in the order of the list.
    Done(Vec<String>),
}

#[derive(Clone, Debug)]
pub struct MarkList {
    list: FilterList<String>,
    marked: Vec<String>,
}

impl MarkList {
    /// The names keep their order. A marked name that is not in `names` comes after them,
    /// so the user can remove its mark.
    pub fn new(mut names: Vec<String>, marked: Vec<String>) -> Self {
        for name in &marked {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        Self {
            list: FilterList::new(names, Clone::clone),
            marked,
        }
    }

    pub fn list(&self) -> &FilterList<String> {
        &self.list
    }

    pub fn marked(&self) -> &[String] {
        &self.marked
    }

    pub fn is_marked(&self, name: &str) -> bool {
        self.marked.iter().any(|item| item == name)
    }

    pub fn move_by(&mut self, rows: isize) {
        self.list.move_by(rows);
    }

    pub fn paste(&mut self, text: &str) {
        self.list.paste(text);
    }

    /// Adds or removes the mark of a row, as a position in the shown rows.
    pub fn toggle(&mut self, row: usize) {
        let Some(name) = self.list.row(row).cloned() else {
            return;
        };
        if self.is_marked(&name) {
            self.marked.retain(|item| *item != name);
        } else {
            self.marked.push(name);
        }
    }

    /// `Space` adds or removes the mark of the cursor row. `Enter` ends the choice.
    pub fn on_key(&mut self, key: KeyEvent) -> MarkRequest {
        if key.code == KeyCode::Char(' ') && (key.modifiers - KeyModifiers::SHIFT).is_empty() {
            self.toggle(self.list.cursor());
            return MarkRequest::None;
        }
        match self.list.on_key(key) {
            ListRequest::None => MarkRequest::None,
            ListRequest::Cancel => MarkRequest::Cancel,
            ListRequest::Choose(_) => MarkRequest::Done(self.chosen()),
        }
    }

    /// Returns the marked names in the order of the list.
    fn chosen(&self) -> Vec<String> {
        let items = self.list.items().iter();
        items.filter(|name| self.is_marked(name)).cloned().collect()
    }
}
