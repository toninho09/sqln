//! The windows over the screen, and the one order that decides which window gets the
//! keys, the mouse, the paste, and the hint, and which window is drawn on top.

use std::collections::VecDeque;

use super::{App, Dialog, Overlay, Picker};
use crate::browser::FileBrowser;
use crate::compare::form::CompareForm;
use crate::export_form::ExportForm;
use crate::form::ConnectionForm;
use crate::help::Help;
use crate::history::HistoryList;
use crate::import_form::ImportForm;
use crate::palette::Palette;
use crate::settings::SettingsForm;
use crate::ui::name::NamePrompt;

/// A window over the screen. The AI prompts have their own routing and are not here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Modal {
    Dialog,
    Password,
    Conflicts,
    Palette,
    History,
    Rename,
    Browser,
    Settings,
    Export,
    Edit,
    Import,
    Compare,
    Picker,
    Help,
    Overlay,
    /// The connection form. It does not block the background work.
    Form,
}

impl Modal {
    /// The windows from the top to the bottom. The first open window gets the input.
    pub(super) const ALL: [Modal; 16] = [
        Modal::Dialog,
        Modal::Password,
        Modal::Conflicts,
        Modal::Palette,
        Modal::History,
        Modal::Rename,
        Modal::Browser,
        Modal::Settings,
        Modal::Export,
        Modal::Edit,
        Modal::Import,
        Modal::Compare,
        Modal::Picker,
        Modal::Help,
        Modal::Overlay,
        Modal::Form,
    ];
}

/// The state of the windows over the screen. Several windows can be open at once;
/// `Modal::ALL` decides which one is on top.
#[derive(Default)]
pub(super) struct Modals {
    /// The first dialog is open. Another console can add a question at any time.
    pub(super) dialogs: VecDeque<Dialog>,
    pub(super) conflicts: Option<super::conflicts::Conflicts>,
    pub(super) palette: Option<Palette>,
    pub(super) history: Option<HistoryList>,
    /// The request for a new name of a console.
    pub(super) rename: Option<NamePrompt>,
    /// The file browser of the focused path field in a dialog or in the form.
    pub(super) browser: Option<FileBrowser>,
    pub(super) settings_form: Option<SettingsForm>,
    /// The hash of each settings file when the settings window opened.
    pub(super) settings_hashes: Vec<(crate::store::Scope, Option<u64>)>,
    pub(super) export_form: Option<ExportForm>,
    pub(super) edit_form: Option<crate::edit_form::EditForm>,
    pub(super) import_form: Option<ImportForm>,
    pub(super) compare_form: Option<CompareForm>,
    pub(super) picker: Option<Picker>,
    pub(super) help: Option<Help>,
    pub(super) overlay: Option<Overlay>,
    pub(super) form: Option<ConnectionForm>,
}

impl Modals {
    /// Returns true when the window is open. The password prompt lives in `auth`.
    fn is_open(&self, modal: Modal) -> bool {
        match modal {
            Modal::Dialog => !self.dialogs.is_empty(),
            Modal::Password => false,
            Modal::Conflicts => self.conflicts.is_some(),
            Modal::Palette => self.palette.is_some(),
            Modal::History => self.history.is_some(),
            Modal::Rename => self.rename.is_some(),
            Modal::Browser => self.browser.is_some(),
            Modal::Settings => self.settings_form.is_some(),
            Modal::Export => self.export_form.is_some(),
            Modal::Edit => self.edit_form.is_some(),
            Modal::Import => self.import_form.is_some(),
            Modal::Compare => self.compare_form.is_some(),
            Modal::Picker => self.picker.is_some(),
            Modal::Help => self.help.is_some(),
            Modal::Overlay => self.overlay.is_some(),
            Modal::Form => self.form.is_some(),
        }
    }
}

impl App {
    pub(super) fn is_open(&self, modal: Modal) -> bool {
        match modal {
            Modal::Password => self.auth.prompt.is_some(),
            _ => self.modals.is_open(modal),
        }
    }

    /// Returns the open window on top.
    pub(super) fn top_modal(&self) -> Option<Modal> {
        Modal::ALL.into_iter().find(|modal| self.is_open(*modal))
    }
}
