//! The question about the scripts in trouble, without `App`: its rows, its cursor, its
//! keys, and the action that each choice asks for.

use crossterm::event::{KeyCode, KeyEvent};

use crate::ui::text;

pub(in crate::app) struct ConflictRow {
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

    /// The action of the choice. The first choice reloads a changed script, and it
    /// recreates a removed one.
    pub(super) fn action(&self) -> ConflictAction {
        match (self.removed, self.choice) {
            (false, Some(0)) => ConflictAction::Reload,
            (false, Some(_)) | (true, Some(0)) => ConflictAction::Overwrite,
            _ => ConflictAction::Close,
        }
    }
}

/// What a row asks of its console.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ConflictAction {
    /// The text of the file replaces the text of the console.
    Reload,
    /// The text of the console replaces the file, or creates it again.
    Overwrite,
    /// The console closes without its text.
    Close,
}

/// What a key asks of the question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ConflictRequest {
    None,
    /// The question closes, and asks again later.
    Later,
    /// The choices apply.
    Apply,
}

pub(in crate::app) struct Conflicts {
    pub rows: Vec<ConflictRow>,
    /// The rows come first. Then come the button that applies, and the button that waits.
    pub cursor: usize,
    pub note: Option<&'static str>,
}

impl Conflicts {
    pub(super) fn new(rows: Vec<ConflictRow>) -> Self {
        Self {
            rows,
            cursor: 0,
            note: None,
        }
    }

    fn positions(&self) -> usize {
        self.rows.len() + 2
    }

    pub(super) fn cycle(&mut self, row: usize) {
        if let Some(item) = self.rows.get_mut(row) {
            item.choice = Some(item.choice.map_or(0, |choice| (choice + 1) % 2));
        }
    }

    fn step(&mut self, forward: bool) {
        let positions = self.positions();
        self.cursor = if forward {
            (self.cursor + 1) % positions
        } else {
            (self.cursor + positions - 1) % positions
        };
    }

    pub(super) fn on_key(&mut self, key: KeyEvent, vim: bool) -> ConflictRequest {
        let apply = self.rows.len();
        let on_row = self.cursor < apply;
        match key.code {
            KeyCode::Esc => return ConflictRequest::Later,
            KeyCode::Down | KeyCode::Tab => self.step(true),
            KeyCode::Up | KeyCode::BackTab => self.step(false),
            KeyCode::Char('j') if vim => self.step(true),
            KeyCode::Char('k') if vim => self.step(false),
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if on_row => {
                self.cycle(self.cursor)
            }
            KeyCode::Char('h' | 'l') if vim && on_row => self.cycle(self.cursor),
            KeyCode::Left | KeyCode::Right if !on_row => {
                self.cursor = if self.cursor == apply {
                    apply + 1
                } else {
                    apply
                };
            }
            KeyCode::Enter if on_row => {
                let row = self.cursor;
                self.cycle(row);
                self.cursor = (row + 1).min(apply);
            }
            KeyCode::Enter if self.cursor == apply => return ConflictRequest::Apply,
            KeyCode::Enter => return ConflictRequest::Later,
            _ => {}
        }
        ConflictRequest::None
    }

    /// Returns the reason that the choices cannot apply yet: each row needs a choice.
    pub(super) fn missing_choice(&self) -> Option<&'static str> {
        self.rows
            .iter()
            .any(|row| row.choice.is_none())
            .then_some(text::CONFLICT_CHOOSE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(removed: bool) -> ConflictRow {
        ConflictRow {
            script: "s".to_owned(),
            name: "n".to_owned(),
            removed,
            choice: None,
        }
    }

    fn press(conflicts: &mut Conflicts, code: KeyCode, vim: bool) -> ConflictRequest {
        conflicts.on_key(KeyEvent::from(code), vim)
    }

    #[test]
    fn enter_chooses_each_row_and_then_applies() {
        let mut conflicts = Conflicts::new(vec![row(false), row(true)]);
        assert_eq!(conflicts.missing_choice(), Some(text::CONFLICT_CHOOSE));
        assert_eq!(
            press(&mut conflicts, KeyCode::Enter, false),
            ConflictRequest::None
        );
        assert_eq!(
            press(&mut conflicts, KeyCode::Enter, false),
            ConflictRequest::None
        );
        assert_eq!(conflicts.cursor, 2, "the cursor reaches the apply button");
        assert_eq!(conflicts.missing_choice(), None);
        assert_eq!(
            press(&mut conflicts, KeyCode::Enter, false),
            ConflictRequest::Apply
        );
        assert_eq!(conflicts.rows[0].action(), ConflictAction::Reload);
        assert_eq!(conflicts.rows[1].action(), ConflictAction::Overwrite);
    }

    #[test]
    fn the_cursor_wraps_and_vim_keys_work_only_in_vim_mode() {
        let mut conflicts = Conflicts::new(vec![row(false)]);
        press(&mut conflicts, KeyCode::Up, false);
        assert_eq!(
            conflicts.cursor, 2,
            "up from the first row reaches the later button"
        );
        press(&mut conflicts, KeyCode::Char('j'), false);
        assert_eq!(conflicts.cursor, 2);
        press(&mut conflicts, KeyCode::Char('j'), true);
        assert_eq!(conflicts.cursor, 0);
        press(&mut conflicts, KeyCode::Char('l'), true);
        assert_eq!(conflicts.rows[0].choice, Some(0));
        press(&mut conflicts, KeyCode::Char(' '), false);
        assert_eq!(conflicts.rows[0].choice, Some(1));
        assert_eq!(conflicts.rows[0].action(), ConflictAction::Overwrite);
    }

    #[test]
    fn left_and_right_switch_the_buttons_and_esc_waits() {
        let mut conflicts = Conflicts::new(vec![row(true)]);
        conflicts.cursor = 1;
        press(&mut conflicts, KeyCode::Right, false);
        assert_eq!(conflicts.cursor, 2);
        assert_eq!(
            press(&mut conflicts, KeyCode::Enter, false),
            ConflictRequest::Later
        );
        assert_eq!(
            press(&mut conflicts, KeyCode::Esc, false),
            ConflictRequest::Later
        );
    }

    #[test]
    fn a_removed_script_closes_with_its_second_choice() {
        let mut removed = row(true);
        removed.choice = Some(1);
        assert_eq!(removed.action(), ConflictAction::Close);
        assert_eq!(
            removed.options(),
            [text::CHOICE_RECREATE, text::CHOICE_CLOSE]
        );
    }
}
