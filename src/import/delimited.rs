//! Reads CSV and TSV. The rules match the writer of the export: a value with a separator
//! has quotes, a quote inside a value doubles, and quotes mark text that is not `NULL`.

use std::borrow::Cow;

use super::{Encoding, ImportFormat, Input, Item, ReadOptions};
use crate::ui::text;

/// The characters of the bytes 0x80 to 0x9F. The other bytes match their code points.
/// An unused byte keeps its code point, as the WHATWG table does.
const WINDOWS_1252: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}',
    '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
];

fn windows_1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| match byte {
            0x80..=0x9f => WINDOWS_1252[usize::from(byte - 0x80)],
            _ => char::from(*byte),
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    /// The start of a field.
    Start,
    /// Inside a field without quotes.
    Plain,
    /// Inside quotes.
    Quoted,
    /// After a quote inside quotes: the end of the quotes, or the first of two quotes.
    AfterQuote,
}

#[derive(Clone, Copy)]
struct Field {
    end: usize,
    quoted: bool,
}

/// The fields of one record, in one buffer.
#[derive(Default)]
struct Fields {
    text: Vec<u8>,
    ends: Vec<Field>,
    quoted: bool,
}

impl Fields {
    fn clear(&mut self) {
        self.text.clear();
        self.ends.clear();
        self.quoted = false;
    }

    fn close(&mut self) {
        self.ends.push(Field {
            end: self.text.len(),
            quoted: self.quoted,
        });
        self.quoted = false;
    }

    fn iter(&self) -> impl Iterator<Item = (&[u8], bool)> {
        let mut start = 0;
        self.ends.iter().map(move |field| {
            let bytes = &self.text[start..field.end];
            start = field.end;
            (bytes, field.quoted)
        })
    }

    /// Reads the bytes of one line. A quote opens only at the start of a field, so a
    /// quote inside plain text stays in the value.
    fn scan(&mut self, line: &[u8], delimiter: u8, mut state: State) -> State {
        for &byte in line {
            state = match state {
                State::Start if byte == b'"' => {
                    self.quoted = true;
                    State::Quoted
                }
                State::Quoted if byte == b'"' => State::AfterQuote,
                State::Quoted => {
                    self.text.push(byte);
                    State::Quoted
                }
                State::AfterQuote if byte == b'"' => {
                    self.text.push(byte);
                    State::Quoted
                }
                State::Start | State::Plain | State::AfterQuote if byte == delimiter => {
                    self.close();
                    State::Start
                }
                State::Start | State::Plain | State::AfterQuote => {
                    self.text.push(byte);
                    State::Plain
                }
            };
        }
        state
    }
}

pub(super) struct DelimitedReader {
    input: Input,
    delimiter: u8,
    encoding: Encoding,
    null: &'static str,
    keep_source: bool,
    skip_header: bool,
    /// The number of fields of a row. It is `None` while the columns are not known.
    width: Option<usize>,
    /// The lines that the reader used.
    line: u64,
    read: u64,
    raw: Vec<u8>,
    fields: Fields,
}

/// Returns the line without its end.
fn body(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

impl DelimitedReader {
    pub(super) fn new(
        input: Input,
        options: &ReadOptions,
        read: u64,
        columns: Option<&[String]>,
    ) -> Self {
        let delimiter = if options.format == ImportFormat::Tsv {
            b'\t'
        } else {
            options.delimiter.symbol()
        };
        Self {
            input,
            delimiter,
            encoding: options.encoding,
            null: options.null_text.text(),
            keep_source: options.keep_source,
            skip_header: options.header,
            width: columns.map(<[String]>::len),
            line: 0,
            read,
            raw: Vec::new(),
            fields: Fields::default(),
        }
    }

    pub(super) fn bytes_read(&self) -> u64 {
        self.read
    }

    /// Reads one record, which can hold several lines. It returns the line of its start.
    /// `keep_blank` makes a blank line a record.
    fn next_record(&mut self, keep_blank: bool) -> Result<Option<u64>, String> {
        loop {
            self.raw.clear();
            self.fields.clear();
            let first = self.line + 1;
            let mut state = State::Start;
            let mut empty = true;
            loop {
                let start = self.raw.len();
                let count = self
                    .input
                    .read_until(b'\n', &mut self.raw)
                    .map_err(|error| error.to_string())?;
                self.read += count as u64;
                if count == 0 {
                    if empty {
                        return Ok(None);
                    }
                    if state == State::Quoted {
                        return Err(text::import_open_quote(first));
                    }
                    break;
                }
                empty = false;
                self.line += 1;
                let line = &self.raw[start..];
                let content = body(line);
                state = self.fields.scan(content, self.delimiter, state);
                if state != State::Quoted {
                    break;
                }
                // A quoted value keeps its line end.
                self.fields.text.extend_from_slice(&line[content.len()..]);
            }
            self.fields.close();
            let blank = self.fields.text.is_empty()
                && self.fields.ends.len() == 1
                && !self.fields.ends[0].quoted;
            if !blank || keep_blank {
                return Ok(Some(first));
            }
        }
    }

    fn decode<'a>(&self, bytes: &'a [u8]) -> Result<Cow<'a, str>, String> {
        match self.encoding {
            Encoding::Utf8 => std::str::from_utf8(bytes)
                .map(Cow::Borrowed)
                .map_err(|_| text::IMPORT_NOT_UTF8.to_owned()),
            Encoding::Windows1252 => Ok(Cow::Owned(windows_1252(bytes))),
        }
    }

    fn source(&self) -> String {
        if self.keep_source {
            String::from_utf8_lossy(body(&self.raw)).into_owned()
        } else {
            String::new()
        }
    }

    fn cells(&self) -> Result<Vec<Option<String>>, String> {
        let mut cells = Vec::with_capacity(self.fields.ends.len());
        for (bytes, quoted) in self.fields.iter() {
            let value = self.decode(bytes)?;
            let null = !quoted && value == self.null;
            cells.push((!null).then(|| value.into_owned()));
        }
        Ok(cells)
    }

    pub(super) fn next_item(&mut self) -> Result<Option<Item>, String> {
        if std::mem::take(&mut self.skip_header) && self.next_record(false)?.is_none() {
            return Ok(None);
        }
        // A file of one column writes `NULL` as a blank line.
        let Some(line) = self.next_record(self.width == Some(1))? else {
            return Ok(None);
        };
        let count = self.fields.ends.len();
        let cells = match self.width {
            Some(width) if width != count => Err(text::import_field_count(count, width)),
            _ => self.cells(),
        };
        let source = self.source();
        Ok(Some(super::row_item(line, source, cells)))
    }

    /// Returns the columns from the first record. Without a header, only their number
    /// comes from the file.
    pub(super) fn columns(mut self) -> Result<Vec<String>, String> {
        if self.next_record(false)?.is_none() {
            return Ok(Vec::new());
        }
        let numbered = |index: usize| text::import_column(index + 1);
        if !self.skip_header {
            return Ok((0..self.fields.ends.len()).map(numbered).collect());
        }
        let names = self.fields.iter().enumerate().map(|(index, (bytes, _))| {
            let name = self.decode(bytes)?;
            Ok::<_, String>(if name.is_empty() {
                numbered(index)
            } else {
                name.into_owned()
            })
        });
        names.collect()
    }
}
