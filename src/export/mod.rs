//! Writes rows to a file. `Encoder` holds the formats, and `ExportWriter` owns the
//! temporary file that the rows go to.

pub mod form;

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};

use crate::clipboard::base64;
use crate::grid::{Value, delimited_cell, delimited_text, hex};
use crate::sql::{Dialect, quote_identifier, quote_literal};
use crate::store::file;

const DEFAULT_BATCH: usize = 100;
const BOM: &[u8] = "\u{feff}".as_bytes();

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    #[default]
    Csv,
    Tsv,
    Json,
    JsonLines,
    SqlInsert,
}

impl ExportFormat {
    pub const ALL: [Self; 5] = [
        Self::Csv,
        Self::Tsv,
        Self::Json,
        Self::JsonLines,
        Self::SqlInsert,
    ];

    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Tsv => "tsv",
            Self::Json => "json",
            Self::JsonLines => "jsonl",
            Self::SqlInsert => "sql",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Delimiter {
    #[default]
    Comma,
    Semicolon,
    Pipe,
}

impl Delimiter {
    pub const ALL: [Self; 3] = [Self::Comma, Self::Semicolon, Self::Pipe];

    pub fn symbol(self) -> char {
        match self {
            Self::Comma => ',',
            Self::Semicolon => ';',
            Self::Pipe => '|',
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NullText {
    #[default]
    Empty,
    Word,
}

impl NullText {
    pub fn text(self) -> &'static str {
        match self {
            Self::Empty => "",
            Self::Word => "NULL",
        }
    }
}

/// What the modal remembers. `config.json` keeps these values in `preferences.export`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default, rename_all = "camelCase")]
pub struct ExportChoices {
    pub format: ExportFormat,
    pub header: bool,
    pub delimiter: Delimiter,
    pub null_text: NullText,
    pub bom: bool,
    pub grouped: bool,
    pub batch: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
}

impl Default for ExportChoices {
    fn default() -> Self {
        Self {
            format: ExportFormat::default(),
            header: true,
            delimiter: Delimiter::default(),
            null_text: NullText::default(),
            bom: false,
            grouped: false,
            batch: DEFAULT_BATCH,
            folder: None,
        }
    }
}

/// How one file is written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportOptions {
    pub format: ExportFormat,
    pub header: bool,
    pub delimiter: char,
    pub null: &'static str,
    pub bom: bool,
    /// The rows of one `INSERT`.
    pub batch: usize,
    /// The target of `INSERT`, as the statement writes it.
    pub table: String,
    pub dialect: Dialect,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: ExportFormat::default(),
            header: true,
            delimiter: Delimiter::default().symbol(),
            null: NullText::default().text(),
            bom: false,
            batch: 1,
            table: String::new(),
            dialect: Dialect::Sqlite,
        }
    }
}

fn json_string(text: &str) -> String {
    serde_json::to_string(text).expect("a string always has a JSON form")
}

/// Returns true for text that JSON and SQL can both read as a number.
fn is_number(text: &str) -> bool {
    serde_json::from_str::<serde_json::Number>(text).is_ok()
}

fn json_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Number(text) if is_number(text) => text.to_string(),
        Value::Text(text) | Value::Number(text) => json_string(text),
        Value::Binary(bytes) => json_string(&base64(bytes)),
    }
}

fn sql_value(value: &Value, dialect: Dialect) -> String {
    match value {
        Value::Null => "NULL".to_owned(),
        Value::Number(text) if is_number(text) => text.to_string(),
        Value::Text(text) | Value::Number(text) => quote_literal(text, dialect),
        Value::Binary(bytes) => {
            let digits = hex(bytes);
            let digits = digits.trim_start_matches("0x");
            if dialect == Dialect::Postgres {
                format!("'\\x{digits}'")
            } else {
                format!("X'{digits}'")
            }
        }
    }
}

/// Gives each column a name of its own: `id`, then `id_2`.
fn unique_names(columns: &[String]) -> Vec<String> {
    let mut used = HashSet::new();
    columns
        .iter()
        .map(|name| {
            let mut unique = name.clone();
            let mut number = 1;
            while !used.insert(unique.clone()) {
                number += 1;
                unique = format!("{name}_{number}");
            }
            unique
        })
        .collect()
}

/// Writes one result in one format. The rows go out as they arrive, so the size of a
/// file does not change the memory that the encoder uses.
pub struct Encoder<W: Write> {
    out: W,
    options: ExportOptions,
    /// The JSON keys, already in JSON form.
    keys: Vec<String>,
    /// The start of each `INSERT`.
    insert: String,
    started: bool,
    /// The rows in the JSON array, or in the open `INSERT`.
    open_rows: usize,
}

impl<W: Write> Encoder<W> {
    pub fn new(out: W, options: ExportOptions) -> Self {
        Self {
            out,
            options,
            keys: Vec::new(),
            insert: String::new(),
            started: false,
            open_rows: 0,
        }
    }

    fn delimiter(&self) -> char {
        if self.options.format == ExportFormat::Tsv {
            '\t'
        } else {
            self.options.delimiter
        }
    }

    fn is_delimited(&self) -> bool {
        matches!(self.options.format, ExportFormat::Csv | ExportFormat::Tsv)
    }

    /// Writes what comes before the rows. A second result of a statement adds nothing.
    pub fn start(&mut self, columns: &[String]) -> io::Result<()> {
        if std::mem::replace(&mut self.started, true) {
            return Ok(());
        }
        if self.is_delimited() {
            if self.options.bom {
                self.out.write_all(BOM)?;
            }
            if self.options.header {
                let null = self.options.null;
                let delimiter = self.delimiter();
                let names: Vec<String> = columns
                    .iter()
                    .map(|name| delimited_text(name, delimiter, null).into_owned())
                    .collect();
                writeln!(self.out, "{}", names.join(&delimiter.to_string()))?;
            }
        }
        match self.options.format {
            ExportFormat::Json | ExportFormat::JsonLines => {
                let names = unique_names(columns);
                self.keys = names.iter().map(|name| json_string(name)).collect();
            }
            ExportFormat::SqlInsert => {
                let dialect = self.options.dialect;
                let names: Vec<String> = columns
                    .iter()
                    .map(|name| quote_identifier(name, dialect))
                    .collect();
                let names = names.join(", ");
                self.insert = format!("INSERT INTO {} ({names}) VALUES", self.options.table);
            }
            ExportFormat::Csv | ExportFormat::Tsv => {}
        }
        Ok(())
    }

    fn json_object(&self, row: &[Value]) -> String {
        let fields: Vec<String> = self
            .keys
            .iter()
            .zip(row)
            .map(|(key, value)| format!("{key}:{}", json_value(value)))
            .collect();
        format!("{{{}}}", fields.join(","))
    }

    fn write_delimited(&mut self, row: &[Value]) -> io::Result<()> {
        let delimiter = self.delimiter();
        let mut line = String::new();
        for (index, value) in row.iter().enumerate() {
            if index > 0 {
                line.push(delimiter);
            }
            line.push_str(&delimited_cell(value, delimiter, self.options.null));
        }
        line.push('\n');
        self.out.write_all(line.as_bytes())
    }

    fn write_insert(&mut self, row: &[Value]) -> io::Result<()> {
        let dialect = self.options.dialect;
        let values: Vec<String> = row.iter().map(|value| sql_value(value, dialect)).collect();
        let values = values.join(", ");
        let batch = self.options.batch.max(1);
        let start = match (self.open_rows, batch) {
            (0, 1) => format!("{} ", self.insert),
            (0, _) => format!("{}\n  ", self.insert),
            _ => ",\n  ".to_owned(),
        };
        self.open_rows = (self.open_rows + 1) % batch;
        let end = if self.open_rows == 0 { ";\n" } else { "" };
        write!(self.out, "{start}({values}){end}")
    }

    pub fn row(&mut self, row: &[Value]) -> io::Result<()> {
        match self.options.format {
            ExportFormat::Csv | ExportFormat::Tsv => self.write_delimited(row),
            ExportFormat::Json => {
                let start = if self.open_rows == 0 { "[\n" } else { ",\n" };
                self.open_rows += 1;
                write!(self.out, "{start}{}", self.json_object(row))
            }
            ExportFormat::JsonLines => writeln!(self.out, "{}", self.json_object(row)),
            ExportFormat::SqlInsert => self.write_insert(row),
        }
    }

    /// Closes the format and sends the waiting bytes to the writer.
    pub fn finish(&mut self) -> io::Result<()> {
        match self.options.format {
            ExportFormat::Json if self.open_rows == 0 => self.out.write_all(b"[]\n")?,
            ExportFormat::Json => self.out.write_all(b"\n]\n")?,
            ExportFormat::SqlInsert if self.open_rows > 0 => self.out.write_all(b";\n")?,
            _ => {}
        }
        self.out.flush()
    }
}

struct State {
    /// `None` after `seal`.
    encoder: Option<Encoder<BufWriter<File>>>,
    error: Option<String>,
}

struct Shared {
    state: Mutex<State>,
    temporary: PathBuf,
    rows: AtomicUsize,
}

/// The file of one export. The connector thread writes the rows, and the interface
/// reads the count. A failed write ends the writes and keeps the first error.
#[derive(Clone)]
pub struct ExportWriter(Arc<Shared>);

impl std::fmt::Debug for ExportWriter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("ExportWriter")
            .field(&self.0.temporary)
            .finish()
    }
}

/// `RunOptions` compares its fields, and two handles match when they share one file.
impl PartialEq for ExportWriter {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ExportWriter {}

impl ExportWriter {
    /// Creates the temporary file beside the destination, and the missing folders.
    pub fn create(destination: &Path, options: ExportOptions) -> io::Result<Self> {
        if let Some(folder) = destination.parent() {
            fs::create_dir_all(folder)?;
        }
        let temporary = file::temporary(destination);
        let out = BufWriter::new(File::create(&temporary)?);
        Ok(Self(Arc::new(Shared {
            state: Mutex::new(State {
                encoder: Some(Encoder::new(out, options)),
                error: None,
            }),
            temporary,
            rows: AtomicUsize::new(0),
        })))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Applies one step to the encoder. It returns false after a failure or a seal.
    fn apply(&self, step: impl FnOnce(&mut Encoder<BufWriter<File>>) -> io::Result<()>) -> bool {
        let mut state = self.state();
        if state.error.is_some() {
            return false;
        }
        let Some(encoder) = &mut state.encoder else {
            return false;
        };
        match step(encoder) {
            Ok(()) => true,
            Err(error) => {
                state.error = Some(error.to_string());
                false
            }
        }
    }

    pub fn start(&self, columns: &[String]) {
        self.apply(|encoder| encoder.start(columns));
    }

    /// Writes one row. A false value tells the reader to stop.
    pub fn write_row(&self, row: &[Value]) -> bool {
        let written = self.apply(|encoder| encoder.row(row));
        if written {
            self.0.rows.fetch_add(1, Ordering::Relaxed);
        }
        written
    }

    /// Closes the format and sends the file to the disk. A second call does nothing.
    pub fn seal(&self) {
        let mut state = self.state();
        let Some(mut encoder) = state.encoder.take() else {
            return;
        };
        let result = encoder
            .finish()
            .and_then(|()| encoder.out.get_ref().sync_all());
        if let Err(error) = result {
            state.error.get_or_insert_with(|| error.to_string());
        }
    }

    pub fn rows(&self) -> usize {
        self.0.rows.load(Ordering::Relaxed)
    }

    pub fn error(&self) -> Option<String> {
        self.state().error.clone()
    }

    /// Seals the file and gives it its final name.
    pub fn promote(&self, destination: &Path) -> io::Result<()> {
        self.seal();
        fs::rename(&self.0.temporary, destination)
    }

    /// Removes the temporary file without a write to the disk.
    pub fn discard(&self) {
        self.state().encoder = None;
        let _ = fs::remove_file(&self.0.temporary);
    }
}

/// Returns the full path of the text of a field. `~` is the home folder, and any other
/// relative path starts at the current folder.
pub fn resolve_path(text: &str, home: Option<PathBuf>) -> io::Result<PathBuf> {
    let text = text.trim();
    let path = match (text.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            home.join(rest.trim_start_matches('/'))
        }
        _ => PathBuf::from(text),
    };
    std::path::absolute(path)
}

/// Returns the name of a new file. A name holds no folder separator and no control
/// character, because a console or a Redis key can hold any text.
pub fn file_name(source: &str, stamp: &str, format: ExportFormat) -> String {
    let source: String = source
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    format!("{source}-{stamp}.{}", format.extension())
}

/// Returns the local time, as a file name writes it.
pub fn time_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d-%H%M").to_string()
}
