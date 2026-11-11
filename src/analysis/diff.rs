use std::collections::HashMap;

use super::Output;
use crate::compare::data::{key_hash, same};
use crate::grid::{ResultTable, Row, Value, printable_char};

const CHUNK_BYTES: usize = 4096;

pub(super) struct Diff {
    key: Vec<usize>,
    previous_index: HashMap<u64, Vec<usize>>,
    current_index: HashMap<u64, Vec<usize>>,
    matched: Vec<bool>,
    phase: u8,
    row: usize,
    added: usize,
    removed: usize,
    changed: usize,
    values: Option<RowValues>,
}

impl Diff {
    pub fn new(key: Vec<usize>) -> Self {
        Self {
            key,
            previous_index: HashMap::new(),
            current_index: HashMap::new(),
            matched: Vec::new(),
            phase: 0,
            row: 0,
            added: 0,
            removed: 0,
            changed: 0,
            values: None,
        }
    }

    fn keys_equal(&self, a: &Row, b: &Row) -> bool {
        self.key
            .iter()
            .all(|&column| same(cell(a, column), cell(b, column)))
    }

    pub fn step(
        &mut self,
        current: &ResultTable,
        previous: &ResultTable,
        output: &mut Output,
    ) -> Result<bool, &'static str> {
        if let Some(values) = &mut self.values {
            if values.step(current, previous, output)? {
                self.values = None;
            }
            return Ok(false);
        }
        if self.phase == 0 {
            if let Some(row) = previous.rows.get(self.row) {
                let hash = key_hash(row, self.key.iter().copied());
                if self.previous_index.get(&hash).is_some_and(|places| {
                    places
                        .iter()
                        .any(|&place| self.keys_equal(&previous.rows[place], row))
                }) {
                    return Err(
                        "Comparison canceled: duplicate key in Previous. Select more key columns.",
                    );
                }
                output.memory.reserve(128)?;
                self.previous_index.entry(hash).or_default().push(self.row);
                self.matched.push(false);
                self.row += 1;
                return Ok(false);
            }
            self.phase = 1;
            self.row = 0;
        }
        if self.phase == 1 {
            if let Some(row) = current.rows.get(self.row) {
                let hash = key_hash(row, self.key.iter().copied());
                if self.current_index.get(&hash).is_some_and(|places| {
                    places
                        .iter()
                        .any(|&place| self.keys_equal(&current.rows[place], row))
                }) {
                    return Err(
                        "Comparison canceled: duplicate key in Current. Select more key columns.",
                    );
                }
                output.memory.reserve(128)?;
                self.current_index.entry(hash).or_default().push(self.row);
                let found = self.previous_index.get(&hash).and_then(|places| {
                    places
                        .iter()
                        .copied()
                        .find(|&place| self.keys_equal(&previous.rows[place], row))
                });
                match found {
                    None => {
                        self.added += 1;
                        output.line(&format!("Added: Current row {}", self.row + 1))?;
                        self.values = Some(RowValues::new(true, self.row, None));
                    }
                    Some(place) => {
                        self.matched[place] = true;
                        let old = &previous.rows[place];
                        if old.cells().zip(row.cells()).any(|(a, b)| !same(a, b)) {
                            self.changed += 1;
                            output.line(&format!(
                                "Changed: Previous row {} -> Current row {}",
                                place + 1,
                                self.row + 1
                            ))?;
                            self.values = Some(RowValues::new(true, self.row, Some(place)));
                        }
                    }
                }
                self.row += 1;
                return Ok(false);
            }
            self.phase = 2;
            self.row = 0;
        }
        if self.row < previous.rows.len() {
            if !self.matched[self.row] {
                self.removed += 1;
                output.line(&format!("Removed: Previous row {}", self.row + 1))?;
                self.values = Some(RowValues::new(false, self.row, None));
            }
            self.row += 1;
            return Ok(false);
        }
        output.line(&format!(
            "Summary: {} added, {} removed, {} changed",
            self.added, self.removed, self.changed
        ))?;
        Ok(true)
    }
}

fn cell(row: &Row, column: usize) -> Value<'_> {
    row.get(column).unwrap_or(Value::Null)
}

struct RowValues {
    current: bool,
    row: usize,
    old: Option<usize>,
    column: usize,
    part: usize,
    offset: usize,
}

impl RowValues {
    fn new(current: bool, row: usize, old: Option<usize>) -> Self {
        Self {
            current,
            row,
            old,
            column: 0,
            part: 0,
            offset: 0,
        }
    }

    fn step(
        &mut self,
        current: &ResultTable,
        previous: &ResultTable,
        output: &mut Output,
    ) -> Result<bool, &'static str> {
        let table = if self.current { current } else { previous };
        let Some(name) = table.columns.get(self.column) else {
            return Ok(true);
        };
        let value = cell(&table.rows[self.row], self.column);
        let before = self.old.map(|row| cell(&previous.rows[row], self.column));
        if self.part == 0 && before.is_some_and(|before| same(before, value)) {
            self.column += 1;
            return Ok(false);
        }
        let parts = [
            Part::Literal("  "),
            Part::Text(name),
            Part::Literal(": "),
            Part::value(before.unwrap_or(value)),
            Part::Literal(if before.is_some() { " -> " } else { "" }),
            before.map_or(Part::Literal(""), |_| Part::value(value)),
            Part::Literal("\n"),
        ];
        if parts[self.part].write(output, &mut self.offset)? {
            self.offset = 0;
            self.part += 1;
            if self.part == parts.len() {
                self.part = 0;
                self.column += 1;
            }
        }
        Ok(false)
    }
}

enum Part<'a> {
    Literal(&'a str),
    Text(&'a str),
    Hex(&'a [u8]),
}

impl<'a> Part<'a> {
    fn value(value: Value<'a>) -> Self {
        match value {
            Value::Null => Self::Literal("NULL"),
            Value::Text(text) | Value::Number(text) => Self::Text(text),
            Value::Binary(bytes) => bytes.text().map_or(Self::Hex(bytes.bytes()), Self::Text),
        }
    }

    /// Reserves each output chunk before conversion. No complete cell copy is needed.
    fn write(&self, output: &mut Output, offset: &mut usize) -> Result<bool, &'static str> {
        match self {
            Self::Literal(text) => {
                output.memory.reserve(text.len() * 2)?;
                output.text.push_str(text);
                Ok(true)
            }
            Self::Text(text) => {
                let mut end = (*offset + CHUNK_BYTES).min(text.len());
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                let chunk = &text[*offset..end];
                let bytes: usize = chunk.chars().map(printable_char).map(char::len_utf8).sum();
                output.memory.reserve(bytes * 2)?;
                output.text.extend(chunk.chars().map(printable_char));
                *offset = end;
                Ok(end == text.len())
            }
            Self::Hex(bytes) => {
                let end = (*offset + CHUNK_BYTES / 2).min(bytes.len());
                let prefix = if *offset == 0 { "0x" } else { "" };
                output
                    .memory
                    .reserve((prefix.len() + (end - *offset) * 2) * 2)?;
                output.text.push_str(prefix);
                for byte in &bytes[*offset..end] {
                    const HEX: &[u8; 16] = b"0123456789ABCDEF";
                    output.text.push(char::from(HEX[usize::from(byte >> 4)]));
                    output.text.push(char::from(HEX[usize::from(byte & 15)]));
                }
                *offset = end;
                Ok(end == bytes.len())
            }
        }
    }
}
