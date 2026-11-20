//! The scripts that another program changed or removed. One question lists them all,
//! and each row needs a choice. The autosave of a listed script waits.

use crossterm::event::{KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

mod rules;

pub(super) use self::rules::Conflicts;
use self::rules::{ConflictAction, ConflictRequest, ConflictRow};
use super::App;
use crate::script_file::Trouble;

impl App {
    /// Opens the question for the scripts in trouble that the user did not see yet.
    /// It returns true when the question opened now.
    pub(super) fn open_conflicts(&mut self) -> bool {
        if self.modals.conflicts.is_some() {
            return false;
        }
        let Some(persist) = &mut self.persist else {
            return false;
        };
        let mut rows = Vec::new();
        for console in &self.tabs.consoles {
            let Some(file) = &console.file else { continue };
            if file.is_saving() || persist.saves.contains_key(&console.id) {
                continue;
            }
            let Some(trouble) = file
                .trouble
                .as_ref()
                .filter(|trouble| trouble.needs_choice())
            else {
                continue;
            };
            if persist.presented.insert(file.meta.id.clone()) {
                rows.push(ConflictRow {
                    script: file.meta.id.clone(),
                    name: console.name.clone(),
                    removed: *trouble == Trouble::Removed,
                    choice: None,
                });
            }
        }
        if rows.is_empty() {
            return false;
        }
        self.modals.conflicts = Some(Conflicts::new(rows));
        true
    }

    pub(super) fn on_conflicts_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        let Some(conflicts) = &mut self.modals.conflicts else {
            return;
        };
        match conflicts.on_key(key, vim) {
            ConflictRequest::None => {}
            ConflictRequest::Later => self.modals.conflicts = None,
            ConflictRequest::Apply => self.apply_conflicts(),
        }
    }

    pub(super) fn on_conflicts_mouse(&mut self, mouse: MouseEvent) {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return;
        }
        let position = Position::new(mouse.column, mouse.row);
        let layout = &self.layout.conflicts;
        let row = layout.rows.iter().position(|area| area.contains(position));
        let apply = layout.buttons[0].contains(position);
        let later = layout.buttons[1].contains(position);
        let Some(conflicts) = &mut self.modals.conflicts else {
            return;
        };
        if let Some(row) = row {
            conflicts.cursor = row;
            conflicts.cycle(row);
        } else if apply {
            self.apply_conflicts();
        } else if later {
            self.modals.conflicts = None;
        }
    }

    /// Carries out the choices. The question stays open until every row has a choice.
    fn apply_conflicts(&mut self) {
        let Some(conflicts) = &mut self.modals.conflicts else {
            return;
        };
        if let Some(note) = conflicts.missing_choice() {
            conflicts.note = Some(note);
            return;
        }
        let Some(conflicts) = self.modals.conflicts.take() else {
            return;
        };
        for row in conflicts.rows {
            if let Some(persist) = &mut self.persist {
                persist.presented.remove(&row.script);
            }
            let found = self.tabs.consoles.iter().position(|c| {
                c.file
                    .as_ref()
                    .is_some_and(|file| file.meta.id == row.script)
            });
            let Some(index) = found else { continue };
            match row.action() {
                ConflictAction::Reload => self.reload_script(index),
                ConflictAction::Overwrite => self.overwrite_script(index),
                ConflictAction::Close => {
                    let console = &mut self.tabs.consoles[index];
                    console.discard_text = true;
                    let id = console.id;
                    self.close_step(id);
                }
            }
        }
    }
}
