//! The import dialog. It holds the choices of one file and writes nothing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::statement::{Target, TargetColumn, is_binary_type};
use super::{
    DEFAULT_ERROR_LIMIT, Encoding, ImportChoices, ImportFormat, ImportJob, ImportMode,
    ImportProgress, Item, Probe, ReadOptions, log_path,
};
use crate::export::{Delimiter, NullText, resolve_path};
use crate::line_input::LineInput;
use crate::settings::{next_choice, next_optional};
use crate::sql::Dialect;
use crate::ui::text;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    File,
    Format,
    Columns,
    Options,
    Preview,
}

impl Section {
    pub const ALL: [Self; 5] = [
        Self::File,
        Self::Format,
        Self::Columns,
        Self::Options,
        Self::Preview,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// The sidebar. It is one place of the focus.
    Sections,
    File,
    Format,
    Encoding,
    Header,
    Delimiter,
    NullText,
    /// The list of the columns of the table.
    Columns,
    Mode,
    Batch,
    Log,
    ErrorLimit,
    /// The rows of the preview.
    Preview,
    Import,
    Cancel,
}

impl Field {
    /// Returns true for a field that holds a path, with the file browser.
    pub fn is_path(self) -> bool {
        matches!(self, Self::File | Self::Log)
    }
}

pub enum ImportAction {
    None,
    Cancel,
    Import,
    /// Opens the file browser for the path.
    Browse,
}

/// One column of the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableColumn {
    pub name: String,
    pub data_type: String,
}

/// The table of the dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportTable {
    /// The name that the title shows.
    pub name: String,
    /// The name of the table, as a statement writes it.
    pub table: String,
    pub dialect: Dialect,
    pub columns: Vec<TableColumn>,
}

/// A read of the start of a file. `serial` tells the answer of the last request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeRequest {
    pub serial: u64,
    pub path: PathBuf,
    pub options: ReadOptions,
    /// True lets the file choose its format.
    pub detect: bool,
}

/// Returns the full path of the text of a field.
fn full_path(text: &str) -> Option<PathBuf> {
    if text.trim().is_empty() {
        return None;
    }
    resolve_path(text, std::env::home_dir()).ok()
}

pub struct ImportForm {
    pub focus: Field,
    pub section: Section,
    pub format: ImportFormat,
    pub encoding: Encoding,
    pub header: bool,
    pub delimiter: Delimiter,
    pub null_text: NullText,
    pub mode: ImportMode,
    pub note: Option<String>,
    /// The column of the file for each column of the table.
    pub mapping: Vec<Option<usize>>,
    /// The selected row of the column list.
    pub column: usize,
    /// The first row and the first column that the preview shows.
    pub preview: (usize, usize),
    file: LineInput,
    batch: LineInput,
    log: LineInput,
    limit: LineInput,
    table: ImportTable,
    probe: Option<Probe>,
    /// The path and the options of the last request.
    requested: (Option<PathBuf>, ReadOptions),
    /// The number of the request in progress. It stays `None` between requests.
    reading: Option<u64>,
    serial: u64,
    /// True asks for a read while the focus stays on the path.
    read_now: bool,
    /// False while the log follows the name of the file.
    log_edited: bool,
    /// The time in the name of the log.
    stamp: String,
}

impl ImportForm {
    /// The focus starts on the path, because the dialog needs a file first.
    pub fn new(table: ImportTable, choices: &ImportChoices, stamp: &str) -> Self {
        let mut file = LineInput::default();
        if let Some(folder) = &choices.folder {
            file.set(&format!("{}/", folder.display()));
        }
        let mut batch = LineInput::default();
        batch.set(&choices.batch.to_string());
        let mut limit = LineInput::default();
        limit.set(&DEFAULT_ERROR_LIMIT.to_string());
        let options = ReadOptions {
            encoding: choices.encoding,
            header: choices.header,
            delimiter: choices.delimiter,
            null_text: choices.null_text,
            dialect: table.dialect,
            ..ReadOptions::default()
        };
        let mapping = vec![None; table.columns.len()];
        Self {
            focus: Field::File,
            section: Section::File,
            format: options.format,
            encoding: options.encoding,
            header: options.header,
            delimiter: options.delimiter,
            null_text: options.null_text,
            mode: ImportMode::default(),
            note: None,
            mapping,
            column: 0,
            preview: (0, 0),
            requested: (full_path(file.text()), options),
            file,
            batch,
            log: LineInput::default(),
            limit,
            table,
            probe: None,
            reading: None,
            serial: 0,
            read_now: false,
            log_edited: false,
            stamp: stamp.to_owned(),
        }
    }

    pub fn table(&self) -> &ImportTable {
        &self.table
    }

    pub fn probe(&self) -> Option<&Probe> {
        self.probe.as_ref()
    }

    pub fn is_reading(&self) -> bool {
        self.reading.is_some()
    }

    /// Returns the columns of the file.
    pub fn source_columns(&self) -> &[String] {
        self.probe.as_ref().map_or(&[], |probe| &probe.columns)
    }

    /// Returns the rows of the preview.
    pub fn rows(&self) -> &[Item] {
        self.probe.as_ref().map_or(&[], |probe| &probe.rows)
    }

    /// Returns the fields of the open section, in the order of the window.
    pub fn fields(&self) -> Vec<Field> {
        match self.section {
            Section::File => vec![Field::File, Field::Format],
            Section::Format => match self.format {
                ImportFormat::Csv => vec![
                    Field::Encoding,
                    Field::Header,
                    Field::Delimiter,
                    Field::NullText,
                ],
                ImportFormat::Tsv => vec![Field::Encoding, Field::Header, Field::NullText],
                ImportFormat::Json | ImportFormat::JsonLines => Vec::new(),
            },
            Section::Columns => vec![Field::Columns],
            Section::Options if self.mode == ImportMode::LogErrors => {
                vec![Field::Mode, Field::Batch, Field::Log, Field::ErrorLimit]
            }
            Section::Options => vec![Field::Mode, Field::Batch],
            Section::Preview => vec![Field::Preview],
        }
    }

    /// Returns a field of text, with the cursor position.
    pub fn input(&self, field: Field) -> Option<&LineInput> {
        match field {
            Field::File => Some(&self.file),
            Field::Batch => Some(&self.batch),
            Field::Log => Some(&self.log),
            Field::ErrorLimit => Some(&self.limit),
            _ => None,
        }
    }

    fn input_mut(&mut self) -> Option<&mut LineInput> {
        match self.focus {
            Field::File => Some(&mut self.file),
            Field::Batch => Some(&mut self.batch),
            Field::Log => Some(&mut self.log),
            Field::ErrorLimit => Some(&mut self.limit),
            _ => None,
        }
    }

    fn stops(&self) -> Vec<Field> {
        let mut stops = vec![Field::Sections];
        stops.extend(self.fields());
        stops.extend([Field::Import, Field::Cancel]);
        stops
    }

    fn move_focus(&mut self, forward: bool) {
        self.focus = next_choice(&self.stops(), self.focus, forward);
    }

    /// Returns the columns of the table that receive values, in the order of the table.
    pub fn mapped(&self) -> Vec<(&TableColumn, usize)> {
        let pairs = self.table.columns.iter().zip(&self.mapping);
        pairs
            .filter_map(|(column, source)| source.map(|source| (column, source)))
            .collect()
    }

    fn step_list(&mut self, forward: bool) {
        let last = self.table.columns.len().saturating_sub(1);
        self.column = if forward {
            (self.column + 1).min(last)
        } else {
            self.column.saturating_sub(1)
        };
    }

    fn scroll_rows(&mut self, forward: bool) {
        let last = self.rows().len().saturating_sub(1);
        self.preview.0 = if forward {
            (self.preview.0 + 1).min(last)
        } else {
            self.preview.0.saturating_sub(1)
        };
    }

    fn scroll_columns(&mut self, forward: bool) {
        let last = self.mapped().len().saturating_sub(1);
        self.preview.1 = if forward {
            (self.preview.1 + 1).min(last)
        } else {
            self.preview.1.saturating_sub(1)
        };
    }

    /// `Up` and `Down` open another section from the sidebar, and move the focus elsewhere.
    fn step(&mut self, forward: bool) {
        match self.focus {
            Field::Sections => self.section = next_choice(&Section::ALL, self.section, forward),
            Field::Columns => self.step_list(forward),
            Field::Preview => self.scroll_rows(forward),
            _ => self.move_focus(forward),
        }
    }

    fn change_source(&mut self, forward: bool) {
        let sources: Vec<usize> = (0..self.source_columns().len()).collect();
        if let Some(source) = self.mapping.get_mut(self.column) {
            *source = next_optional(&sources, *source, forward);
        }
    }

    /// The choices change with the side keys.
    fn change(&mut self, forward: bool) {
        match self.focus {
            Field::Format => {
                self.format = next_choice(&ImportFormat::ALL, self.format, forward);
            }
            Field::Encoding => self.encoding = next_choice(&Encoding::ALL, self.encoding, forward),
            Field::Header => self.header = !self.header,
            Field::Delimiter => {
                self.delimiter = next_choice(&Delimiter::ALL, self.delimiter, forward);
            }
            Field::NullText => {
                self.null_text = match self.null_text {
                    NullText::Empty => NullText::Word,
                    NullText::Word => NullText::Empty,
                };
            }
            Field::Mode => self.mode = next_choice(&ImportMode::ALL, self.mode, forward),
            Field::Columns => self.change_source(forward),
            Field::Preview => self.scroll_columns(forward),
            _ => {}
        }
    }

    fn is_number_field(&self) -> bool {
        matches!(self.focus, Field::Batch | Field::ErrorLimit)
    }

    pub fn paste(&mut self, value: &str) {
        let line = value.lines().next().unwrap_or("");
        let digits: String = line.chars().filter(char::is_ascii_digit).collect();
        let text = if self.is_number_field() {
            digits.as_str()
        } else {
            line
        };
        self.log_edited |= self.focus == Field::Log;
        if let Some(input) = self.input_mut() {
            input.insert(text);
        }
    }

    /// Puts the focus on one field. The mouse uses this.
    pub fn focus_on(&mut self, field: Field) {
        self.focus = field;
    }

    /// Opens one section and puts the focus on the sidebar. The mouse uses this.
    pub fn open_section(&mut self, index: usize) {
        if let Some(section) = Section::ALL.get(index) {
            self.section = *section;
        }
        self.focus = Field::Sections;
    }

    /// Selects one row of the column list. The mouse uses this.
    pub fn select_column(&mut self, index: usize) {
        self.focus = Field::Columns;
        self.column = index.min(self.table.columns.len().saturating_sub(1));
    }

    /// Acts on a click: a choice changes, a button runs.
    pub fn press(&mut self) -> ImportAction {
        match self.focus {
            Field::Import => ImportAction::Import,
            Field::Cancel => ImportAction::Cancel,
            field if self.input(field).is_none() => {
                self.change(true);
                ImportAction::None
            }
            _ => ImportAction::None,
        }
    }

    fn edit(&mut self, key: KeyEvent) {
        if self.is_number_field() && matches!(key.code, KeyCode::Char(c) if !c.is_ascii_digit()) {
            return;
        }
        let before = self.log.text().to_owned();
        let edits_log = self.focus == Field::Log;
        if let Some(input) = self.input_mut() {
            input.on_key(key);
        }
        self.log_edited |= edits_log && before != self.log.text();
    }

    pub fn on_key(&mut self, key: KeyEvent, vim: bool) -> ImportAction {
        self.note = None;
        let text = self.input(self.focus).is_some();
        match key.code {
            KeyCode::Esc => return ImportAction::Cancel,
            KeyCode::Enter if self.focus == Field::Import => return ImportAction::Import,
            KeyCode::Enter if self.focus == Field::Cancel => return ImportAction::Cancel,
            KeyCode::Char('o')
                if key.modifiers == KeyModifiers::CONTROL && self.focus.is_path() =>
            {
                return ImportAction::Browse;
            }
            KeyCode::Enter | KeyCode::Tab => self.move_focus(true),
            KeyCode::BackTab => self.move_focus(false),
            KeyCode::Down => self.step(true),
            KeyCode::Up => self.step(false),
            KeyCode::Char('j') if vim && !text => self.step(true),
            KeyCode::Char('k') if vim && !text => self.step(false),
            KeyCode::Left if !text => self.change(false),
            KeyCode::Right if !text => self.change(true),
            KeyCode::Char('h') if vim && !text => self.change(false),
            KeyCode::Char('l') if vim && !text => self.change(true),
            _ => self.edit(key),
        }
        ImportAction::None
    }

    /// Returns the start of the file browser: the text of the focused path, and true
    /// when the path can name a new file. Only the log can do that.
    pub fn browser_start(&self) -> (&str, bool) {
        if self.focus == Field::Log {
            (self.log.text(), true)
        } else {
            (self.file.text(), false)
        }
    }

    /// Writes the choice of the file browser in the focused path.
    pub fn set_path(&mut self, path: &str) {
        if self.focus == Field::Log {
            self.log.set(path);
            self.log_edited = true;
        } else {
            self.file.set(path);
            self.read_now = true;
        }
    }

    /// Returns the text of the path, as the user sees it.
    pub fn path_text(&self) -> &str {
        self.file.text()
    }

    fn read_options(&self) -> ReadOptions {
        ReadOptions {
            format: self.format,
            encoding: self.encoding,
            header: self.header,
            delimiter: self.delimiter,
            null_text: self.null_text,
            dialect: self.table.dialect,
            keep_source: false,
        }
    }

    /// Returns the read that the fields ask for. The path waits for the end of its edit,
    /// so a read does not start at each key.
    pub fn probe_request(&mut self) -> Option<ProbeRequest> {
        if self.focus == Field::File && !std::mem::take(&mut self.read_now) {
            return None;
        }
        let path = full_path(self.file.text());
        let options = self.read_options();
        if (path.as_ref(), options) == (self.requested.0.as_ref(), self.requested.1) {
            return None;
        }
        let detect = path != self.requested.0;
        self.requested = (path.clone(), options);
        if detect && !self.log_edited {
            let shown = path.as_deref().map(|path| log_path(path, &self.stamp));
            self.log
                .set(&shown.unwrap_or_default().display().to_string());
        }
        let Some(path) = path else {
            self.probe = None;
            self.reading = None;
            return None;
        };
        self.serial += 1;
        self.reading = Some(self.serial);
        Some(ProbeRequest {
            serial: self.serial,
            path,
            options,
            detect,
        })
    }

    /// Finds the mapping of new columns: by name, or by position without names.
    fn map_columns(&self, columns: &[String]) -> Vec<Option<usize>> {
        let named = !self.format.is_delimited() || self.header;
        let by_name = |name: &str| {
            let lower = name.to_lowercase();
            columns
                .iter()
                .position(|column| column == name)
                .or_else(|| columns.iter().position(|c| c.to_lowercase() == lower))
        };
        let table = self.table.columns.iter().enumerate();
        table
            .map(|(index, column)| {
                if named {
                    by_name(&column.name)
                } else {
                    (index < columns.len()).then_some(index)
                }
            })
            .collect()
    }

    /// Takes the answer of a read. New columns restart the mapping, and an old answer
    /// leaves the form as it is.
    pub fn on_probe(&mut self, serial: u64, probe: Probe) {
        if self.reading != Some(serial) {
            return;
        }
        self.reading = None;
        self.format = probe.options.format;
        self.delimiter = probe.options.delimiter;
        self.requested.1 = self.read_options();
        if self.source_columns() != probe.columns {
            self.mapping = self.map_columns(&probe.columns);
        }
        self.preview = (0, 0);
        self.probe = Some(probe);
    }

    fn number(input: &LineInput, name: &str) -> Result<usize, String> {
        match input.text().trim().parse::<usize>() {
            Ok(value) if value > 0 => Ok(value),
            _ => Err(text::settings_number(name)),
        }
    }

    /// Returns the path of the source, or the text of the problem.
    pub fn source_path(&self) -> Result<PathBuf, String> {
        full_path(self.file.text()).ok_or_else(|| text::IMPORT_NO_FILE.to_owned())
    }

    /// Returns the work of the dialog, or the text of the first problem.
    pub fn job(&self) -> Result<ImportJob, String> {
        let path = self.source_path()?;
        if self.is_reading() {
            return Err(text::IMPORT_READING.to_owned());
        }
        if self.source_columns().is_empty() {
            let failure = self.probe.as_ref().and_then(|probe| probe.failure.clone());
            return Err(failure.unwrap_or_else(|| text::IMPORT_NO_COLUMNS.to_owned()));
        }
        let columns: Vec<TargetColumn> = self
            .mapped()
            .into_iter()
            .map(|(column, source)| TargetColumn {
                name: column.name.clone(),
                binary: is_binary_type(&column.data_type),
                source,
            })
            .collect();
        if columns.is_empty() {
            return Err(text::IMPORT_NO_MAPPING.to_owned());
        }
        let batch = Self::number(&self.batch, text::FIELD_IMPORT_BATCH)?;
        let error_limit = Self::number(&self.limit, text::FIELD_IMPORT_LIMIT)?;
        let log = full_path(self.log.text())
            .filter(|_| self.mode == ImportMode::LogErrors)
            .unwrap_or_else(|| log_path(&path, &self.stamp));
        Ok(ImportJob {
            read: self.read_options(),
            columns: self.source_columns().to_vec(),
            target: Target {
                table: self.table.table.clone(),
                columns,
                dialect: self.table.dialect,
                base64: !self.format.is_delimited(),
            },
            mode: self.mode,
            batch,
            error_limit,
            log,
            path,
            progress: Arc::new(ImportProgress::default()),
        })
    }

    /// Returns what the next dialog of this kind starts with.
    pub fn choices(&self, source: &Path) -> ImportChoices {
        let batch = Self::number(&self.batch, text::FIELD_IMPORT_BATCH);
        ImportChoices {
            encoding: self.encoding,
            header: self.header,
            delimiter: self.delimiter,
            null_text: self.null_text,
            batch: batch.unwrap_or(ImportChoices::default().batch),
            folder: source.parent().map(Path::to_path_buf),
        }
    }
}
