//! The measurement of text in columns.

use unicode_width::UnicodeWidthChar;

/// Returns the part of a text that fits a width, and the columns that it uses.
pub fn fit(text: &str, width: usize) -> (String, usize) {
    let mut fitted = String::new();
    let mut used = 0;
    for c in text.chars() {
        let char_width = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + char_width > width {
            break;
        }
        fitted.push(c);
        used += char_width;
    }
    (fitted, used)
}

/// Returns the last characters of a text. The end of a path holds its last name.
pub fn last_chars(chars: &[char], width: usize) -> String {
    chars[chars.len().saturating_sub(width)..].iter().collect()
}
