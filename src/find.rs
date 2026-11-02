//! The text comparison of the searches: literal and without case.

pub fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

pub fn needle(query: &str) -> Vec<char> {
    query.chars().map(fold).collect()
}

/// A streaming KMP matcher. A caller can pause between characters.
#[derive(Debug, Default)]
pub(crate) struct Matcher {
    needle: Vec<char>,
    prefixes: Vec<usize>,
    matched: usize,
}

impl Matcher {
    pub fn new(query: &str) -> Self {
        let needle = needle(query);
        let mut prefixes = vec![0; needle.len()];
        let mut matched = 0;
        for index in 1..needle.len() {
            while matched > 0 && needle[index] != needle[matched] {
                matched = prefixes[matched - 1];
            }
            if needle[index] == needle[matched] {
                matched += 1;
            }
            prefixes[index] = matched;
        }
        Self {
            needle,
            prefixes,
            matched: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.needle.is_empty()
    }

    pub fn reset(&mut self) {
        self.matched = 0;
    }

    /// Returns None when a prefix fallback needs another step for this character.
    pub fn push(&mut self, c: char) -> Option<bool> {
        if self.is_empty() {
            return Some(false);
        }
        let c = fold(c);
        if self.matched > 0 && c != self.needle[self.matched] {
            self.matched = self.prefixes[self.matched - 1];
            return None;
        }
        if c == self.needle[self.matched] {
            self.matched += 1;
        }
        if self.matched == self.needle.len() {
            self.matched = self.prefixes[self.matched - 1];
            Some(true)
        } else {
            Some(false)
        }
    }
}

/// Returns true when `text` holds the text of `needle`, which must be folded.
pub fn contains(text: &str, needle: &[char]) -> bool {
    let Some((first, rest)) = needle.split_first() else {
        return false;
    };
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if fold(c) == *first {
            let mut after = chars.clone();
            if rest
                .iter()
                .all(|expected| after.next().is_some_and(|c| fold(c) == *expected))
            {
                return true;
            }
        }
    }
    false
}
