//! The scripts that another program changed or removed. One question lists them all,
//! and each row needs a choice. The autosave of a listed script waits.

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use super::App;
use crate::script_file::Trouble;
use crate::ui::text;

pub(super) struct ConflictRow {
    /// The identity of the script. A console can change its number, but not this.
    pub script: String,
    pub name: String,
    pub removed: bool,
    /// The index in `options`. `None` waits for the user.
    pub choice: Option<usize>,
}

impl ConflictRow {
    pub fn options(&self) -> [&'static str; 2] {
        if self.removed {
            [text::CHOICE_RECREATE, text::CHOICE_CLOSE]
        } else {
            [text::CHOICE_RELOAD, text::CHOICE_OVERWRITE]
        }
    }
}

pub(super) struct Conflicts {
    pub rows: Vec<ConflictRow>,
    /// The rows come first. Then come the button that applies, and the button that waits.
    pub cursor: usize,
    pub note: Option<&'static str>,
}

impl Conflicts {
    fn positions(&self) -> usize {
        self.rows.len() + 2
    }

    fn cycle(&mut self, row: usize) {
        if let Some(item) = self.rows.get_mut(row) {
            item.choice = Some(item.choice.map_or(0, |choice| (choice + 1) % 2));
        }
    }
}

impl App {
    /// Opens the question for the scripts in trouble that the user did not see yet.
    /// It returns true when the question opened now.
    pub(super) fn open_conflicts(&mut self) -> bool {
        if self.conflicts.is_some() {
            return false;
        }
        let Some(persist) = &mut self.persist else { return false };
        let mut rows = Vec::new();
        for console in &self.consoles {
            let Some(file) = &console.file else { continue };
            let Some(trouble) = file.trouble.as_ref().filter(|trouble| trouble.needs_choice()) else {
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
        self.conflicts = Some(Conflicts { rows, cursor: 0, note: None });
        true
    }

    pub(super) fn on_conflicts_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        let Some(conflicts) = &mut self.conflicts else { return };
        let apply = conflicts.rows.len();
        let on_row = conflicts.cursor < apply;
        let positions = conflicts.positions();
        match key.code {
            KeyCode::Esc => self.conflicts = None,
            KeyCode::Down | KeyCode::Tab => conflicts.cursor = (conflicts.cursor + 1) % positions,
            KeyCode::Up | KeyCode::BackTab => {
                conflicts.cursor = (conflicts.cursor + positions - 1) % positions;
            }
            KeyCode::Char('j') if vim => conflicts.cursor = (conflicts.cursor + 1) % positions,
            KeyCode::Char('k') if vim => {
                conflicts.cursor = (conflicts.cursor + positions - 1) % positions;
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if on_row => {
                let row = conflicts.cursor;
                conflicts.cycle(row);
            }
            KeyCode::Char('h' | 'l') if vim && on_row => {
                let row = conflicts.cursor;
                conflicts.cycle(row);
            }
            KeyCode::Left | KeyCode::Right if !on_row => {
                conflicts.cursor = if conflicts.cursor == apply { apply + 1 } else { apply };
            }
            KeyCode::Enter if on_row => {
                let row = conflicts.cursor;
                conflicts.cycle(row);
                conflicts.cursor = (row + 1).min(apply);
            }
            KeyCode::Enter if conflicts.cursor == apply => self.apply_conflicts(),
            KeyCode::Enter => self.conflicts = None,
            _ => {}
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
        let Some(conflicts) = &mut self.conflicts else { return };
        if let Some(row) = row {
            conflicts.cursor = row;
            conflicts.cycle(row);
        } else if apply {
            self.apply_conflicts();
        } else if later {
            self.conflicts = None;
        }
    }

    /// Carries out the choices. The question stays open until every row has a choice.
    fn apply_conflicts(&mut self) {
        let Some(conflicts) = &mut self.conflicts else { return };
        if conflicts.rows.iter().any(|row| row.choice.is_none()) {
            conflicts.note = Some(text::CONFLICT_CHOOSE);
            return;
        }
        let Some(conflicts) = self.conflicts.take() else { return };
        for row in conflicts.rows {
            if let Some(persist) = &mut self.persist {
                persist.presented.remove(&row.script);
            }
            let found = self
                .consoles
                .iter()
                .position(|c| c.file.as_ref().is_some_and(|file| file.meta.id == row.script));
            let Some(index) = found else { continue };
            // The first choice reloads a changed script, and it recreates a removed one.
            match (row.removed, row.choice) {
                (false, Some(0)) => self.reload_script(index),
                (false, Some(_)) | (true, Some(0)) => self.overwrite_script(index),
                _ => {
                    let console = &mut self.consoles[index];
                    console.discard_text = true;
                    let id = console.id;
                    self.close_step(id);
                }
            }
        }
    }
}
