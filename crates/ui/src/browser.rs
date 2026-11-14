//! The file browser: the rows of one folder, with a filter. It helps to fill a path field.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::export::resolve_path;
use crate::find;
use crate::line_input::{Edit, LineInput};
use crate::ui::text;

/// The rows of one step of `Page Up` and `Page Down`.
const PAGE_ROWS: isize = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    name: OsString,
    /// The name as text. A file system name can hold bytes that are not UTF-8.
    label: String,
    folder: bool,
}

/// One row of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    /// Chooses the open folder.
    UseFolder,
    Parent,
    /// The position in the entries.
    Entry(usize),
}

/// What a row shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowView<'a> {
    UseFolder,
    Parent,
    Folder(&'a str),
    File(&'a str),
}

/// What a key or a click asks from the application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrowserRequest {
    None,
    Cancel,
    /// The path of the file that the user chose.
    File(String),
    /// The path of the open folder, with the name of the file of the field.
    Folder(String),
}

#[derive(Clone, Debug)]
pub struct FileBrowser {
    folder: PathBuf,
    home: Option<PathBuf>,
    /// The name of the file of the field. **Use this folder** keeps it.
    name: Option<OsString>,
    /// True when the field can name a file that does not exist.
    new_file: bool,
    entries: Vec<Entry>,
    filter: LineInput,
    rows: Vec<Row>,
    cursor: usize,
    note: Option<String>,
}

/// Returns the first folder that exists on the path of a field, and the name of the file.
fn start_of(path_text: &str, home: Option<PathBuf>) -> (PathBuf, Option<OsString>) {
    let path_text = path_text.trim();
    let text = if path_text.is_empty() { "." } else { path_text };
    let root = || PathBuf::from(std::path::MAIN_SEPARATOR_STR);
    let path = resolve_path(text, home).unwrap_or_else(|_| root());
    let folder = path.ancestors().find(|place| place.is_dir());
    // A path can hold `..`, and a parent of the text would not be the folder above.
    let folder = folder.map_or_else(root, |place| {
        fs::canonicalize(place).unwrap_or_else(|_| place.to_owned())
    });
    let name = path
        .file_name()
        .filter(|_| !path.is_dir() && !text.ends_with(std::path::is_separator))
        .map(OsStr::to_owned);
    (folder, name)
}

/// Reads a folder. The folders come first, and the order ignores case. The type of a
/// link needs one more read, so only a link pays for it.
fn read_entries(folder: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for item in fs::read_dir(folder)?.flatten() {
        let is_folder = item
            .file_type()
            .is_ok_and(|kind| kind.is_dir() || (kind.is_symlink() && item.path().is_dir()));
        let name = item.file_name();
        entries.push(Entry {
            label: name.to_string_lossy().into_owned(),
            name,
            folder: is_folder,
        });
    }
    entries.sort_by_cached_key(|entry| {
        (
            !entry.folder,
            entry.label.to_lowercase(),
            entry.label.clone(),
        )
    });
    Ok(entries)
}

/// Returns the text of a path. A field holds trimmed text in one line, so it would
/// change some paths, and then the field would hold another path.
fn field_text(path: &Path) -> Option<String> {
    path.to_str()
        .filter(|text| text.trim() == *text && !text.chars().any(char::is_control))
        .map(str::to_owned)
}

impl FileBrowser {
    /// Opens the folder of the path in a field. `new_file` offers **Use this folder**.
    pub fn new(path_text: &str, new_file: bool, home: Option<PathBuf>) -> Self {
        let (folder, name) = start_of(path_text, home.clone());
        let mut browser = Self {
            folder: folder.clone(),
            home,
            new_file,
            entries: Vec::new(),
            filter: LineInput::default(),
            rows: Vec::new(),
            cursor: 0,
            note: None,
            name,
        };
        browser.refilter();
        let selected = browser.name.clone();
        browser.open_folder(folder, selected.as_deref());
        browser
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn filter(&self) -> &LineInput {
        &self.filter
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Lists the rows that the filter keeps.
    pub fn rows(&self) -> impl ExactSizeIterator<Item = RowView<'_>> {
        self.rows.iter().map(|row| match row {
            Row::UseFolder => RowView::UseFolder,
            Row::Parent => RowView::Parent,
            Row::Entry(index) if self.entries[*index].folder => {
                RowView::Folder(&self.entries[*index].label)
            }
            Row::Entry(index) => RowView::File(&self.entries[*index].label),
        })
    }

    /// A filter with text hides the fixed rows, so `Enter` acts on the first result. A
    /// filter that starts with a dot also shows the hidden names.
    fn refilter(&mut self) {
        let text = self.filter.text();
        let hidden = text.starts_with('.');
        let words: Vec<Vec<char>> = text.split_whitespace().map(find::needle).collect();
        self.rows.clear();
        if text.is_empty() {
            if self.new_file {
                self.rows.push(Row::UseFolder);
            }
            if self.folder.parent().is_some() {
                self.rows.push(Row::Parent);
            }
        }
        let keeps = |entry: &Entry| {
            (hidden || !entry.label.starts_with('.'))
                && words.iter().all(|word| find::contains(&entry.label, word))
        };
        let shown = self.entries.iter().enumerate();
        let shown = shown.filter(|(_, entry)| keeps(entry));
        self.rows.extend(shown.map(|(index, _)| Row::Entry(index)));
        self.cursor = 0;
    }

    /// Shows a folder, with the cursor on `selected`. A folder that the system refuses
    /// leaves the list as it is, with a note.
    fn open_folder(&mut self, folder: PathBuf, selected: Option<&OsStr>) {
        let entries = match read_entries(&folder) {
            Ok(entries) => entries,
            Err(error) => {
                self.note = Some(text::browser_unreadable(&error.to_string()));
                return;
            }
        };
        self.folder = folder;
        self.entries = entries;
        self.filter = LineInput::default();
        self.refilter();
        let position = self.rows.iter().position(|row| match row {
            Row::Entry(index) => Some(self.entries[*index].name.as_os_str()) == selected,
            _ => false,
        });
        self.cursor = position.unwrap_or(0);
    }

    /// Goes to the folder above, with the cursor on the folder that the user left.
    fn go_up(&mut self) {
        let left = self.folder.file_name().map(OsStr::to_owned);
        if let Some(parent) = self.folder.parent().map(Path::to_owned) {
            self.open_folder(parent, left.as_deref());
        }
    }

    fn go_home(&mut self) {
        if let Some(home) = self.home.clone() {
            self.open_folder(home, None);
        }
    }

    fn accept(&mut self, path: &Path, request: fn(String) -> BrowserRequest) -> BrowserRequest {
        match field_text(path) {
            Some(text) => request(text),
            None => {
                self.note = Some(text::BROWSER_BAD_PATH.to_owned());
                BrowserRequest::None
            }
        }
    }

    /// Acts on a row: a folder opens, and a file or **Use this folder** ends the choice.
    pub fn open(&mut self, index: usize) -> BrowserRequest {
        match self.rows.get(index).copied() {
            None => BrowserRequest::None,
            Some(Row::Parent) => {
                self.go_up();
                BrowserRequest::None
            }
            Some(Row::UseFolder) => {
                let name = self.name.as_deref().unwrap_or(OsStr::new(""));
                self.accept(&self.folder.join(name), BrowserRequest::Folder)
            }
            Some(Row::Entry(entry)) => {
                let path = self.folder.join(&self.entries[entry].name);
                if self.entries[entry].folder {
                    self.open_folder(path, None);
                    BrowserRequest::None
                } else {
                    self.accept(&path, BrowserRequest::File)
                }
            }
        }
    }

    /// Puts the cursor on a row. The mouse uses this.
    pub fn point_at(&mut self, index: usize) {
        if index < self.rows.len() {
            self.cursor = index;
        }
    }

    pub fn move_by(&mut self, rows: isize) {
        let last = self.rows.len().saturating_sub(1);
        self.cursor = self.cursor.saturating_add_signed(rows).min(last);
    }

    pub fn paste(&mut self, text: &str) {
        self.note = None;
        self.filter.insert(text.lines().next().unwrap_or(""));
        self.refilter();
    }

    fn on_folder_row(&self) -> bool {
        match self.rows.get(self.cursor) {
            Some(Row::Parent) => true,
            Some(Row::Entry(index)) => self.entries[*index].folder,
            _ => false,
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> BrowserRequest {
        self.note = None;
        let empty = self.filter.text().is_empty();
        let plain = (key.modifiers - KeyModifiers::SHIFT).is_empty();
        match key.code {
            KeyCode::Esc => return BrowserRequest::Cancel,
            KeyCode::Enter => return self.open(self.cursor),
            KeyCode::Right if empty && self.on_folder_row() => return self.open(self.cursor),
            KeyCode::Up => self.move_by(-1),
            KeyCode::Down => self.move_by(1),
            KeyCode::PageUp => self.move_by(-PAGE_ROWS),
            KeyCode::PageDown => self.move_by(PAGE_ROWS),
            KeyCode::Left | KeyCode::Backspace if empty => self.go_up(),
            KeyCode::Char('~') if empty && plain => self.go_home(),
            _ => {
                if !matches!(self.filter.on_key(key), Edit::None | Edit::Moved) {
                    self.refilter();
                }
            }
        }
        BrowserRequest::None
    }
}
