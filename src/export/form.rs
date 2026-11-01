//! The export dialog. It holds the choices of one file and writes nothing.

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{Delimiter, ExportChoices, ExportFormat, ExportOptions, NullText, file_name};
use crate::line_input::LineInput;
use crate::settings::next_choice;
use crate::sql::Dialect;
use crate::ui::text;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// The sidebar. It is one place of the focus.
    Formats,
    Rows,
    File,
    Header,
    Delimiter,
    NullText,
    Bom,
    Table,
    Grouping,
    Batch,
    Export,
    Cancel,
}

/// The rows that a file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowChoice {
    Selection,
    AllRows,
    LoadedRows,
}

/// Why the dialog offers `All rows`, or why it does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllRows {
    Available,
    /// The grid holds each row, so a new read adds nothing.
    Complete,
    /// The text explains the reason to the user.
    Refused(&'static str),
}

/// What the dialog needs to know about the result on screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportSource {
    /// The start of the name of a new file.
    pub name: String,
    /// The target of `INSERT`. A console result has no table.
    pub table: String,
    pub dialect: Dialect,
    pub loaded: usize,
    /// The rows and the columns of the selection.
    pub selection: Option<(usize, usize)>,
    pub all_rows: AllRows,
}

pub enum ExportRequest {
    None,
    Cancel,
    Export,
    /// Opens the file browser for the path.
    Browse,
}

pub struct ExportForm {
    pub focus: Field,
    pub format: ExportFormat,
    pub rows: RowChoice,
    pub header: bool,
    pub delimiter: Delimiter,
    pub null_text: NullText,
    pub bom: bool,
    pub grouped: bool,
    pub note: Option<String>,
    file: LineInput,
    table: LineInput,
    batch: LineInput,
    source: ExportSource,
    /// False while the extension of the path follows the format.
    file_edited: bool,
}

impl ExportForm {
    /// The focus starts on the sidebar. `stamp` is the time in the name of a new file.
    pub fn new(source: ExportSource, choices: &ExportChoices, stamp: &str) -> Self {
        let formats = formats_of(source.dialect);
        let format = if formats.contains(&choices.format) {
            choices.format
        } else {
            ExportFormat::default()
        };
        let name = file_name(&source.name, stamp, format);
        let path = choices
            .folder
            .as_deref()
            .unwrap_or(Path::new("."))
            .join(name);
        let mut file = LineInput::default();
        file.set(&path.display().to_string());
        let mut table = LineInput::default();
        table.set(&source.table);
        let mut batch = LineInput::default();
        batch.set(&choices.batch.to_string());
        let rows = if source.selection.is_some() {
            RowChoice::Selection
        } else if source.all_rows == AllRows::Available {
            RowChoice::AllRows
        } else {
            RowChoice::LoadedRows
        };
        Self {
            focus: Field::Formats,
            format,
            rows,
            header: choices.header,
            delimiter: choices.delimiter,
            null_text: choices.null_text,
            bom: choices.bom,
            grouped: choices.grouped,
            note: None,
            file,
            table,
            batch,
            source,
            file_edited: false,
        }
    }

    pub fn formats(&self) -> Vec<ExportFormat> {
        formats_of(self.source.dialect)
    }

    pub fn source(&self) -> &ExportSource {
        &self.source
    }

    /// Returns the reason that `All rows` is not on the list.
    pub fn all_rows_reason(&self) -> Option<&'static str> {
        match self.source.all_rows {
            AllRows::Refused(reason) => Some(reason),
            AllRows::Available | AllRows::Complete => None,
        }
    }

    fn row_choices(&self) -> Vec<RowChoice> {
        let mut choices = Vec::new();
        if self.source.selection.is_some() {
            choices.push(RowChoice::Selection);
        }
        if self.source.all_rows == AllRows::Available {
            choices.push(RowChoice::AllRows);
        }
        choices.push(RowChoice::LoadedRows);
        choices
    }

    /// Returns the fields of the open format, in the order of the window.
    pub fn fields(&self) -> Vec<Field> {
        let mut fields = vec![Field::Rows, Field::File];
        match self.format {
            ExportFormat::Csv => {
                fields.extend([Field::Header, Field::Delimiter, Field::NullText, Field::Bom]);
            }
            ExportFormat::Tsv => fields.extend([Field::Header, Field::NullText, Field::Bom]),
            ExportFormat::Json | ExportFormat::JsonLines => {}
            ExportFormat::SqlInsert => {
                fields.extend([Field::Table, Field::Grouping]);
                if self.grouped {
                    fields.push(Field::Batch);
                }
            }
        }
        fields
    }

    /// Returns a field of text, with the cursor position.
    pub fn input(&self, field: Field) -> Option<&LineInput> {
        match field {
            Field::File => Some(&self.file),
            Field::Table => Some(&self.table),
            Field::Batch => Some(&self.batch),
            _ => None,
        }
    }

    fn input_mut(&mut self) -> Option<&mut LineInput> {
        match self.focus {
            Field::File => Some(&mut self.file),
            Field::Table => Some(&mut self.table),
            Field::Batch => Some(&mut self.batch),
            _ => None,
        }
    }

    fn stops(&self) -> Vec<Field> {
        let mut stops = vec![Field::Formats];
        stops.extend(self.fields());
        stops.extend([Field::Export, Field::Cancel]);
        stops
    }

    fn move_focus(&mut self, forward: bool) {
        self.focus = next_choice(&self.stops(), self.focus, forward);
    }

    /// Gives the path the extension of the format, until the user edits the path.
    fn set_format(&mut self, format: ExportFormat) {
        self.format = format;
        if !self.file_edited {
            let path = Path::new(self.file.text()).with_extension(format.extension());
            self.file.set(&path.display().to_string());
        }
    }

    /// `Up` and `Down` open another format from the sidebar, and move the focus elsewhere.
    fn step(&mut self, forward: bool) {
        if self.focus == Field::Formats {
            self.set_format(next_choice(&self.formats(), self.format, forward));
        } else {
            self.move_focus(forward);
        }
    }

    /// The choices change with the side keys.
    fn change(&mut self, forward: bool) {
        match self.focus {
            Field::Rows => self.rows = next_choice(&self.row_choices(), self.rows, forward),
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
            Field::Bom => self.bom = !self.bom,
            Field::Grouping => self.grouped = !self.grouped,
            _ => {}
        }
    }

    pub fn paste(&mut self, value: &str) {
        let line = value.lines().next().unwrap_or("");
        let digits: String = line.chars().filter(char::is_ascii_digit).collect();
        let text = if self.focus == Field::Batch {
            digits.as_str()
        } else {
            line
        };
        if self.focus == Field::File {
            self.file_edited = true;
        }
        if let Some(input) = self.input_mut() {
            input.insert(text);
        }
    }

    /// Puts the focus on one field. The mouse uses this.
    pub fn focus_on(&mut self, field: Field) {
        self.focus = field;
    }

    /// Opens one format and puts the focus on the sidebar. The mouse uses this.
    pub fn open_format(&mut self, index: usize) {
        if let Some(format) = self.formats().get(index) {
            self.set_format(*format);
        }
        self.focus = Field::Formats;
    }

    /// Acts on a click: a choice changes, a button runs.
    pub fn press(&mut self) -> ExportRequest {
        match self.focus {
            Field::Export => ExportRequest::Export,
            Field::Cancel => ExportRequest::Cancel,
            field if self.input(field).is_none() => {
                self.change(true);
                ExportRequest::None
            }
            _ => ExportRequest::None,
        }
    }

    fn edit(&mut self, key: KeyEvent) {
        if self.focus == Field::Batch && matches!(key.code, KeyCode::Char(c) if !c.is_ascii_digit())
        {
            return;
        }
        let edits_file = self.focus == Field::File;
        let before = self.file.text().to_owned();
        if let Some(input) = self.input_mut() {
            input.on_key(key);
        }
        self.file_edited |= edits_file && before != self.file.text();
    }

    pub fn on_key(&mut self, key: KeyEvent, vim: bool) -> ExportRequest {
        self.note = None;
        let text = self.input(self.focus).is_some();
        match key.code {
            KeyCode::Esc => return ExportRequest::Cancel,
            KeyCode::Enter if self.focus == Field::Export => return ExportRequest::Export,
            KeyCode::Enter if self.focus == Field::Cancel => return ExportRequest::Cancel,
            KeyCode::Char('o')
                if key.modifiers == KeyModifiers::CONTROL && self.focus == Field::File =>
            {
                return ExportRequest::Browse;
            }
            KeyCode::Enter => self.move_focus(true),
            KeyCode::Tab => self.move_focus(true),
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
        ExportRequest::None
    }

    /// Returns the options of the file, or the text of the first wrong field.
    pub fn options(&self) -> Result<ExportOptions, String> {
        let sql = self.format == ExportFormat::SqlInsert;
        let table = self.table.text().trim();
        if sql && table.is_empty() {
            return Err(text::EXPORT_NO_TABLE.to_owned());
        }
        let batch = if sql && self.grouped {
            self.batch_size()?
        } else {
            1
        };
        Ok(ExportOptions {
            format: self.format,
            header: self.header,
            delimiter: self.delimiter.symbol(),
            null: self.null_text.text(),
            bom: self.bom,
            batch,
            table: table.to_owned(),
            dialect: self.source.dialect,
        })
    }

    fn batch_size(&self) -> Result<usize, String> {
        match self.batch.text().trim().parse::<usize>() {
            Ok(rows) if rows > 0 => Ok(rows),
            _ => Err(text::settings_number(text::FIELD_EXPORT_BATCH)),
        }
    }

    /// Writes the choice of the file browser in the path. A chosen file has its own
    /// extension, so `chose_file` stops the extension that follows the format.
    pub fn set_path(&mut self, path: &str, chose_file: bool) {
        self.file.set(path);
        self.file_edited |= chose_file;
    }

    /// Returns the text of the path, as the user sees it.
    pub fn path_text(&self) -> &str {
        self.file.text()
    }

    /// Returns what the next dialog of this kind starts with.
    pub fn choices(&self, destination: &Path) -> ExportChoices {
        ExportChoices {
            format: self.format,
            header: self.header,
            delimiter: self.delimiter,
            null_text: self.null_text,
            bom: self.bom,
            grouped: self.grouped,
            batch: self.batch_size().unwrap_or(ExportChoices::default().batch),
            folder: destination.parent().map(Path::to_path_buf),
        }
    }
}

/// Redis has no tables, so it has no `INSERT`.
fn formats_of(dialect: Dialect) -> Vec<ExportFormat> {
    ExportFormat::ALL
        .into_iter()
        .filter(|format| dialect != Dialect::Redis || *format != ExportFormat::SqlInsert)
        .collect()
}
