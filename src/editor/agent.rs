//! Revision-checked assistant edits and explicit execution ranges.

use super::{RunPlan, SqlEditor, buffer};
use crate::agent::{Edit, Error, Range, TEXT_BYTES};
use crate::sql::split;
use edtui::Index2;

#[derive(Clone, Copy, Default)]
pub(super) struct ReadCursor {
    revision: u64,
    byte: usize,
    line: usize,
    column: usize,
}

impl SqlEditor {
    /// Returns a bounded page without constructing a copy of the whole document.
    pub fn agent_text_page(&self, offset: usize) -> crate::agent::Reply {
        let saved = *self.agent_page.borrow();
        let mut cursor = if saved.revision == self.revision && saved.byte == offset {
            saved
        } else {
            ReadCursor {
                revision: self.revision,
                ..Default::default()
            }
        };
        let mut text = String::new();
        let mut more = false;
        while cursor.line < self.state.lines.len() {
            let row = buffer::row(&self.state.lines, cursor.line);
            let c = if let Some(c) = row.get(cursor.column) {
                *c
            } else if cursor.line + 1 < self.state.lines.len() {
                '\n'
            } else {
                break;
            };
            if cursor.byte < offset {
                if cursor.byte + c.len_utf8() > offset {
                    return Err(Error::new(
                        "invalid_offset",
                        "Use the next_offset from the previous page.",
                    ));
                }
            } else {
                if text.len() + c.len_utf8() > TEXT_BYTES {
                    more = true;
                    break;
                }
                text.push(c);
            }
            cursor.byte += c.len_utf8();
            if cursor.column == row.len() {
                cursor.line += 1;
                cursor.column = 0;
            } else {
                cursor.column += 1;
            }
        }
        if cursor.byte < offset {
            return Err(Error::new(
                "invalid_offset",
                "The text offset is outside this console.",
            ));
        }
        *self.agent_page.borrow_mut() = cursor;
        Ok(
            serde_json::json!({"text":text,"offset":offset,"next_offset":more.then_some(cursor.byte),"total_bytes":(!more).then_some(cursor.byte),"revision":self.revision,"lines":self.state.lines.len()}),
        )
    }

    pub fn agent_selection(&self) -> serde_json::Value {
        self.selection_range().map_or(serde_json::Value::Null, |(start,end)| serde_json::json!({"start":{"line":start.row,"column":start.col},"end":{"line":end.row,"column":end.col}}))
    }

    pub fn agent_plan(&mut self, range: Option<&Range>) -> Result<RunPlan, Error> {
        let (start, end) = match range {
            Some(range) => (point(range.start), point(range.end)),
            None => (Index2::new(0, 0), buffer::document_end(&self.state.lines)),
        };
        self.agent_check_range(start, end)?;
        let dialect = self.dialect;
        let snapshot = self.snapshot();
        let (base, stop) = (snapshot.offset(start), snapshot.offset(end));
        let text = &snapshot.text[base..stop];
        let parts = split(text, dialect);
        if parts.is_empty() {
            return Err(Error::new(
                "no_statement",
                "The requested range has no statement.",
            ));
        }
        Ok(RunPlan {
            statements: parts
                .iter()
                .map(|part| part.text(text).to_owned())
                .collect(),
            rows: parts
                .iter()
                .map(|part| snapshot.position(base + part.start).row)
                .collect(),
            starts: parts.iter().map(|part| base + part.start).collect(),
        })
    }

    pub fn agent_check_edits(&self, edits: &[Edit]) -> Result<(), Error> {
        if edits.is_empty() || edits.len() > 1000 {
            return Err(Error::new(
                "invalid_edit",
                "Supply between one and 1000 ordered edits.",
            ));
        }
        let mut previous = None;
        for edit in edits {
            let (start, end) = (point(edit.start), point(edit.end));
            self.agent_check_range(start, end)?;
            if previous.is_some_and(|previous| start < previous) {
                return Err(Error::new(
                    "invalid_edit",
                    "Use ordered, non-overlapping edit intervals.",
                ));
            }
            previous = Some(end);
        }
        Ok(())
    }

    fn agent_check_range(&self, start: Index2, end: Index2) -> Result<(), Error> {
        if start > end
            || end.row >= self.state.lines.len()
            || start.col > buffer::row(&self.state.lines, start.row).len()
            || end.col > buffer::row(&self.state.lines, end.row).len()
        {
            return Err(Error::new(
                "invalid_range",
                "Read the console again and use valid character positions.",
            ));
        }
        Ok(())
    }

    /// Returns the edit that replaces the whole text with `text`.
    pub fn agent_replace_all(&self, text: &str) -> Edit {
        let end = buffer::document_end(&self.state.lines);
        Edit {
            start: crate::agent::Position { line: 0, column: 0 },
            end: crate::agent::Position {
                line: end.row,
                column: end.col,
            },
            text: text.to_owned(),
        }
    }

    /// Applies one undo step and restores positions relative to the changed text.
    pub fn apply_agent_edits(&mut self, revision: u64, edits: &[Edit]) -> Result<(), Error> {
        if revision != self.revision {
            return Err(Error::new(
                "revision_conflict",
                "The console changed. Read it again and submit a new edit.",
            ));
        }
        self.agent_check_edits(edits)?;
        let normalized: Vec<_> = edits
            .iter()
            .map(|edit| super::buffer::normalize_newlines(&edit.text))
            .collect();
        let map = |mut pos| {
            for (edit, text) in edits.iter().zip(&normalized).rev() {
                pos = move_point(pos, point(edit.start), point(edit.end), text);
            }
            pos
        };
        let cursor = map(self.state.cursor);
        let anchor = self.anchor.map(map);
        let selection = self.state.selection.clone().map(|mut selection| {
            selection.start = map(selection.start);
            selection.end = map(selection.end);
            selection.anchor = selection.anchor.map(map);
            selection
        });
        let (left, top) = self.scroll();
        let scroll = map(Index2::new(top, left));
        let ranges: Vec<_> = edits
            .iter()
            .zip(&normalized)
            .map(|(edit, text)| (point(edit.start), point(edit.end), text.as_str()))
            .collect();
        self.replace_ranges(&ranges);
        self.state.cursor = cursor;
        self.anchor = anchor;
        self.state.selection = selection;
        self.set_scroll(scroll.col, scroll.row);
        self.sync_selection();
        Ok(())
    }
}

fn point(pos: crate::agent::Position) -> Index2 {
    Index2::new(pos.line, pos.column)
}
fn move_point(pos: Index2, start: Index2, end: Index2, text: &str) -> Index2 {
    if pos < start {
        return pos;
    }
    let mut added = Index2::new(start.row, start.col);
    for c in text.chars() {
        if c == '\n' {
            added.row += 1;
            added.col = 0;
        } else if c != '\r' {
            added.col += 1;
        }
    }
    if pos <= end {
        return added;
    }
    Index2::new(
        pos.row.saturating_add(added.row).saturating_sub(end.row),
        if pos.row == end.row {
            added.col + pos.col - end.col
        } else {
            pos.col
        },
    )
}
