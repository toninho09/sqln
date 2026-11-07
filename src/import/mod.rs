//! Reads a file for a table. `FileReader` gives the rows of a file one at a time, and
//! `Feed` groups them into the batches that the runner sends to the database.

mod delimited;
pub mod feed;
pub mod form;
mod json;
pub mod statement;

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

use self::delimited::DelimitedReader;
use self::json::JsonReader;
use self::statement::Target;
use crate::export::{Delimiter, NullText};
use crate::sql::Dialect;

const BOM: &[u8] = "\u{feff}".as_bytes();
const READ_BUFFER: usize = 256 * 1024;
/// The rows that the preview shows.
pub const PREVIEW_ROWS: usize = 50;
/// The bytes that the dialog reads from a file. One large record must not fill the memory.
const PROBE_BYTES: u64 = 16 * 1024 * 1024;
/// The objects that give the columns of a JSON file.
pub const KEY_OBJECTS: usize = 1000;
pub const DEFAULT_BATCH: usize = 100;
pub const DEFAULT_ERROR_LIMIT: usize = 1000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportFormat {
    #[default]
    Csv,
    Tsv,
    Json,
    JsonLines,
}

impl ImportFormat {
    pub const ALL: [Self; 4] = [Self::Csv, Self::Tsv, Self::Json, Self::JsonLines];

    pub fn is_delimited(self) -> bool {
        matches!(self, Self::Csv | Self::Tsv)
    }

    /// Returns the format that the name of a file suggests.
    fn of_path(path: &Path) -> Self {
        let extension = path.extension().and_then(|name| name.to_str());
        match extension.map(str::to_lowercase).as_deref() {
            Some("tsv") => Self::Tsv,
            Some("json") => Self::Json,
            Some("jsonl" | "ndjson") => Self::JsonLines,
            _ => Self::Csv,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Encoding {
    #[default]
    Utf8,
    Windows1252,
}

impl Encoding {
    pub const ALL: [Self; 2] = [Self::Utf8, Self::Windows1252];
}

/// What the import does with a row that fails.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImportMode {
    #[default]
    AllOrNothing,
    ByBatch,
    LogErrors,
}

impl ImportMode {
    pub const ALL: [Self; 3] = [Self::AllOrNothing, Self::ByBatch, Self::LogErrors];
}

/// What the dialog remembers. `config.json` keeps these values in `preferences.import`.
/// The failure mode stays out, because two modes leave partial data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default, rename_all = "camelCase")]
pub struct ImportChoices {
    pub encoding: Encoding,
    pub header: bool,
    pub delimiter: Delimiter,
    pub null_text: NullText,
    pub batch: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
}

impl Default for ImportChoices {
    fn default() -> Self {
        Self {
            encoding: Encoding::default(),
            header: true,
            delimiter: Delimiter::default(),
            null_text: NullText::default(),
            batch: DEFAULT_BATCH,
            folder: None,
        }
    }
}

/// How one file is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadOptions {
    pub format: ImportFormat,
    pub encoding: Encoding,
    pub header: bool,
    pub delimiter: Delimiter,
    pub null_text: NullText,
    pub dialect: Dialect,
    /// True keeps the text of each row, for the log.
    pub keep_source: bool,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            format: ImportFormat::default(),
            encoding: Encoding::default(),
            header: true,
            delimiter: Delimiter::default(),
            null_text: NullText::default(),
            dialect: Dialect::Sqlite,
            keep_source: false,
        }
    }
}

/// One row of a file. `None` is `NULL`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRow {
    /// The line of the file where the row starts.
    pub line: u64,
    /// The text of the row in the file, when `keep_source` asks for it.
    pub source: String,
    pub cells: Vec<Option<String>>,
}

/// A row that the import cannot use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadRow {
    pub line: u64,
    pub source: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Row(FileRow),
    Bad(BadRow),
}

/// Builds the item of a read row: a row when the cells fit, or the reason why the
/// reader cannot use them.
fn row_item(line: u64, source: String, cells: Result<Vec<Option<String>>, String>) -> Item {
    match cells {
        Ok(cells) => Item::Row(FileRow {
            line,
            source,
            cells,
        }),
        Err(reason) => Item::Bad(BadRow {
            line,
            source,
            reason,
        }),
    }
}

enum Source {
    Delimited(DelimitedReader),
    Json(JsonReader),
}

/// Gives the rows of a file. Memory use does not change with the size of the file.
pub struct FileReader {
    source: Source,
}

pub type Input = Box<dyn BufRead + Send>;

/// Removes a BOM. A file in Windows-1252 starts with these bytes only when it holds UTF-8.
fn skip_bom(input: &mut Input) -> io::Result<u64> {
    if input.fill_buf()?.starts_with(BOM) {
        input.consume(BOM.len());
        return Ok(BOM.len() as u64);
    }
    Ok(0)
}

fn input_of(path: &Path, limit: Option<u64>) -> io::Result<Input> {
    let file = File::open(path)?;
    Ok(match limit {
        Some(limit) => Box::new(BufReader::with_capacity(READ_BUFFER, file.take(limit))),
        None => Box::new(BufReader::with_capacity(READ_BUFFER, file)),
    })
}

impl FileReader {
    /// `columns` holds the columns of the source, so a row has the same number of cells.
    pub fn new(mut input: Input, options: &ReadOptions, columns: &[String]) -> io::Result<Self> {
        let read = skip_bom(&mut input)?;
        let source = if options.format.is_delimited() {
            Source::Delimited(DelimitedReader::new(input, options, read, Some(columns)))
        } else {
            Source::Json(JsonReader::new(input, options, read, columns))
        };
        Ok(Self { source })
    }

    pub fn open(path: &Path, options: &ReadOptions, columns: &[String]) -> io::Result<Self> {
        Self::new(input_of(path, None)?, options, columns)
    }

    /// Returns the next row. An `Err` holds a fault that ends the read of the file.
    pub fn next_item(&mut self) -> Result<Option<Item>, String> {
        match &mut self.source {
            Source::Delimited(reader) => reader.next_item(),
            Source::Json(reader) => reader.next_item(),
        }
    }

    /// Returns the bytes that the reader used.
    pub fn bytes_read(&self) -> u64 {
        match &self.source {
            Source::Delimited(reader) => reader.bytes_read(),
            Source::Json(reader) => reader.bytes_read(),
        }
    }

    /// Returns the objects with a key that is not a column.
    pub fn ignored_keys(&self) -> u64 {
        match &self.source {
            Source::Delimited(_) => 0,
            Source::Json(reader) => reader.ignored_keys(),
        }
    }
}

/// Reads the columns of a source. A file without a header gets `column_1`, then `column_2`.
pub fn read_columns(mut input: Input, options: &ReadOptions) -> Result<Vec<String>, String> {
    let read = skip_bom(&mut input).map_err(|error| error.to_string())?;
    if options.format.is_delimited() {
        DelimitedReader::new(input, options, read, None).columns()
    } else {
        json::read_keys(input, options, read)
    }
}

/// The counts of an import. The runner writes them, and the interface reads them.
#[derive(Debug, Default)]
pub struct ImportProgress {
    rows: AtomicUsize,
    errors: AtomicUsize,
    read: AtomicU64,
    ignored: AtomicU64,
    size: AtomicU64,
}

impl ImportProgress {
    pub fn rows(&self) -> usize {
        self.rows.load(Ordering::Relaxed)
    }

    pub fn errors(&self) -> usize {
        self.errors.load(Ordering::Relaxed)
    }

    pub fn ignored_keys(&self) -> u64 {
        self.ignored.load(Ordering::Relaxed)
    }

    /// Returns the part of the file that the import read, from 0 to 100.
    pub fn percent(&self) -> u64 {
        let size = self.size.load(Ordering::Relaxed);
        let read = self.read.load(Ordering::Relaxed);
        if size == 0 {
            return 100;
        }
        read.min(size).saturating_mul(100) / size
    }

    pub(crate) fn add_rows(&self, rows: usize) {
        self.rows.fetch_add(rows, Ordering::Relaxed);
    }

    /// The rows of a transaction that ended without a commit are not in the table.
    pub(crate) fn clear_rows(&self) {
        self.rows.store(0, Ordering::Relaxed);
    }

    pub(crate) fn add_errors(&self, errors: usize) {
        self.errors.fetch_add(errors, Ordering::Relaxed);
    }

    pub(crate) fn set_read(&self, read: u64, ignored: u64) {
        self.read.store(read, Ordering::Relaxed);
        self.ignored.store(ignored, Ordering::Relaxed);
    }

    pub(crate) fn set_size(&self, size: u64) {
        self.size.store(size, Ordering::Relaxed);
    }
}

/// All that the runner needs for one import.
#[derive(Debug)]
pub struct ImportJob {
    pub path: PathBuf,
    pub read: ReadOptions,
    /// The columns of the file.
    pub columns: Vec<String>,
    pub target: Target,
    pub mode: ImportMode,
    /// The rows of one `INSERT`.
    pub batch: usize,
    pub error_limit: usize,
    pub log: PathBuf,
    pub progress: Arc<ImportProgress>,
}

/// Returns the path of the log: `<source>-errors-<stamp>.log`, beside the source.
pub fn log_path(source: &Path, stamp: &str) -> PathBuf {
    let name = source
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    source.with_file_name(format!("{name}-errors-{stamp}.log"))
}

/// Returns the delimiter of a line. A delimiter inside quotes does not count.
fn count_delimiters(line: &[u8]) -> [usize; 4] {
    let mut counts = [0; 4];
    let mut quoted = false;
    for byte in line {
        match byte {
            b'"' => quoted = !quoted,
            b',' if !quoted => counts[0] += 1,
            b';' if !quoted => counts[1] += 1,
            b'|' if !quoted => counts[2] += 1,
            b'\t' if !quoted => counts[3] += 1,
            _ => {}
        }
    }
    counts
}

/// Returns the format and the delimiter that a file suggests. `options` keeps what the
/// name and the first line do not decide.
pub fn detect(path: &Path, first_line: &[u8], options: ReadOptions) -> ReadOptions {
    let format = ImportFormat::of_path(path);
    let mut options = ReadOptions { format, ..options };
    if !format.is_delimited() {
        return options;
    }
    let counts = count_delimiters(first_line);
    let most = counts.iter().copied().max().unwrap_or(0);
    if most == 0 {
        return options;
    }
    if counts[3] == most {
        options.format = ImportFormat::Tsv;
    } else if format == ImportFormat::Csv {
        let index = counts.iter().position(|count| *count == most);
        options.delimiter = Delimiter::ALL[index.unwrap_or(0)];
    }
    options
}

/// Reads the first line of a file, for `detect`.
fn first_line(path: &Path) -> io::Result<Vec<u8>> {
    let mut input = input_of(path, Some(PROBE_BYTES))?;
    skip_bom(&mut input)?;
    let mut line = Vec::new();
    input.read_until(b'\n', &mut line)?;
    Ok(line)
}

/// What the dialog learns from a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    pub options: ReadOptions,
    pub columns: Vec<String>,
    pub rows: Vec<Item>,
    /// The fault that stopped the read.
    pub failure: Option<String>,
}

/// Reads the start of a file. `detect_format` lets the file choose its format.
/// A read that reaches the limit of bytes keeps the rows before the limit.
pub fn probe(path: &Path, options: ReadOptions, detect_format: bool) -> Probe {
    let mut probe = Probe {
        options,
        columns: Vec::new(),
        rows: Vec::new(),
        failure: None,
    };
    let opened = (|| {
        if detect_format {
            probe.options = detect(path, &first_line(path)?, options);
        }
        input_of(path, Some(PROBE_BYTES))
    })();
    let columns = opened
        .map_err(|error| error.to_string())
        .and_then(|input| read_columns(input, &probe.options));
    probe.columns = match columns {
        Ok(columns) => columns,
        Err(reason) => {
            probe.failure = Some(reason);
            return probe;
        }
    };
    let reader = input_of(path, Some(PROBE_BYTES))
        .and_then(|input| FileReader::new(input, &probe.options, &probe.columns));
    let mut reader = match reader {
        Ok(reader) => reader,
        Err(error) => {
            probe.failure = Some(error.to_string());
            return probe;
        }
    };
    while probe.rows.len() < PREVIEW_ROWS {
        match reader.next_item() {
            Ok(Some(item)) => probe.rows.push(item),
            Ok(None) => break,
            Err(reason) => {
                if reader.bytes_read() < PROBE_BYTES {
                    probe.failure = Some(reason);
                }
                break;
            }
        }
    }
    probe
}

/// Returns the size of a file, for the progress.
pub(crate) fn file_size(path: &Path) -> u64 {
    fs::metadata(path).map_or(0, |data| data.len())
}
