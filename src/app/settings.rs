//! Writing the settings and the theme files.

use super::App;
use crate::settings::themes::ThemeChanges;
use crate::store::Scope;
use crate::store::config;
use crate::store::themes;
use crate::ui::text;

impl App {
    /// Writes the scope of the dialog. The caller built and validated the values.
    pub(super) fn write_settings(&mut self, preferences: config::Preferences, scope: Scope) {
        let Some(store) = self.persist.as_ref().map(|persist| persist.store.clone()) else {
            return;
        };
        // Use the current files for the other scope, not only the dialog snapshot.
        let validation = match scope {
            Scope::Global => store
                .project
                .as_ref()
                .map(|root| config::preferences(root, Scope::Project))
                .transpose()
                .and_then(|project| config::validate_keymaps(&preferences, project.as_ref())),
            Scope::Project => config::preferences(&store.global, Scope::Global)
                .and_then(|global| config::validate_keymaps(&global, Some(&preferences))),
        };
        if let Err(message) = validation {
            if let Some(form) = &mut self.modals.settings_form {
                form.note = Some(message);
            }
            return;
        }
        // Write new and edited themes before preferences, and remove old ones only
        // after preferences no longer refer to them.
        let changes = self
            .modals
            .settings_form
            .as_ref()
            .map(|form| form.theme_changes())
            .unwrap_or_default();
        if let Err(message) = self.write_theme_files(&changes) {
            if let Some(form) = &mut self.modals.settings_form {
                form.note = Some(message);
            }
            return;
        }
        let Some(root) = store.root(scope) else {
            return;
        };
        let known = self
            .modals
            .settings_hashes
            .iter()
            .find(|(item, _)| *item == scope)
            .and_then(|(_, hash)| *hash);
        match config::change_preferences(root, scope, preferences, known) {
            Ok(hash) => {
                if let Some(persist) = &mut self.persist
                    && let Some((_, known)) =
                        persist.hashes.iter_mut().find(|(item, _)| *item == scope)
                {
                    *known = Some(hash);
                }
                if let Err(message) = self.remove_theme_files(&changes) {
                    if let Some(form) = &mut self.modals.settings_form {
                        form.note = Some(format!(
                            "Settings were saved, but a theme could not be removed: {message} Retry."
                        ));
                    }
                    return;
                }
                self.modals.settings_form = None;
                self.reload_config();
                if self
                    .persist
                    .as_ref()
                    .is_some_and(|persist| persist.error.is_none())
                {
                    self.status = Some(text::SETTINGS_SAVED.to_owned());
                }
            }
            Err(message) => {
                if let Some(form) = &mut self.modals.settings_form {
                    form.note = Some(message);
                }
            }
        }
    }

    /// Writes the theme files of the dialog. Record each success immediately so
    /// that a retry after a later failure does not conflict with our own write.
    pub(super) fn write_theme_files(&mut self, changes: &ThemeChanges) -> Result<(), String> {
        let Some(store) = self.persist.as_ref().map(|persist| persist.store.clone()) else {
            return Ok(());
        };
        let root = |scope: Scope| -> Result<crate::store::Root, String> {
            store
                .root(scope)
                .cloned()
                .ok_or_else(|| "The scope has no folder.".to_owned())
        };
        for theme in &changes.saved {
            let target = root(theme.scope)?;
            let hash = themes::save(
                &target,
                &theme.id,
                &theme.name,
                &theme.scheme.to_colors(),
                theme.file,
            )?;
            if let Some(form) = &mut self.modals.settings_form {
                form.theme_written(&theme.id, hash);
            }
        }
        if let Some(persist) = &mut self.persist {
            persist.theme_hashes = super::persist::theme_manifests(&persist.store);
        }
        Ok(())
    }

    pub(super) fn remove_theme_files(&mut self, changes: &ThemeChanges) -> Result<(), String> {
        let Some(persist) = &mut self.persist else {
            return Ok(());
        };
        for theme in &changes.removed {
            let root = persist
                .store
                .root(theme.scope)
                .ok_or_else(|| "The scope has no folder.".to_owned())?;
            if let Some(hash) = theme.file {
                themes::remove(root, &theme.id, hash)?;
            }
        }
        persist.theme_hashes = super::persist::theme_manifests(&persist.store);
        Ok(())
    }
}
