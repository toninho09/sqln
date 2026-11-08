//! Reads JSON and JSON Lines. A byte scanner finds each value of an array, so the file
//! does not enter the memory, and `RawValue` keeps the order of the keys and each digit.

use std::collections::{HashMap, HashSet};
use std::fmt;

use serde::Deserialize;
use serde::de::{Deserializer, MapAccess, Visitor};
use serde_json::value::RawValue;

use super::{ImportFormat, Input, Item, KEY_OBJECTS, ReadOptions};
use crate::sql::Dialect;
use crate::ui::text;

/// The pairs of one object, in the order of the file.
struct Pairs<'a>(Vec<(String, &'a RawValue)>);

struct PairsVisitor;

impl<'de> Visitor<'de> for PairsVisitor {
    type Value = Pairs<'de>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut pairs = Vec::with_capacity(map.size_hint().unwrap_or(0));
        while let Some(key) = map.next_key::<String>()? {
            pairs.push((key, map.next_value::<&'de RawValue>()?));
        }
        Ok(Pairs(pairs))
    }
}

impl<'de> Deserialize<'de> for Pairs<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(PairsVisitor)
    }
}

/// The text of `true` and `false` in a database.
fn booleans(dialect: Dialect) -> (&'static str, &'static str) {
    if dialect == Dialect::Postgres {
        ("true", "false")
    } else {
        ("1", "0")
    }
}

/// Reads an object from valid text.
fn pairs(object: &str) -> Result<Pairs<'_>, String> {
    if !object.trim_start().starts_with('{') {
        return Err(text::IMPORT_NOT_OBJECT.to_owned());
    }
    serde_json::from_str(object).map_err(|error| error.to_string())
}

/// Writes one value as the text of a cell. A number keeps its digits, and an object or
/// an array stays as JSON text.
fn cell(value: &RawValue, truth: (&str, &str)) -> Result<Option<String>, String> {
    let raw = value.get();
    Ok(match raw.as_bytes().first() {
        Some(b'"') => Some(serde_json::from_str(raw).map_err(|error| error.to_string())?),
        Some(b'n') => None,
        Some(b't') => Some(truth.0.to_owned()),
        Some(b'f') => Some(truth.1.to_owned()),
        _ => Some(raw.to_owned()),
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// Before the `[` of the file.
    Start,
    /// After the `[`, before the first value.
    First,
    /// After a value.
    Next,
    /// After the `]`.
    End,
}

/// The bytes between the values of a file.
struct Scanner {
    input: Input,
    line: u64,
    read: u64,
    place: Place,
}

impl Scanner {
    /// Returns the first byte after the spaces, or `None` at the end of the file.
    fn skip_spaces(&mut self) -> Result<Option<u8>, String> {
        loop {
            let buffer = self.input.fill_buf().map_err(|error| error.to_string())?;
            if buffer.is_empty() {
                return Ok(None);
            }
            let spaces = buffer
                .iter()
                .take_while(|byte| byte.is_ascii_whitespace())
                .count();
            let found = buffer.get(spaces).copied();
            let lines = buffer[..spaces].iter().filter(|byte| **byte == b'\n');
            self.line += lines.count() as u64;
            self.read += spaces as u64;
            self.input.consume(spaces);
            if found.is_some() {
                return Ok(found);
            }
        }
    }

    fn consume_byte(&mut self) {
        self.input.consume(1);
        self.read += 1;
    }

    /// Moves to the next value of the array. It returns false after the last value.
    fn enter_value(&mut self) -> Result<bool, String> {
        let byte = self.skip_spaces()?;
        match (self.place, byte) {
            (Place::End, None) => Ok(false),
            (Place::End, Some(_)) => Err(text::import_json_after(self.line + 1)),
            (Place::Start, Some(b'[')) => {
                self.consume_byte();
                self.place = Place::First;
                self.enter_value()
            }
            (Place::Start, None) => {
                self.place = Place::End;
                Ok(false)
            }
            (Place::Start, Some(_)) => Err(text::IMPORT_NOT_ARRAY.to_owned()),
            (_, None) => Err(text::IMPORT_JSON_OPEN.to_owned()),
            (Place::First | Place::Next, Some(b']')) => {
                self.consume_byte();
                self.place = Place::End;
                self.enter_value()
            }
            (Place::First, Some(_)) => {
                self.place = Place::Next;
                Ok(true)
            }
            (Place::Next, Some(b',')) => {
                self.consume_byte();
                match self.skip_spaces()? {
                    Some(b']') | None => Err(text::import_json_syntax(self.line + 1)),
                    Some(_) => Ok(true),
                }
            }
            (Place::Next, Some(_)) => Err(text::import_json_syntax(self.line + 1)),
        }
    }

    /// Copies one value of the array to `text`. Only the brackets and the strings
    /// matter here, so a value with a wrong inner syntax stays a problem of one row.
    fn copy_value(&mut self, text: &mut Vec<u8>) -> Result<(), String> {
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        let mut started = false;
        loop {
            let buffer = self.input.fill_buf().map_err(|error| error.to_string())?;
            if buffer.is_empty() {
                if started && depth == 0 && !in_string {
                    return Ok(());
                }
                return Err(text::IMPORT_JSON_OPEN.to_owned());
            }
            let mut used = 0;
            let mut done = false;
            for &byte in buffer {
                if in_string {
                    match byte {
                        _ if escaped => escaped = false,
                        b'\\' => escaped = true,
                        b'"' => {
                            in_string = false;
                            done = depth == 0;
                        }
                        _ => {}
                    }
                } else {
                    match byte {
                        b'"' => in_string = true,
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' if depth > 0 => {
                            depth -= 1;
                            done = depth == 0;
                        }
                        // The end of a number or a word at the top of the array.
                        b',' | b']' | b' ' | b'\t' | b'\r' | b'\n' if depth == 0 => {
                            done = true;
                            break;
                        }
                        _ => {}
                    }
                }
                self.line += u64::from(byte == b'\n');
                used += 1;
                started = true;
                if done {
                    break;
                }
            }
            text.extend_from_slice(&buffer[..used]);
            self.input.consume(used);
            self.read += used as u64;
            if done {
                return Ok(());
            }
        }
    }

    /// Reads the next line with text. It returns the number of that line.
    fn next_line(&mut self, text: &mut Vec<u8>) -> Result<Option<u64>, String> {
        loop {
            text.clear();
            let count = self
                .input
                .read_until(b'\n', text)
                .map_err(|error| error.to_string())?;
            if count == 0 {
                return Ok(None);
            }
            self.read += count as u64;
            self.line += 1;
            if !text.iter().all(u8::is_ascii_whitespace) {
                return Ok(Some(self.line));
            }
        }
    }

    /// Reads the next value of the file. It returns the line of its start.
    fn next_value(&mut self, lines: bool, text: &mut Vec<u8>) -> Result<Option<u64>, String> {
        if lines {
            return self.next_line(text);
        }
        text.clear();
        if !self.enter_value()? {
            return Ok(None);
        }
        let line = self.line + 1;
        self.copy_value(text)?;
        Ok(Some(line))
    }
}

pub(super) struct JsonReader {
    scanner: Scanner,
    lines: bool,
    columns: HashMap<String, usize>,
    width: usize,
    truth: (&'static str, &'static str),
    keep_source: bool,
    ignored: u64,
    text: Vec<u8>,
}

impl JsonReader {
    pub(super) fn new(input: Input, options: &ReadOptions, read: u64, columns: &[String]) -> Self {
        // A repeated name keeps its first place.
        let mut places = HashMap::with_capacity(columns.len());
        for (index, name) in columns.iter().enumerate() {
            places.entry(name.clone()).or_insert(index);
        }
        Self {
            scanner: Scanner {
                input,
                line: 0,
                read,
                place: Place::Start,
            },
            lines: options.format == ImportFormat::JsonLines,
            columns: places,
            width: columns.len(),
            truth: booleans(options.dialect),
            keep_source: options.keep_source,
            ignored: 0,
            text: Vec::new(),
        }
    }

    pub(super) fn bytes_read(&self) -> u64 {
        self.scanner.read
    }

    pub(super) fn ignored_keys(&self) -> u64 {
        self.ignored
    }

    /// Writes the values of an object in the order of the columns.
    fn cells(&mut self) -> Result<Vec<Option<String>>, String> {
        let text = std::str::from_utf8(&self.text).map_err(|_| text::IMPORT_NOT_UTF8.to_owned())?;
        let pairs = pairs(text)?;
        let mut cells = vec![None; self.width];
        let mut ignored = false;
        for (key, value) in &pairs.0 {
            match self.columns.get(key) {
                Some(index) => cells[*index] = cell(value, self.truth)?,
                None => ignored = true,
            }
        }
        self.ignored += u64::from(ignored);
        Ok(cells)
    }

    fn source(&self) -> String {
        if self.keep_source {
            String::from_utf8_lossy(&self.text).trim().to_owned()
        } else {
            String::new()
        }
    }

    pub(super) fn next_item(&mut self) -> Result<Option<Item>, String> {
        let mut text = std::mem::take(&mut self.text);
        let line = self.scanner.next_value(self.lines, &mut text);
        self.text = text;
        let Some(line) = line? else {
            return Ok(None);
        };
        let cells = self.cells();
        let source = self.source();
        Ok(Some(super::row_item(line, source, cells)))
    }
}

/// Returns the keys of the first objects, in order of appearance. A value that is not
/// an object adds no key, and the rows report it later.
pub(super) fn read_keys(
    input: Input,
    options: &ReadOptions,
    read: u64,
) -> Result<Vec<String>, String> {
    let mut reader = JsonReader::new(input, options, read, &[]);
    let mut keys = Vec::new();
    let mut seen = HashSet::new();
    let mut text = Vec::new();
    for _ in 0..KEY_OBJECTS {
        match reader.scanner.next_value(reader.lines, &mut text) {
            Ok(Some(_)) => {}
            Ok(None) => break,
            // The rows before the fault still give keys, and the preview reports it.
            Err(_) if !keys.is_empty() => break,
            Err(reason) => return Err(reason),
        }
        let Ok(object) = std::str::from_utf8(&text) else {
            continue;
        };
        let Ok(Pairs(pairs)) = pairs(object) else {
            continue;
        };
        for (key, _) in pairs {
            if seen.insert(key.clone()) {
                keys.push(key);
            }
        }
    }
    Ok(keys)
}
