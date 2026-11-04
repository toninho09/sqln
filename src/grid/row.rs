//! One row of a result in one block of memory. A block for each value costs more than
//! the text of a usual value, so the values of a row stay together.
//!
//! Each cell is a head and the bytes of its value. The first byte of the head holds the
//! kind of the value in its low bits and the length in its high bits. A value of
//! `LONG` bytes or more has the top length there, and the rest of its length comes
//! next, in groups of 7 bits with the high bit set on each byte before the last.

use std::fmt;

use super::{Binary, Value};

const KIND_BITS: u32 = 3;
const KIND_MASK: u8 = (1 << KIND_BITS) - 1;
const LONG: usize = 31;
const GROUP_BITS: u32 = 7;
const GROUP_MASK: u8 = (1 << GROUP_BITS) - 1;
const MORE: u8 = 1 << GROUP_BITS;

const NULL: u8 = 0;
const TEXT: u8 = 1;
const NUMBER: u8 = 2;
const BINARY_TEXT: u8 = 3;
const BINARY_HEX: u8 = 4;

/// A row of this size or less leaves its bytes in the writer for the next row.
const REUSED_BYTES: usize = 64 * 1024;

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Row {
    data: Box<[u8]>,
}

impl Row {
    pub fn cells(&self) -> Cells<'_> {
        Cells { rest: &self.data }
    }

    /// Reads from the start of the row. To read many columns, use `cells`.
    pub fn get(&self, column: usize) -> Option<Value<'_>> {
        self.cells().nth(column)
    }

    /// Returns the number of columns.
    pub fn columns(&self) -> usize {
        let mut cells = self.cells();
        std::iter::from_fn(|| cells.next_cell()).count()
    }

    /// Returns the size of the block of the row.
    pub fn bytes(&self) -> usize {
        self.data.len()
    }

    /// Returns the text of a value. `NULL` and binary data that is not text have no text.
    pub fn text(&self, column: usize) -> Option<String> {
        match self.get(column)? {
            Value::Null => None,
            Value::Text(text) | Value::Number(text) => Some(text.to_owned()),
            Value::Binary(bytes) => bytes.text().map(str::to_owned),
        }
    }
}

impl fmt::Debug for Row {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.cells()).finish()
    }
}

impl<'a> IntoIterator for &'a Row {
    type Item = Value<'a>;
    type IntoIter = Cells<'a>;

    fn into_iter(self) -> Cells<'a> {
        self.cells()
    }
}

impl<'a> FromIterator<Value<'a>> for Row {
    fn from_iter<T: IntoIterator<Item = Value<'a>>>(values: T) -> Self {
        let mut writer = RowWriter::default();
        values.into_iter().for_each(|value| writer.push(value));
        writer.finish()
    }
}

/// Reads the values of a row, from its first column.
#[derive(Clone, Debug)]
pub struct Cells<'a> {
    rest: &'a [u8],
}

impl<'a> Cells<'a> {
    /// Returns the kind and the bytes of the next cell, and it reads no text.
    #[inline]
    fn next_cell(&mut self) -> Option<(u8, &'a [u8])> {
        let (&head, mut rest) = self.rest.split_first()?;
        let mut length = usize::from(head >> KIND_BITS);
        if length == LONG {
            let mut shift = 0;
            loop {
                let (&byte, after) = rest.split_first()?;
                rest = after;
                length += usize::from(byte & GROUP_MASK) << shift;
                if byte < MORE {
                    break;
                }
                shift += GROUP_BITS;
            }
        }
        let (value, rest) = rest.split_at_checked(length)?;
        self.rest = rest;
        Some((head & KIND_MASK, value))
    }
}

/// A check of the text here costs a third of the time of a copy or of an export.
#[inline]
fn text_of(bytes: &[u8]) -> &str {
    // SAFETY: `RowWriter::push` writes the bytes of a `str` under each kind that calls
    // this function, `Cells` reads whole cells, and nothing else changes a row.
    unsafe { std::str::from_utf8_unchecked(bytes) }
}

impl<'a> Iterator for Cells<'a> {
    type Item = Value<'a>;

    #[inline]
    fn next(&mut self) -> Option<Value<'a>> {
        let (kind, bytes) = self.next_cell()?;
        Some(match kind {
            TEXT => Value::Text(text_of(bytes)),
            NUMBER => Value::Number(text_of(bytes)),
            BINARY_TEXT => Value::Binary(Binary::of_text(text_of(bytes))),
            BINARY_HEX => Value::Binary(Binary::of_bytes(bytes)),
            _ => Value::Null,
        })
    }

    /// A skipped cell needs no check of its text.
    fn nth(&mut self, count: usize) -> Option<Value<'a>> {
        for _ in 0..count {
            self.next_cell()?;
        }
        self.next()
    }
}

/// Builds rows. The same writer serves many rows, so each one costs a single block.
#[derive(Debug, Default)]
pub struct RowWriter {
    data: Vec<u8>,
}

impl RowWriter {
    pub fn push(&mut self, value: Value<'_>) {
        let (kind, bytes) = match value {
            Value::Null => (NULL, &[][..]),
            Value::Text(text) => (TEXT, text.as_bytes()),
            Value::Number(text) => (NUMBER, text.as_bytes()),
            Value::Binary(binary) => match binary.text() {
                Some(text) => (BINARY_TEXT, text.as_bytes()),
                None => (BINARY_HEX, binary.bytes()),
            },
        };
        self.data
            .push((bytes.len().min(LONG) as u8) << KIND_BITS | kind);
        if let Some(mut rest) = bytes.len().checked_sub(LONG) {
            while rest > usize::from(GROUP_MASK) {
                self.data.push(rest as u8 | MORE);
                rest >>= GROUP_BITS;
            }
            self.data.push(rest as u8);
        }
        self.data.extend_from_slice(bytes);
    }

    /// Ends the row. The writer is then ready for the next one.
    pub fn finish(&mut self) -> Row {
        // A large row keeps its block, so its values are not copied a second time.
        if self.data.len() > REUSED_BYTES {
            return Row {
                data: std::mem::take(&mut self.data).into_boxed_slice(),
            };
        }
        let data = Box::from(self.data.as_slice());
        self.data.clear();
        Row { data }
    }
}
