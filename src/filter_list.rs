//! A list with a filter: the core of the command palette and of the query history.

use crossterm::event::{KeyCode, KeyEvent};

use crate::line_input::{Edit, LineInput};

/// What a key asks from the owner of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListRequest {
    None,
    Cancel,
    /// A row, as a position in `shown`.
    Choose(usize),
}

/// Splits the text of a filter into lowercase words.
pub fn words_of(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_lowercase).collect()
}

/// Returns true when each word is in one of the lowercase texts.
pub fn holds_words(texts: &[&str], words: &[String]) -> bool {
    words
        .iter()
        .all(|word| texts.iter().any(|text| text.contains(word.as_str())))
}

#[derive(Clone, Debug)]
pub struct FilterList<T> {
    items: Vec<T>,
    /// The text of each item for the filter, in lowercase.
    texts: Vec<String>,
    input: LineInput,
    /// The positions in `items` that the filter keeps.
    shown: Vec<usize>,
    cursor: usize,
}

impl<T> FilterList<T> {
    pub fn new(items: Vec<T>, text_of: impl Fn(&T) -> String) -> Self {
        let texts = items.iter().map(|item| text_of(item).to_lowercase());
        let texts = texts.collect();
        let shown = (0..items.len()).collect();
        Self {
            items,
            texts,
            input: LineInput::default(),
            shown,
            cursor: 0,
        }
    }

    pub fn input(&self) -> &LineInput {
        &self.input
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Lists the items that the filter keeps.
    pub fn shown(&self) -> impl ExactSizeIterator<Item = &T> {
        self.shown.iter().map(|index| &self.items[*index])
    }

    /// Returns a row, as a position in `shown`.
    pub fn row(&self, index: usize) -> Option<&T> {
        self.shown.get(index).map(|index| &self.items[*index])
    }

    /// Keeps the items that hold each word of the filter.
    fn refilter(&mut self) {
        let words = words_of(self.input.text());
        let texts = &self.texts;
        self.shown = (0..self.items.len())
            .filter(|index| holds_words(&[&texts[*index]], &words))
            .collect();
        self.cursor = 0;
    }

    pub fn paste(&mut self, text: &str) {
        self.input.insert(text.lines().next().unwrap_or(""));
        self.refilter();
    }

    pub fn move_by(&mut self, rows: isize) {
        let last = self.shown.len().saturating_sub(1);
        self.cursor = self.cursor.saturating_add_signed(rows).min(last);
    }

    pub fn on_key(&mut self, key: KeyEvent) -> ListRequest {
        match key.code {
            KeyCode::Esc => ListRequest::Cancel,
            KeyCode::Enter => ListRequest::Choose(self.cursor),
            KeyCode::Up => {
                self.move_by(-1);
                ListRequest::None
            }
            KeyCode::Down => {
                self.move_by(1);
                ListRequest::None
            }
            _ => {
                if !matches!(self.input.on_key(key), Edit::None | Edit::Moved) {
                    self.refilter();
                }
                ListRequest::None
            }
        }
    }
}
