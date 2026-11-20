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

/// Returns the columns of a character on the screen.
pub fn char_columns(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

/// Returns the last characters of a text that fit `width` columns. The end of a path
/// holds its last name.
pub fn last_chars(chars: &[char], width: usize) -> String {
    let mut used = 0;
    let first = chars
        .iter()
        .rposition(|&c| {
            used += char_columns(c);
            used > width
        })
        .map_or(0, |index| index + 1);
    chars[first..].iter().collect()
}
