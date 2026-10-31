//! The text comparison of the searches: literal and without case.

pub fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

pub fn needle(query: &str) -> Vec<char> {
    query.chars().map(fold).collect()
}

/// Returns true when `text` holds the text of `needle`, which must be folded.
pub fn contains(text: &str, needle: &[char]) -> bool {
    let Some((first, rest)) = needle.split_first() else { return false };
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if fold(c) == *first {
            let mut after = chars.clone();
            if rest.iter().all(|expected| after.next().is_some_and(|c| fold(c) == *expected)) {
                return true;
            }
        }
    }
    false
}
