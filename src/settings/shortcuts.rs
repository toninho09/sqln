//! Shortcut selection and key recording for the settings dialog.

use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use super::{Field, SettingsForm, next_choice};
use crate::editor::KeyboardMode;
use crate::input::{Action, Chord, Region};
use crate::store::Scope;
use crate::store::config::{self, KeyboardName, Preferences};
use crate::ui::text;

pub const FIELDS: &[Field] = &[
    Field::ShortcutMode,
    Field::ShortcutCommands,
    Field::ShortcutBinding,
    Field::ShortcutFinish,
    Field::ShortcutRecord,
    Field::ShortcutRemove,
    Field::ShortcutRestore,
    Field::ShortcutBack,
];

const LIST_FIELDS: &[Field] = &[Field::ShortcutMode, Field::ShortcutCommands];
const EDIT_FIELDS: &[Field] = &[
    Field::ShortcutMode,
    Field::ShortcutBinding,
    Field::ShortcutFinish,
    Field::ShortcutRecord,
    Field::ShortcutRemove,
    Field::ShortcutRestore,
    Field::ShortcutBack,
];

pub struct Shortcuts {
    pub mode: KeyboardName,
    pub action: Action,
    pub(super) editing: bool,
    pub binding: usize,
    pub mouse_confirm: bool,
    recording: Option<Vec<String>>,
}

impl Shortcuts {
    pub fn new(mode: Option<KeyboardName>) -> Self {
        Self {
            mode: mode.unwrap_or(KeyboardName::Conventional),
            action: Action::Run,
            editing: false,
            binding: 0,
            mouse_confirm: false,
            recording: None,
        }
    }

    pub fn actions() -> impl Iterator<Item = Action> {
        [
            Region::Global,
            Region::Navigator,
            Region::Grid,
            Region::Diagram,
        ]
        .into_iter()
        .flat_map(Action::in_region)
    }

    pub fn is_editing(&self) -> bool {
        self.editing
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    pub fn recorded(&self) -> String {
        self.recording
            .as_ref()
            .map(|keys| keys.join(" "))
            .unwrap_or_default()
    }

    fn keyboard(&self) -> KeyboardMode {
        match self.mode {
            KeyboardName::Conventional => KeyboardMode::Conventional,
            KeyboardName::Vim => KeyboardMode::Vim,
        }
    }

    fn overrides<'a>(
        &self,
        preferences: &'a Preferences,
    ) -> Option<&'a BTreeMap<String, Vec<String>>> {
        preferences.keymap.as_ref().map(|maps| match self.mode {
            KeyboardName::Conventional => &maps.conventional,
            KeyboardName::Vim => &maps.vim,
        })
    }
}

impl SettingsForm {
    /// Returns the keys at the selected scope, including its inherited keys.
    pub fn shortcut_keys(&self) -> Vec<String> {
        self.shortcut_keys_for(self.shortcuts.action)
    }

    /// Lists every command, including commands without a shortcut.
    pub fn shortcut_commands(&self) -> Vec<(Action, Vec<String>)> {
        Shortcuts::actions()
            .map(|action| (action, self.shortcut_keys_for(action)))
            .collect()
    }

    fn shortcut_keys_for(&self, action: Action) -> Vec<String> {
        let local = self.draft().keymap.as_ref().and_then(|maps| {
            let map = match self.shortcuts.mode {
                KeyboardName::Conventional => &maps.conventional,
                KeyboardName::Vim => &maps.vim,
            };
            map.get(action.name())
        });
        if let Some(keys) = local {
            return keys.clone();
        }
        if self.scope == Scope::Project
            && let Some(keys) = self
                .shortcuts
                .overrides(&self.global)
                .and_then(|map| map.get(action.name()))
        {
            return keys.clone();
        }
        action.default_keys(self.shortcuts.keyboard())
    }

    pub(crate) fn shortcut_stops(&self) -> &'static [Field] {
        if self.shortcuts.editing {
            EDIT_FIELDS
        } else {
            LIST_FIELDS
        }
    }

    pub fn edit_shortcut(&mut self, action: Action) {
        if self.shortcuts.is_recording() {
            return;
        }
        self.shortcuts.action = action;
        self.shortcuts.binding = 0;
        self.shortcuts.editing = true;
        self.focus = Field::ShortcutBinding;
        self.note = None;
    }

    pub fn move_shortcut_command(&mut self, delta: isize) {
        if self.shortcuts.is_recording() || self.shortcuts.editing {
            return;
        }
        let actions: Vec<_> = Shortcuts::actions().collect();
        let index = actions
            .iter()
            .position(|action| *action == self.shortcuts.action)
            .unwrap_or(0);
        let next = index.saturating_add_signed(delta).min(actions.len() - 1);
        self.shortcuts.action = actions[next];
        self.shortcuts.binding = 0;
        self.focus = Field::ShortcutCommands;
    }

    fn back_to_commands(&mut self) {
        self.shortcuts.editing = false;
        self.focus = Field::ShortcutCommands;
    }

    pub fn shortcut_value(&self, field: Field) -> String {
        match field {
            Field::ShortcutMode => text::keyboard_name(self.shortcuts.mode),
            Field::ShortcutBack => text::SHORTCUT_BACK.to_owned(),
            Field::ShortcutBinding => self
                .shortcut_keys()
                .get(self.shortcuts.binding)
                .map_or_else(|| text::SHORTCUT_ADD.to_owned(), |key| text::key_label(key)),
            Field::ShortcutFinish if self.shortcuts.mouse_confirm => {
                text::SHORTCUT_MOUSE_OK.to_owned()
            }
            Field::ShortcutFinish => "Enter".to_owned(),
            Field::ShortcutRecord => text::SHORTCUT_RECORD.to_owned(),
            Field::ShortcutRemove => text::SHORTCUT_REMOVE.to_owned(),
            Field::ShortcutRestore => text::SHORTCUT_RESTORE.to_owned(),
            _ => String::new(),
        }
    }

    fn change_shortcut(&mut self, forward: bool) {
        match self.focus {
            Field::ShortcutMode => {
                self.shortcuts.mode = next_choice(
                    &[KeyboardName::Conventional, KeyboardName::Vim],
                    self.shortcuts.mode,
                    forward,
                );
                self.shortcuts.binding = 0;
            }
            Field::ShortcutBinding => {
                let choices: Vec<_> = (0..=self.shortcut_keys().len()).collect();
                self.shortcuts.binding = next_choice(&choices, self.shortcuts.binding, forward);
            }
            Field::ShortcutFinish => self.shortcuts.mouse_confirm = !self.shortcuts.mouse_confirm,
            _ => {}
        }
    }

    pub(super) fn shortcut_key(&mut self, key: KeyEvent, vim: bool) -> bool {
        if self.shortcuts.editing
            && key.code == KeyCode::Esc
            && super::SECTIONS[self.section]
                .fields
                .contains(&Field::ShortcutCommands)
        {
            self.back_to_commands();
            return true;
        }
        if self.focus == Field::ShortcutCommands {
            match key.code {
                KeyCode::Up => self.move_shortcut_command(-1),
                KeyCode::Down => self.move_shortcut_command(1),
                KeyCode::Char('k') if vim => self.move_shortcut_command(-1),
                KeyCode::Char('j') if vim => self.move_shortcut_command(1),
                KeyCode::PageUp => self.move_shortcut_command(-8),
                KeyCode::PageDown => self.move_shortcut_command(8),
                KeyCode::Home => self.move_shortcut_command(isize::MIN),
                KeyCode::End => self.move_shortcut_command(isize::MAX),
                KeyCode::Enter => self.edit_shortcut(self.shortcuts.action),
                _ => return false,
            }
            return true;
        }
        if !FIELDS.contains(&self.focus) {
            return false;
        }
        match key.code {
            KeyCode::Enter
                if matches!(
                    self.focus,
                    Field::ShortcutRecord
                        | Field::ShortcutRemove
                        | Field::ShortcutRestore
                        | Field::ShortcutBack
                ) =>
            {
                self.shortcut_press();
            }
            KeyCode::Left => self.change_shortcut(false),
            KeyCode::Right => self.change_shortcut(true),
            KeyCode::Char('h') if vim => self.change_shortcut(false),
            KeyCode::Char('l') if vim => self.change_shortcut(true),
            _ => return false,
        }
        true
    }

    pub(super) fn shortcut_press(&mut self) -> bool {
        if self.shortcuts.is_recording() {
            match self.focus {
                Field::ShortcutOk => self.finish_shortcut(),
                Field::ShortcutCancel => self.cancel_shortcut(),
                _ => {}
            }
            return true;
        }
        match self.focus {
            Field::ShortcutCommands => self.edit_shortcut(self.shortcuts.action),
            Field::ShortcutBack => self.back_to_commands(),
            Field::ShortcutRecord => {
                self.shortcuts.editing = true;
                self.note = None;
                self.shortcuts.recording = Some(Vec::new());
                self.focus = Field::ShortcutOk;
            }
            Field::ShortcutRemove => {
                let mut keys = self.shortcut_keys();
                if self.shortcuts.binding < keys.len() {
                    keys.remove(self.shortcuts.binding);
                    self.set_shortcut(Some(keys));
                }
            }
            Field::ShortcutRestore => {
                self.set_shortcut(None);
                if self.note.is_none() {
                    self.note = Some(text::SHORTCUT_RESTORED.to_owned());
                }
            }
            field if FIELDS.contains(&field) => self.change_shortcut(true),
            _ => return false,
        }
        true
    }

    pub(super) fn record_shortcut(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        self.note = None;
        if key.code == KeyCode::Esc && key.modifiers.is_empty() {
            self.cancel_shortcut();
        } else if key.code == KeyCode::Enter
            && key.modifiers.is_empty()
            && !self.shortcuts.mouse_confirm
        {
            self.finish_shortcut();
        } else {
            match Chord::from_event(key).text() {
                Ok(key) => self.shortcuts.recording.as_mut().unwrap().push(key),
                Err(message) => self.note = Some(message),
            }
        }
    }

    fn cancel_shortcut(&mut self) {
        self.shortcuts.recording = None;
        self.focus = Field::ShortcutRecord;
        self.note = None;
    }

    fn finish_shortcut(&mut self) {
        let keys = self.shortcuts.recording.as_ref().unwrap();
        if keys.is_empty() {
            self.note = Some(text::SHORTCUT_EMPTY.to_owned());
            return;
        }
        let sequence = keys.join(" ");
        let mut keys = self.shortcut_keys();
        let index = self.shortcuts.binding.min(keys.len());
        if index == keys.len() {
            keys.push(sequence);
        } else {
            keys[index] = sequence;
        }
        self.shortcuts.recording = None;
        self.focus = Field::ShortcutRecord;
        self.set_shortcut(Some(keys));
    }

    fn set_shortcut(&mut self, keys: Option<Vec<String>>) {
        let action = self.shortcuts.action.name();
        let mode = self.shortcuts.mode;
        let draft = self.draft_mut();
        let maps = draft.keymap.get_or_insert_default();
        let map = match mode {
            KeyboardName::Conventional => &mut maps.conventional,
            KeyboardName::Vim => &mut maps.vim,
        };
        match keys {
            Some(keys) => {
                map.insert(action.to_owned(), keys);
            }
            None => {
                map.remove(action);
            }
        }
        if maps.conventional.is_empty() && maps.vim.is_empty() {
            draft.keymap = None;
        }
        self.shortcuts.binding = self.shortcuts.binding.min(self.shortcut_keys().len());
        let mut preferences = match (self.scope, &self.project) {
            (Scope::Project, Some(project)) => project.clone(),
            _ => self.global.clone(),
        };
        preferences.keymap = self.draft().keymap.clone();
        self.note = self.validate_shortcuts(&preferences).err();
    }

    pub(super) fn validate_shortcuts(&self, preferences: &Preferences) -> Result<(), String> {
        if self.shortcuts.is_recording() {
            return Err(text::SHORTCUT_FINISH_FIRST.to_owned());
        }
        match self.scope {
            Scope::Global => config::validate_keymaps(preferences, self.project.as_ref()),
            Scope::Project => config::validate_keymaps(&self.global, Some(preferences)),
        }
    }
}
