//! The file browser of a path field. The field is the focus of the open dialog.

use super::App;
use crate::browser::{BrowserRequest, FileBrowser};
use crate::form::Field;

impl App {
    /// Only the export path, the import log, and the SQLite file can name a file that
    /// does not exist.
    pub(super) fn open_browser(&mut self) {
        let (path, new_file) = if let Some(form) = &self.export_form {
            (form.path_text(), true)
        } else if let Some(form) = &self.import_form {
            form.browser_start()
        } else if let Some(form) = &self.form {
            (form.text(form.focus), form.focus == Field::File)
        } else {
            return;
        };
        let home = std::env::home_dir();
        self.browser = Some(FileBrowser::new(path, new_file, home));
    }

    pub(super) fn on_browser_key(&mut self, key: crossterm::event::KeyEvent) {
        if let Some(browser) = &mut self.browser {
            let request = browser.on_key(key);
            self.on_browser_request(request);
        }
    }

    pub(super) fn on_browser_request(&mut self, request: BrowserRequest) {
        let (path, chose_file) = match request {
            BrowserRequest::None => return,
            BrowserRequest::Cancel => {
                self.browser = None;
                return;
            }
            BrowserRequest::File(path) => (path, true),
            BrowserRequest::Folder(path) => (path, false),
        };
        self.browser = None;
        if let Some(form) = &mut self.export_form {
            form.set_path(&path, chose_file);
        } else if let Some(form) = &mut self.import_form {
            form.set_path(&path);
            self.request_probe();
        } else if let Some(form) = &mut self.form {
            form.set_focused_path(&path);
        }
    }
}
