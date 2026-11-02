//! Turns the rows of a file into batches, and writes the log of the rows that fail.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::statement::{MAX_STATEMENT_BYTES, Target};
use super::{BadRow, FileReader, ImportJob, ImportMode, ImportProgress, Item, file_size};
use crate::ui::text;

/// A row that waits for the database.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub line: u64,
    pub source: String,
    pub values: String,
}

/// What comes after the rows of a batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LotEnd {
    More,
    /// The file has no more rows.
    Done,
    /// The import ends here, with this reason.
    Stop(String),
}

/// One batch of rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lot {
    pub rows: Vec<Pending>,
    pub end: LotEnd,
}

/// The log file. It starts at the first error, so a clean import leaves no file.
struct ErrorLog {
    path: PathBuf,
    out: Option<BufWriter<File>>,
}

impl ErrorLog {
    /// A later run in the same minute adds to the file, so no report gets lost.
    fn open(path: &Path) -> io::Result<BufWriter<File>> {
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(BufWriter::new(file))
    }

    fn write(&mut self, row: &BadRow) -> io::Result<()> {
        if self.out.is_none() {
            self.out = Some(Self::open(&self.path)?);
        }
        let Some(out) = &mut self.out else {
            return Ok(());
        };
        writeln!(out, "Line {}: {}", row.line, row.reason)?;
        writeln!(out, "{}", row.source)?;
        writeln!(out)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.as_mut().map_or(Ok(()), Write::flush)
    }
}

/// Gives the batches of one import.
pub struct Feed {
    reader: FileReader,
    target: Target,
    head: String,
    mode: ImportMode,
    batch: usize,
    error_limit: usize,
    log: ErrorLog,
    progress: Arc<ImportProgress>,
    /// The row that did not fit in the last batch.
    carry: Option<Pending>,
}

impl Feed {
    pub fn open(job: ImportJob) -> Result<Self, String> {
        let mut read = job.read;
        read.keep_source = job.mode == ImportMode::LogErrors;
        let reader =
            FileReader::open(&job.path, &read, &job.columns).map_err(|error| error.to_string())?;
        job.progress.set_size(file_size(&job.path));
        Ok(Self {
            reader,
            head: job.target.head(),
            target: job.target,
            mode: job.mode,
            batch: job.batch.max(1),
            error_limit: job.error_limit.max(1),
            log: ErrorLog {
                path: job.log,
                out: None,
            },
            progress: job.progress,
            carry: None,
        })
    }

    pub fn add_rows(&self, rows: usize) {
        self.progress.add_rows(rows);
    }

    /// The rows of a transaction without a commit are not in the table.
    pub fn clear_rows(&self) {
        self.progress.clear_rows();
    }

    /// Returns true when the errors reached the limit of the log mode.
    pub fn at_limit(&self, more: usize) -> bool {
        self.progress.errors() + more >= self.error_limit
    }

    pub fn limit_reason(&self) -> String {
        text::import_error_limit(self.error_limit)
    }

    /// Writes the rows to the log. It returns the reason of a failed write.
    pub fn log_rows(&mut self, rows: &[BadRow]) -> Result<(), String> {
        for row in rows {
            self.log
                .write(row)
                .map_err(|error| text::import_log_failed(&error.to_string()))?;
        }
        self.progress.add_errors(rows.len());
        Ok(())
    }

    /// Sends the waiting text of the log to the file.
    pub fn close(&mut self) -> Result<(), String> {
        self.log
            .flush()
            .map_err(|error| text::import_log_failed(&error.to_string()))
    }

    /// Writes one `INSERT` for these rows.
    pub fn statement(&self, rows: &[Pending]) -> String {
        let size = rows.iter().map(|row| row.values.len() + 2).sum::<usize>();
        let mut sql = String::with_capacity(self.head.len() + size);
        sql.push_str(&self.head);
        for (index, row) in rows.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            sql.push_str(&row.values);
        }
        sql
    }

    /// Handles a row that the import cannot use. It returns the reason to stop.
    fn on_bad_row(&mut self, row: BadRow) -> Option<String> {
        if self.mode != ImportMode::LogErrors {
            return Some(text::import_bad_row(row.line, &row.reason));
        }
        if let Err(reason) = self.log_rows(std::slice::from_ref(&row)) {
            return Some(reason);
        }
        self.at_limit(0).then(|| self.limit_reason())
    }

    fn next_end(&mut self, rows: &mut Vec<Pending>, bytes: &mut usize) -> Option<LotEnd> {
        let item = match self.reader.next_item() {
            Ok(Some(item)) => item,
            Ok(None) => return Some(LotEnd::Done),
            Err(reason) => return Some(LotEnd::Stop(reason)),
        };
        let row = match item {
            Item::Bad(row) => Err(row),
            Item::Row(row) => match self.target.values(&row.cells) {
                Ok(values) => Ok(Pending {
                    line: row.line,
                    source: row.source,
                    values,
                }),
                Err(reason) => Err(BadRow {
                    line: row.line,
                    source: row.source,
                    reason,
                }),
            },
        };
        match row {
            Ok(row) if !rows.is_empty() && *bytes + row.values.len() > MAX_STATEMENT_BYTES => {
                self.carry = Some(row);
                Some(LotEnd::More)
            }
            Ok(row) => {
                *bytes += row.values.len();
                rows.push(row);
                None
            }
            Err(row) => self.on_bad_row(row).map(LotEnd::Stop),
        }
    }

    /// Reads the next batch. A row above the size limit goes alone.
    pub fn next_lot(&mut self) -> Lot {
        let mut rows = Vec::with_capacity(self.batch.min(1024));
        let mut bytes = self.head.len();
        if let Some(row) = self.carry.take() {
            bytes += row.values.len();
            rows.push(row);
        }
        let end = loop {
            if rows.len() >= self.batch {
                break LotEnd::More;
            }
            if let Some(end) = self.next_end(&mut rows, &mut bytes) {
                break end;
            }
        };
        self.progress
            .set_read(self.reader.bytes_read(), self.reader.ignored_keys());
        Lot { rows, end }
    }
}
