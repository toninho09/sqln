//! The windows over the screen, and the one order that decides which window gets the
//! keys, the mouse, the paste, and the hint, and which window is drawn on top.

use super::App;

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

impl App {
    pub(super) fn is_open(&self, modal: Modal) -> bool {
        match modal {
            Modal::Dialog => !self.dialogs.is_empty(),
            Modal::Password => self.auth.prompt.is_some(),
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

    /// Returns the open window on top.
    pub(super) fn top_modal(&self) -> Option<Modal> {
        Modal::ALL.into_iter().find(|modal| self.is_open(*modal))
    }
}
