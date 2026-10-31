//! Literal text search without case. Both keyboard modes use this bar.

use edtui::{Index2, Lines};

use crate::find::{fold, needle};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchField {
    #[default]
    Find,
    Replace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Match {
    pub start: Index2,
    pub end: Index2,
}

#[derive(Default)]
pub struct SearchBar {
    pub find: String,
    pub replace: String,
    pub field: SearchField,
    pub matches: Vec<Match>,
    pub current: usize,
}

impl SearchBar {
    pub fn focused_text(&mut self) -> &mut String {
        match self.field {
            SearchField::Find => &mut self.find,
            SearchField::Replace => &mut self.replace,
        }
    }

    pub fn toggle_field(&mut self) {
        self.field = match self.field {
            SearchField::Find => SearchField::Replace,
            SearchField::Replace => SearchField::Find,
        };
    }

    /// Recalculates the matches, and it selects the first one at or after `from`.
    pub fn refresh(&mut self, lines: &Lines, from: Index2) {
        self.matches = find_matches(lines, &self.find);
        self.current = self
            .matches
            .iter()
            .position(|found| found.start >= from)
            .unwrap_or_default();
    }

    pub fn current_match(&self) -> Option<Match> {
        self.matches.get(self.current).copied()
    }

    pub fn advance(&mut self, forward: bool) {
        let count = self.matches.len();
        if count > 0 {
            self.current = (self.current + if forward { 1 } else { count - 1 }) % count;
        }
    }
}

fn find_matches(lines: &Lines, query: &str) -> Vec<Match> {
    let needle = needle(query);
    if needle.is_empty() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    for (row, chars) in lines.iter_row().enumerate() {
        let mut col = 0;
        while col + needle.len() <= chars.len() {
            if chars[col..col + needle.len()]
                .iter()
                .zip(&needle)
                .all(|(a, b)| fold(*a) == *b)
            {
                matches.push(Match {
                    start: Index2::new(row, col),
                    end: Index2::new(row, col + needle.len()),
                });
                col += needle.len();
            } else {
                col += 1;
            }
        }
    }
    matches
}
