//! Text operations on `Lines`. A position counts characters, and an end is exclusive.

use edtui::{Index2, Lines, RowIndex};
use unicode_width::UnicodeWidthStr;

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
    let mut text =
        String::with_capacity(lines.iter_row().map(Vec::len).sum::<usize>() + lines.len());
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

/// Replaces ranges in one pass, so that a long row with many ranges costs one copy. The
/// ranges come in the order of the text, and they do not overlap.
pub fn replace_ranges(lines: &mut Lines, ranges: &[(Index2, Index2, &str)]) {
    let mut rows = std::mem::take(lines).into_vecs();
    if rows.is_empty() {
        rows.push(Vec::new());
    }
    let last = rows.len() - 1;
    let end = Index2::new(last, rows[last].len());
    let mut done = Vec::with_capacity(rows.len());
    let mut open = Vec::new();
    let mut at = Index2::new(0, 0);
    for (start, stop, text) in ranges.iter().copied().chain([(end, end, "")]) {
        while at.row < start.row {
            open.extend_from_slice(&rows[at.row][at.col..]);
            done.push(std::mem::take(&mut open));
            at = Index2::new(at.row + 1, 0);
        }
        open.extend_from_slice(&rows[at.row][at.col..start.col]);
        let mut parts = text.split('\n');
        open.extend(parts.next().unwrap_or_default().chars());
        for part in parts {
            done.push(std::mem::take(&mut open));
            open.extend(part.chars());
        }
        at = stop;
    }
    done.push(open);
    *lines = Lines::new(done);
}

/// The result of a comment toggle on a group of rows.
pub struct Comment {
    /// The new text of the rows.
    pub text: String,
    /// The column and the change of length of each row.
    edits: Vec<(usize, isize)>,
}

impl Comment {
    /// Returns the place of a position after the change. `first` is the first row of the
    /// group, and a row after the group keeps its position.
    pub fn moved(&self, first: usize, position: Index2) -> Index2 {
        let edit = position.row.checked_sub(first);
        match edit.and_then(|index| self.edits.get(index)) {
            Some((at, change)) if position.col >= *at => Index2::new(
                position.row,
                position.col.saturating_add_signed(*change).max(*at),
            ),
            _ => position,
        }
    }
}

/// Adds or removes a line comment on a group of rows. A blank row stays as it is, and
/// only blank rows return `None`. Rows that all hold the comment lose it. Other rows
/// receive it at the smallest indent, so that the text keeps its form.
pub fn toggle_comment(lines: &Lines, first: usize, last: usize, marker: &str) -> Option<Comment> {
    let rows: Vec<&[char]> = (first..=last).map(|index| row(lines, index)).collect();
    let marker: Vec<char> = marker.chars().collect();
    let prefix: Vec<char> = marker.iter().copied().chain([' ']).collect();
    let start = |row: &[char]| row.iter().position(|c| !c.is_whitespace());
    let indent = rows.iter().filter_map(|row| start(row)).min()?;
    let remove = rows
        .iter()
        .all(|row| start(row).is_none_or(|at| row[at..].starts_with(&marker)));
    let mut text: Vec<String> = Vec::with_capacity(rows.len());
    let mut edits = Vec::with_capacity(rows.len());
    for row in rows {
        let (at, gone, added): (usize, usize, &[char]) = match start(row) {
            None => (0, 0, &[]),
            Some(at) if remove => {
                let spaced = row.get(at + marker.len()) == Some(&' ');
                (at, marker.len() + usize::from(spaced), &[])
            }
            Some(_) => (indent, 0, &prefix),
        };
        text.push(
            row[..at]
                .iter()
                .chain(added)
                .chain(&row[at + gone..])
                .collect(),
        );
        edits.push((at, added.len().cast_signed() - gone.cast_signed()));
    }
    Some(Comment {
        text: text.join("\n"),
        edits,
    })
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

/// Returns the display width with the tab width of the editor.
pub(super) fn display_width(text: &str) -> usize {
    let mut parts = text.split('\t');
    parts.next().unwrap_or_default().width()
        + parts
            .map(|part| super::TAB_WIDTH + part.width())
            .sum::<usize>()
}

/// The flat text of one revision, with its tokens and its position table.
pub struct Snapshot {
    pub text: String,
    pub tokens: Vec<Token>,
    pub json: Option<crate::grid::json::Highlights>,
    pub json_validation: Option<crate::grid::json::Validation>,
    pub max_width: usize,
    pub statements: std::cell::OnceCell<Vec<crate::sql::Statement>>,
    pub run_selection: Option<super::run::Selection>,
    line_starts: Vec<usize>,
    character_starts: Vec<usize>,
}

impl Snapshot {
    pub fn new(lines: &Lines, dialect: Dialect, literal_text: bool) -> Self {
        let text = text_of(lines);
        let max_width = text.split('\n').map(display_width).max().unwrap_or(0);
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(offset, _)| offset + 1));
        let mut character = 0;
        let character_starts = text
            .split('\n')
            .map(|line| {
                let start = character;
                character += line.chars().count() + 1;
                start
            })
            .collect();
        let json_validation = literal_text
            .then(|| crate::grid::json::validation(&text))
            .flatten();
        let json = matches!(json_validation, Some(crate::grid::json::Validation::Valid))
            .then(|| crate::grid::json::Highlights::validated(&text));
        let tokens = if json.is_some() {
            Vec::new()
        } else {
            tokenize(&text, dialect)
        };
        Self {
            text,
            tokens,
            json,
            json_validation,
            max_width,
            statements: std::cell::OnceCell::new(),
            run_selection: None,
            line_starts,
            character_starts,
        }
    }

    pub fn line_start(&self, row: usize) -> usize {
        self.line_starts
            .get(row)
            .copied()
            .unwrap_or(self.text.len())
    }

    pub fn character_offset(&self, position: Index2) -> usize {
        self.character_starts
            .get(position.row)
            .copied()
            .unwrap_or(0)
            + position.col
    }

    pub fn offset(&self, position: Index2) -> usize {
        let start = self.line_start(position.row);
        let line = &self.text[start..];
        let line = &line[..line.find('\n').unwrap_or(line.len())];
        start
            + line
                .char_indices()
                .nth(position.col)
                .map_or(line.len(), |(offset, _)| offset)
    }

    pub fn row_of(&self, offset: usize) -> usize {
        self.line_starts.partition_point(|start| *start <= offset) - 1
    }

    pub fn position(&self, offset: usize) -> Index2 {
        let row = self.row_of(offset);
        Index2::new(
            row,
            self.text[self.line_starts[row]..offset].chars().count(),
        )
    }

    /// Returns the position of the last character before `offset`.
    pub fn position_before(&self, offset: usize) -> Index2 {
        let position = self.position(offset);
        match position.col {
            0 if offset > 0 => self.position(offset - 1),
            col => Index2::new(position.row, col.saturating_sub(1)),
        }
    }

    /// Lists JSON tokens on the visible character columns of a row.
    pub fn row_json_tokens(
        &self,
        row: usize,
        left: usize,
        end: usize,
    ) -> Vec<(crate::grid::json::Kind, usize, usize)> {
        let Some(json) = &self.json else {
            return Vec::new();
        };
        let start = self.offset(Index2::new(row, left));
        let stop = self.offset(Index2::new(row, end));
        let (mut at, mut col) = (start, left);
        json.in_range(start..stop)
            .map(|(range, kind)| {
                col += self.text[at..range.start].chars().count();
                let first = col;
                col += self.text[range.clone()].chars().count();
                at = range.end;
                (kind, first, col - 1)
            })
            .collect()
    }

    /// Lists the tokens on the columns `left..end` of a row. Each one comes with its
    /// first and its last column there, because a token can go past these columns.
    pub fn row_tokens(&self, row: usize, left: usize, end: usize) -> Vec<(&Token, usize, usize)> {
        let start = self.offset(Index2::new(row, left));
        let stop = self.offset(Index2::new(row, end));
        let first = self.tokens.partition_point(|token| token.end() <= start);
        let mut found = Vec::new();
        let (mut at, mut col) = (start, left);
        for token in &self.tokens[first..] {
            let (from, to) = (token.start().max(start), token.end().min(stop));
            if from >= stop {
                break;
            }
            if from < to {
                col += self.text[at..from].chars().count();
                let first_col = col;
                col += self.text[from..to].chars().count();
                at = to;
                found.push((token, first_col, col - 1));
            }
        }
        found
    }
}
