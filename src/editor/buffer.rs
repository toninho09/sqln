//! Text operations on `Lines`. A position counts characters, and an end is exclusive.

use edtui::{Index2, Lines, RowIndex};

use crate::sql::{Dialect, Token, tokenize};

pub fn row(lines: &Lines, index: usize) -> &[char] {
    lines.get(RowIndex::new(index)).map_or(&[], Vec::as_slice)
}

pub fn last_row(lines: &Lines) -> usize {
    lines.len().saturating_sub(1)
}

pub fn document_end(lines: &Lines) -> Index2 {
    let last = last_row(lines);
    Index2::new(last, row(lines, last).len())
}

pub fn text_of(lines: &Lines) -> String {
    let mut text = String::with_capacity(lines.iter_row().map(Vec::len).sum::<usize>() + lines.len());
    for (index, row) in lines.iter_row().enumerate() {
        if index > 0 {
            text.push('\n');
        }
        text.extend(row);
    }
    text
}

pub fn text_in(lines: &Lines, start: Index2, end: Index2) -> String {
    if start.row == end.row {
        return row(lines, start.row)[start.col..end.col].iter().collect();
    }
    let mut text: String = row(lines, start.row)[start.col..].iter().collect();
    for index in start.row + 1..end.row {
        text.push('\n');
        text.extend(row(lines, index));
    }
    text.push('\n');
    text.extend(&row(lines, end.row)[..end.col]);
    text
}

/// Replaces a range with text and returns the position after the new text.
pub fn replace_range(lines: &mut Lines, start: Index2, end: Index2, text: &str) -> Index2 {
    let mut rows = std::mem::take(lines).into_vecs();
    if rows.is_empty() {
        rows.push(Vec::new());
    }
    let mut new_rows: Vec<Vec<char>> = text.split('\n').map(|row| row.chars().collect()).collect();
    new_rows[0].splice(0..0, rows[start.row][..start.col].iter().copied());
    let last = new_rows.len() - 1;
    let position = Index2::new(start.row + last, new_rows[last].len());
    new_rows[last].extend_from_slice(&rows[end.row][end.col..]);
    rows.splice(start.row..=end.row, new_rows);
    *lines = Lines::new(rows);
    position
}

/// Keeps one line ending, because the editor stores rows.
pub fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

pub fn previous_position(lines: &Lines, position: Index2) -> Index2 {
    if position.col > 0 {
        Index2::new(position.row, position.col - 1)
    } else if position.row > 0 {
        Index2::new(position.row - 1, row(lines, position.row - 1).len())
    } else {
        position
    }
}

pub fn next_position(lines: &Lines, position: Index2) -> Index2 {
    if position.col < row(lines, position.row).len() {
        Index2::new(position.row, position.col + 1)
    } else if position.row < last_row(lines) {
        Index2::new(position.row + 1, 0)
    } else {
        position
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

pub fn word_left(lines: &Lines, position: Index2) -> Index2 {
    if position.col == 0 {
        return previous_position(lines, position);
    }
    let chars = row(lines, position.row);
    let mut col = position.col;
    while col > 0 && chars[col - 1].is_whitespace() {
        col -= 1;
    }
    if col > 0 {
        let word = is_word_char(chars[col - 1]);
        while col > 0 && !chars[col - 1].is_whitespace() && is_word_char(chars[col - 1]) == word {
            col -= 1;
        }
    }
    Index2::new(position.row, col)
}

pub fn word_right(lines: &Lines, position: Index2) -> Index2 {
    let chars = row(lines, position.row);
    if position.col >= chars.len() {
        return next_position(lines, position);
    }
    let mut col = position.col;
    while col < chars.len() && chars[col].is_whitespace() {
        col += 1;
    }
    if col < chars.len() {
        let word = is_word_char(chars[col]);
        while col < chars.len() && !chars[col].is_whitespace() && is_word_char(chars[col]) == word {
            col += 1;
        }
    }
    Index2::new(position.row, col)
}

/// The flat text of one revision, with its tokens and its position table.
pub struct Snapshot {
    pub text: String,
    pub tokens: Vec<Token>,
    line_starts: Vec<usize>,
}

impl Snapshot {
    pub fn new(lines: &Lines, dialect: Dialect) -> Self {
        let text = text_of(lines);
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(offset, _)| offset + 1));
        let tokens = tokenize(&text, dialect);
        Self { text, tokens, line_starts }
    }

    pub fn line_start(&self, row: usize) -> usize {
        self.line_starts.get(row).copied().unwrap_or(self.text.len())
    }

    pub fn offset(&self, position: Index2) -> usize {
        let start = self.line_start(position.row);
        let line = &self.text[start..];
        let line = &line[..line.find('\n').unwrap_or(line.len())];
        start + line.char_indices().nth(position.col).map_or(line.len(), |(offset, _)| offset)
    }

    pub fn position(&self, offset: usize) -> Index2 {
        let row = self.line_starts.partition_point(|start| *start <= offset) - 1;
        Index2::new(row, self.text[self.line_starts[row]..offset].chars().count())
    }

    /// Returns the position of the last character before `offset`.
    pub fn position_before(&self, offset: usize) -> Index2 {
        let position = self.position(offset);
        match position.col {
            0 if offset > 0 => self.position(offset - 1),
            col => Index2::new(position.row, col.saturating_sub(1)),
        }
    }
}
